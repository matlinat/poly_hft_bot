use chrono::{DateTime, Utc};
use sqlx::{query, Pool, Postgres};

use crate::storage::models::{MarketSnapshotRow, TradeEventRow};
use crate::strategy::MarketSnapshot;

/// Records normalized market snapshots into TimescaleDB.
///
/// The expected schema (created via migrations) is:
/// ```sql
/// CREATE TABLE IF NOT EXISTS market_snapshots (
///   ts           TIMESTAMPTZ NOT NULL,
///   market_slug  TEXT        NOT NULL,
///   up_bid       DOUBLE PRECISION NOT NULL,
///   up_ask       DOUBLE PRECISION NOT NULL,
///   down_bid     DOUBLE PRECISION NOT NULL,
///   down_ask     DOUBLE PRECISION NOT NULL
/// );
/// ```
pub struct SnapshotRecorder {
    pool: Pool<Postgres>,
}

impl SnapshotRecorder {
    pub fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }

    pub async fn record_snapshot(
        &self,
        snapshot: &MarketSnapshot,
    ) -> anyhow::Result<()> {
        let row: MarketSnapshotRow = snapshot.into();

        query(
            "INSERT INTO market_snapshots \
             (ts, market_slug, up_bid, up_ask, down_bid, down_ask) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(row.ts)
        .bind(row.market_slug)
        .bind(row.up_bid)
        .bind(row.up_ask)
        .bind(row.down_bid)
        .bind(row.down_ask)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

/// Records execution-level trade events into TimescaleDB.
///
/// The expected schema (created via migrations) is:
/// ```sql
/// CREATE TABLE IF NOT EXISTS trade_events (
///   ts                    TIMESTAMPTZ NOT NULL,
///   market_slug           TEXT        NOT NULL,
///   round_start           TIMESTAMPTZ NOT NULL,
///   leg                   TEXT        NOT NULL,
///   client_order_id       TEXT        NOT NULL,
///   side                  TEXT        NOT NULL,
///   price                 DOUBLE PRECISION NOT NULL,
///   size                  DOUBLE PRECISION NOT NULL,
///   status                TEXT        NOT NULL,
///   expected_locked_profit DOUBLE PRECISION
/// );
/// ```
pub struct TradeRecorder {
    pool: Pool<Postgres>,
}

impl TradeRecorder {
    pub fn new(pool: Pool<Postgres>) -> Self {
        Self { pool }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn record_trade(
        &self,
        ts: DateTime<Utc>,
        market_slug: &str,
        round_start: DateTime<Utc>,
        leg: &str,
        client_order_id: &str,
        side: &str,
        price: f64,
        size: f64,
        status: &str,
        expected_locked_profit: Option<f64>,
    ) -> anyhow::Result<()> {
        let row = TradeEventRow {
            ts,
            market_slug: market_slug.to_string(),
            round_start,
            leg: leg.to_string(),
            client_order_id: client_order_id.to_string(),
            side: side.to_string(),
            price,
            size,
            status: status.to_string(),
            expected_locked_profit,
        };

        query(
            "INSERT INTO trade_events \
             (ts, market_slug, round_start, leg, client_order_id, side, price, size, status, expected_locked_profit) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
        )
        .bind(row.ts)
        .bind(row.market_slug)
        .bind(row.round_start)
        .bind(row.leg)
        .bind(row.client_order_id)
        .bind(row.side)
        .bind(row.price)
        .bind(row.size)
        .bind(row.status)
        .bind(row.expected_locked_profit)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

