use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use crate::strategy::MarketSnapshot;

/// Row model for time-series market snapshots stored in TimescaleDB.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MarketSnapshotRow {
    pub ts: DateTime<Utc>,
    pub market_slug: String,
    pub up_bid: f64,
    pub up_ask: f64,
    pub down_bid: f64,
    pub down_ask: f64,
}

impl From<&MarketSnapshot> for MarketSnapshotRow {
    fn from(s: &MarketSnapshot) -> Self {
        Self {
            ts: s.ts,
            market_slug: s.market_slug.clone(),
            up_bid: s.up_bid,
            up_ask: s.up_ask,
            down_bid: s.down_bid,
            down_ask: s.down_ask,
        }
    }
}

impl From<MarketSnapshotRow> for MarketSnapshot {
    fn from(row: MarketSnapshotRow) -> Self {
        Self {
            ts: row.ts,
            market_slug: row.market_slug,
            up_bid: row.up_bid,
            up_ask: row.up_ask,
            down_bid: row.down_bid,
            down_ask: row.down_ask,
        }
    }
}

/// Simplified trade event model capturing execution outcomes.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TradeEventRow {
    pub ts: DateTime<Utc>,
    pub market_slug: String,
    pub round_start: DateTime<Utc>,
    pub leg: String,
    pub client_order_id: String,
    pub side: String,
    pub price: f64,
    pub size: f64,
    pub status: String,
    pub expected_locked_profit: Option<f64>,
}

