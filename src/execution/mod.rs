mod executor;
pub mod order;

use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;
use tokio_tungstenite::tungstenite::protocol::Message;
use tracing::{info, warn};

use crate::client::gamma::{resolve_15m_market, ResolvedMarket};
use crate::client::websocket::connect_with_retries;
use crate::monitoring::{dashboard, metrics::METRICS};
use crate::storage::{
    create_pg_pool,
    recorder::{SnapshotRecorder, TradeRecorder},
};
use crate::strategy::{LegSide, MarketSnapshot, TwoLegDecision, TwoLegEngine, TwoLegParams};
use crate::types::{AppConfig, MarketConfig};

pub use executor::{ExecutionError, ExecutionResult, OrderExecutor};

#[derive(Clone, Copy, Debug, Default)]
struct SideBook {
    best_bid: f64,
    best_ask: f64,
}

#[derive(Clone, Debug, Default)]
struct MarketBook {
    up: SideBook,
    down: SideBook,
}

#[derive(Debug, Deserialize)]
struct PriceChangeItem {
    asset_id: String,
    #[serde(default)]
    best_bid: Option<String>,
    #[serde(default)]
    best_ask: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PriceChangeEvent {
    _event_type: String,
    price_changes: Vec<PriceChangeItem>,
    timestamp: String,
}

#[derive(Debug, Deserialize)]
struct BestBidAskEvent {
    _event_type: String,
    asset_id: String,
    best_bid: String,
    best_ask: String,
    timestamp: String,
}

fn parse_millis_timestamp(ts: &str) -> DateTime<Utc> {
    ts.parse::<i64>()
        .ok()
        .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
        .unwrap_or_else(Utc::now)
}

fn update_book_and_build_snapshot(
    books_by_market: &mut HashMap<String, MarketBook>,
    market_slug: &str,
    side: LegSide,
    best_bid: f64,
    best_ask: f64,
    ts: DateTime<Utc>,
) -> Option<MarketSnapshot> {
    let book = books_by_market
        .entry(market_slug.to_string())
        .or_insert_with(MarketBook::default);

    let side_book = match side {
        LegSide::Up => &mut book.up,
        LegSide::Down => &mut book.down,
    };

    if best_bid > 0.0 {
        side_book.best_bid = best_bid;
    }
    if best_ask > 0.0 {
        side_book.best_ask = best_ask;
    }

    if book.up.best_bid > 0.0
        && book.up.best_ask > 0.0
        && book.down.best_bid > 0.0
        && book.down.best_ask > 0.0
    {
        Some(MarketSnapshot {
            ts,
            market_slug: market_slug.to_string(),
            up_bid: book.up.best_bid,
            up_ask: book.up.best_ask,
            down_bid: book.down.best_bid,
            down_ask: book.down.best_ask,
        })
    } else {
        None
    }
}

async fn process_snapshot(
    snapshot: MarketSnapshot,
    engine: &mut TwoLegEngine,
    executor: &mut OrderExecutor,
    snapshot_recorder: &SnapshotRecorder,
    trade_recorder: &TradeRecorder,
    available_capital: f64,
) -> Result<()> {
    let market_slug = snapshot.market_slug.clone();

    METRICS.record_snapshot(&market_slug);

    if let Err(err) = snapshot_recorder.record_snapshot(&snapshot).await {
        warn!(
            target: "storage",
            error = %err,
            market = %market_slug,
            "failed to record snapshot"
        );
    }

    let decisions = engine.on_snapshot(snapshot.clone(), available_capital);

    for decision in decisions {
        let (round_start, leg_label, expected_locked_profit) = match &decision {
            TwoLegDecision::OpenLeg1 { round_start, .. } => (*round_start, "leg1", None),
            TwoLegDecision::OpenLeg2 {
                round_start,
                expected_locked_profit,
                ..
            } => (*round_start, "leg2", Some(*expected_locked_profit)),
        };

        let market_for_trade = match &decision {
            TwoLegDecision::OpenLeg1 { market_slug, .. }
            | TwoLegDecision::OpenLeg2 { market_slug, .. } => market_slug.clone(),
        };

        match executor.execute_decision(decision).await {
            Ok(order_id) => {
                if let Some(order) = executor.order(&order_id).cloned() {
                    let side_str = match order.request.side {
                        order::OrderSide::Buy => "buy",
                        order::OrderSide::Sell => "sell",
                    };
                    let status_str = format!("{:?}", order.status).to_lowercase();

                    if let Err(err) = trade_recorder
                        .record_trade(
                            snapshot.ts,
                            &market_for_trade,
                            round_start,
                            leg_label,
                            &order.request.client_order_id,
                            side_str,
                            order.avg_fill_price,
                            order.filled_size,
                            &status_str,
                            expected_locked_profit,
                        )
                        .await
                    {
                        warn!(
                            target: "storage",
                            error = %err,
                            market = %market_for_trade,
                            "failed to record trade"
                        );
                    }
                }
            }
            Err(err) => {
                warn!(
                    target: "execution",
                    error = %err,
                    market = %market_for_trade,
                    "failed to execute decision"
                );
            }
        }
    }

    Ok(())
}

async fn handle_ws_text(
    text: &str,
    asset_to_market: &HashMap<String, (String, LegSide)>,
    books_by_market: &mut HashMap<String, MarketBook>,
    engine: &mut TwoLegEngine,
    executor: &mut OrderExecutor,
    snapshot_recorder: &SnapshotRecorder,
    trade_recorder: &TradeRecorder,
    available_capital: f64,
) -> Result<()> {
    let v: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => {
            // Ignore non-JSON messages such as raw PING/PONG echoes.
            return Ok(());
        }
    };

    let event_type = v
        .get("event_type")
        .and_then(|e| e.as_str())
        .unwrap_or_default();

    match event_type {
        "price_change" => {
            let ev: PriceChangeEvent = serde_json::from_value(v)?;
            let ts = parse_millis_timestamp(&ev.timestamp);

            for change in ev.price_changes {
                let best_bid = change
                    .best_bid
                    .as_deref()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);
                let best_ask = change
                    .best_ask
                    .as_deref()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0);

                if let Some((market_slug, side)) = asset_to_market.get(&change.asset_id) {
                    if let Some(snapshot) = update_book_and_build_snapshot(
                        books_by_market,
                        market_slug,
                        *side,
                        best_bid,
                        best_ask,
                        ts,
                    ) {
                        process_snapshot(
                            snapshot,
                            engine,
                            executor,
                            snapshot_recorder,
                            trade_recorder,
                            available_capital,
                        )
                        .await?;
                    }
                }
            }
        }
        "best_bid_ask" => {
            let ev: BestBidAskEvent = serde_json::from_value(v)?;
            let ts = parse_millis_timestamp(&ev.timestamp);
            let best_bid = ev.best_bid.parse::<f64>().unwrap_or(0.0);
            let best_ask = ev.best_ask.parse::<f64>().unwrap_or(0.0);

            if let Some((market_slug, side)) = asset_to_market.get(&ev.asset_id) {
                if let Some(snapshot) = update_book_and_build_snapshot(
                    books_by_market,
                    market_slug,
                    *side,
                    best_bid,
                    best_ask,
                    ts,
                ) {
                    process_snapshot(
                        snapshot,
                        engine,
                        executor,
                        snapshot_recorder,
                        trade_recorder,
                        available_capital,
                    )
                    .await?;
                }
            }
        }
        _ => {
            // Ignore other event types for now (book, tick_size_change, last_trade_price, etc.).
        }
    }

    Ok(())
}

/// Resolve markets to token IDs: from Gamma API for 15m (when `coin` is set), else from config.
async fn resolve_markets(
    http: &reqwest::Client,
    markets: &[MarketConfig],
) -> anyhow::Result<Vec<ResolvedMarket>> {
    let mut resolved = Vec::with_capacity(markets.len());
    for m in markets {
        if let Some(ref coin) = m.coin {
            match resolve_15m_market(http, coin, &m.slug).await {
                Ok(Some(r)) => {
                    info!(
                        target: "bot",
                        slug = %r.slug,
                        coin = %coin,
                        "resolved 15m market from Gamma API"
                    );
                    resolved.push(r);
                }
                Ok(None) => {
                    warn!(
                        target: "bot",
                        slug = %m.slug,
                        coin = %coin,
                        "Gamma API returned no market for current round; skipping"
                    );
                }
                Err(e) => {
                    warn!(
                        target: "bot",
                        slug = %m.slug,
                        coin = %coin,
                        error = %e,
                        "failed to resolve 15m market from Gamma; skipping"
                    );
                }
            }
        } else if let (Some(ref up), Some(ref down)) = (&m.up_token_id, &m.down_token_id) {
            resolved.push(ResolvedMarket {
                slug: m.slug.clone(),
                up_token_id: up.clone(),
                down_token_id: down.clone(),
            });
        } else {
            warn!(
                target: "bot",
                slug = %m.slug,
                "market has no coin (for Gamma) and no token IDs; skipping"
            );
        }
    }
    Ok(resolved)
}

/// Entrypoint used by `main.rs` to start the trading bot.
///
/// This wires together WebSocket ingestion, strategy engine, execution, storage,
/// and monitoring into a single event-driven loop. For 15m markets, set `coin` in config
/// (e.g. "btc", "eth", "sol") to resolve token IDs from the Gamma API at startup.
pub async fn run_bot(cfg: AppConfig) -> anyhow::Result<()> {
    info!(target: "bot", "run_bot starting");

    // Periodic metrics snapshots for basic observability.
    dashboard::spawn_dashboard_task(Duration::from_secs(10));

    // HTTP client for Gamma API (no auth); short timeout so startup fails fast if Gamma is unreachable.
    let http = reqwest::Client::builder()
        .user_agent("polymarket-hft-bot/0.1")
        .timeout(Duration::from_secs(15))
        .build()?;
    info!(target: "bot", "resolving markets from Gamma API / config");

    // Resolve markets to token IDs (Gamma for 15m when coin is set, else from config).
    let resolved = resolve_markets(&http, &cfg.markets.markets).await?;
    if resolved.is_empty() {
        anyhow::bail!("no markets resolved; check [markets.markets] and coin/token IDs");
    }
    info!(target: "bot", count = resolved.len(), "markets resolved");

    // Storage backends.
    info!(target: "bot", "connecting to Postgres");
    let pool = create_pg_pool(&cfg.postgres).await?;
    info!(target: "bot", "Postgres connected");
    let snapshot_recorder = SnapshotRecorder::new(pool.clone());
    let trade_recorder = TradeRecorder::new(pool.clone());

    // Strategy engine.
    let params = TwoLegParams::from(&cfg.bot);
    let mut engine = TwoLegEngine::new(params);

    // Execution engine (paper or live) using resolved markets.
    let mut executor = OrderExecutor::from_config_and_resolved(&cfg, resolved.clone())?;
    let mode = match cfg.execution.mode {
        crate::types::ExecutionMode::Paper => "paper",
        crate::types::ExecutionMode::Live => "live",
    };

    info!(
        target: "bot",
        execution_mode = mode,
        max_parallel_orders = cfg.execution.max_parallel_orders,
        markets = resolved.len(),
        "execution engine initialized"
    );

    // Map asset IDs to (market_slug, leg side) using resolved markets.
    let mut asset_to_market: HashMap<String, (String, LegSide)> = HashMap::new();
    let mut books_by_market: HashMap<String, MarketBook> = HashMap::new();

    for m in &resolved {
        asset_to_market.insert(m.up_token_id.clone(), (m.slug.clone(), LegSide::Up));
        asset_to_market.insert(m.down_token_id.clone(), (m.slug.clone(), LegSide::Down));
        books_by_market.insert(m.slug.clone(), MarketBook::default());
    }

    // WebSocket ingest.
    let ws_url = cfg.api.ws_url.clone();
    info!(
        target: "bot",
        ws_url = %ws_url,
        "connecting to Polymarket market WebSocket"
    );

    let mut conn = connect_with_retries(ws_url);
    let sender = conn.sender();
    let inbound_rx = conn.receiver();

    let assets: Vec<String> = resolved
        .iter()
        .flat_map(|m| [m.up_token_id.clone(), m.down_token_id.clone()])
        .collect();

    let sub = serde_json::json!({
        "assets_ids": assets,
        "type": "market",
        "custom_feature_enabled": true
    });

    if let Err(err) = sender.send(Message::Text(sub.to_string())) {
        return Err(anyhow::anyhow!(format!(
            "failed to send market subscription: {err}"
        )));
    }

    // Until we have a proper runtime risk/state manager, use a fixed notional
    // capital assumption for sizing.
    let available_capital = 10_000.0_f64;

    loop {
        METRICS.heartbeat();

        let msg = match inbound_rx.recv().await {
            Some(m) => m,
            None => {
                warn!(target: "bot", "websocket channel closed; exiting run loop");
                break;
            }
        };

        match msg {
            Message::Text(text) => {
                if let Err(err) = handle_ws_text(
                    &text,
                    &asset_to_market,
                    &mut books_by_market,
                    &mut engine,
                    &mut executor,
                    &snapshot_recorder,
                    &trade_recorder,
                    available_capital,
                )
                .await
                {
                    warn!(
                        target: "bot",
                        error = %err,
                        "failed to process websocket message"
                    );
                }
            }
            Message::Ping(_) | Message::Pong(_) => {
                // Heartbeats are handled in the WebSocket client; nothing to do here.
            }
            Message::Close(frame) => {
                warn!(target: "bot", ?frame, "websocket closed by server");
            }
            _ => {}
        }
    }

    Ok(())
}

