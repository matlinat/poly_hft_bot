use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures::{stream::SplitStream, SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::interval;
use tokio_tungstenite::{
    connect_async,
    tungstenite::protocol::Message,
    WebSocketStream,
};

use super::{ClientError, ClientResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Connected,
    Reconnecting,
    Disconnected,
}

impl From<u8> for ConnectionState {
    fn from(value: u8) -> Self {
        match value {
            0 => ConnectionState::Connecting,
            1 => ConnectionState::Connected,
            2 => ConnectionState::Reconnecting,
            _ => ConnectionState::Disconnected,
        }
    }
}

impl From<ConnectionState> for u8 {
    fn from(value: ConnectionState) -> Self {
        match value {
            ConnectionState::Connecting => 0,
            ConnectionState::Connected => 1,
            ConnectionState::Reconnecting => 2,
            ConnectionState::Disconnected => 3,
        }
    }
}

pub struct WebSocketConnection {
    url: String,
    outbound_tx: mpsc::UnboundedSender<Message>,
    inbound_rx: mpsc::UnboundedReceiver<Message>,
    state: Arc<AtomicU8>,
}

impl WebSocketConnection {
    pub fn sender(&self) -> mpsc::UnboundedSender<Message> {
        self.outbound_tx.clone()
    }

    pub fn receiver(&mut self) -> &mut mpsc::UnboundedReceiver<Message> {
        &mut self.inbound_rx
    }

    pub fn state(&self) -> ConnectionState {
        self.state.load(Ordering::SeqCst).into()
    }
}

async fn handle_connection(
    url: &str,
    outbound_rx: &mut mpsc::UnboundedReceiver<Message>,
    inbound_tx: &mpsc::UnboundedSender<Message>,
    state: &Arc<AtomicU8>,
) -> ClientResult<()> {
    let (ws_stream, _) = connect_async(url).await?;
    state.store(ConnectionState::Connected.into(), Ordering::SeqCst);

    let (mut write, mut read) = ws_stream.split();
    let mut heartbeat = interval(Duration::from_secs(10));

    loop {
        tokio::select! {
            Some(msg) = outbound_rx.recv() => {
                if let Err(err) = write.send(msg).await {
                    state.store(ConnectionState::Reconnecting.into(), Ordering::SeqCst);
                    return Err(ClientError::WebSocket(err));
                }
            }
            maybe_msg = read.next() => {
                match maybe_msg {
                    Some(Ok(msg)) => {
                        if inbound_tx.send(msg).is_err() {
                            // receiver dropped; treat as graceful shutdown
                            state.store(ConnectionState::Disconnected.into(), Ordering::SeqCst);
                            return Ok(());
                        }
                    }
                    Some(Err(err)) => {
                        state.store(ConnectionState::Reconnecting.into(), Ordering::SeqCst);
                        return Err(ClientError::WebSocket(err));
                    }
                    None => {
                        state.store(ConnectionState::Reconnecting.into(), Ordering::SeqCst);
                        return Ok(());
                    }
                }
            }
            _ = heartbeat.tick() => {
                if let Err(err) = write.send(Message::Text("PING".to_string())).await {
                    state.store(ConnectionState::Reconnecting.into(), Ordering::SeqCst);
                    return Err(ClientError::WebSocket(err));
                }
            }
        }
    }
}

/// Connect to a Polymarket WebSocket endpoint with automatic heartbeats and reconnection.
///
/// This spawns a background task that:
/// - Maintains the TCP/WebSocket connection.
/// - Sends `PING` heartbeats every 10 seconds.
/// - Reconnects with exponential backoff if the connection drops.
///
/// The returned `WebSocketConnection` exposes:
/// - A sender for outbound messages (e.g. subscription payloads).
/// - A receiver for inbound messages.
/// - A connection state indicator.
pub fn connect_with_retries(url: impl Into<String>) -> WebSocketConnection {
    let url = url.into();
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel();
    let (inbound_tx, inbound_rx) = mpsc::unbounded_channel();
    let state = Arc::new(AtomicU8::new(ConnectionState::Connecting.into()));

    let url_for_task = url.clone();
    let state_clone = Arc::clone(&state);

    tokio::spawn(async move {
        let mut attempt: u32 = 0;
        loop {
            state_clone.store(ConnectionState::Connecting.into(), Ordering::SeqCst);

            match handle_connection(&url_for_task, &mut outbound_rx, &inbound_tx, &state_clone).await {
                Ok(()) => {
                    state_clone.store(ConnectionState::Disconnected.into(), Ordering::SeqCst);
                    break;
                }
                Err(_err) => {
                    attempt += 1;
                    let backoff_ms = 500u64.saturating_mul(1u64 << attempt.min(5));
                    tokio::time::sleep(Duration::from_millis(backoff_ms.min(8_000))).await;
                    state_clone.store(ConnectionState::Reconnecting.into(), Ordering::SeqCst);
                    continue;
                }
            }
        }
    });

    WebSocketConnection {
        url,
        outbound_tx,
        inbound_rx,
        state,
    }
}

