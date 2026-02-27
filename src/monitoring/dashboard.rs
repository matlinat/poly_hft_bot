use std::time::Duration;

use tokio::net::TcpListener;
use tokio::io::AsyncWriteExt;
use tokio::select;
use tokio::time::interval;

use crate::monitoring::metrics::{log_metrics_snapshot, METRICS};

/// Spawn a background task that periodically logs a compact metrics snapshot.
///
/// This provides a simple terminal "dashboard" when combined with `tracing`
/// JSON logs and `jq`/`grep` on the operator side.
pub fn spawn_dashboard_task(period: Duration) {
    let mut ticker = interval(period);
    tokio::spawn(async move {
        loop {
            ticker.tick().await;
            let snapshot = METRICS.snapshot();
            log_metrics_snapshot(&snapshot);
        }
    });
}

/// Simple health-check TCP listener exposing an HTTP-style `/health` endpoint.
///
/// This is intentionally minimal and dependency-light; it responds with a
/// single `200 OK` if the in-process metrics consider the bot healthy.
pub async fn serve_health(addr: &str, max_staleness: Duration) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;

    loop {
        let (mut socket, _) = listener.accept().await?;
        let mut buf = [0u8; 1024];

        // Best-effort read of the incoming request; we don't inspect the path
        // in detail but this keeps the interface roughly HTTP-compatible.
        let _ = socket.readable().await;
        let _ = socket.try_read(&mut buf);

        let healthy = METRICS.is_healthy(max_staleness);
        let body = if healthy { "OK" } else { "STALE" };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: text/plain\r\n\r\n{}",
            body.len(),
            body
        );

        socket.write_all(response.as_bytes()).await?;
        socket.shutdown().await?;
    }
}

/// Convenience helper that runs both the dashboard logger and health server
/// until a shutdown signal is received.
pub async fn run_monitoring(
    health_addr: &str,
    dashboard_period: Duration,
    max_staleness: Duration,
    mut shutdown: tokio::sync::oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    spawn_dashboard_task(dashboard_period);

    select! {
        res = serve_health(health_addr, max_staleness) => {
            res
        }
        _ = &mut shutdown => {
            Ok(())
        }
    }
}

