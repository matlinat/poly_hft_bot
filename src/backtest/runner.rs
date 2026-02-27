use chrono::Utc;
use serde::Serialize;
use tracing::info;

use crate::{
    backtest::config::BacktestConfig,
    backtest::core::{run_backtest_on_snapshots, BacktestResult},
    storage::{create_pg_pool, models::MarketSnapshotRow},
    strategy::MarketSnapshot,
};

/// Execute a backtest by loading snapshots from TimescaleDB and replaying them through the
/// two-leg strategy engine.
pub async fn run_backtest(cfg: BacktestConfig) -> anyhow::Result<()> {
    let pool = create_pg_pool(&cfg.postgres).await?;

    let mut rows_all: Vec<MarketSnapshotRow> = Vec::new();

    for m in &cfg.markets {
        let mut rows: Vec<MarketSnapshotRow> = sqlx::query_as(
            "SELECT ts, market_slug, up_bid, up_ask, down_bid, down_ask \
             FROM market_snapshots \
             WHERE market_slug = $1 AND ts >= $2 AND ts <= $3 \
             ORDER BY ts ASC",
        )
        .bind(&m.slug)
        .bind(m.start)
        .bind(m.end)
        .fetch_all(&pool)
        .await?;

        rows_all.append(&mut rows);
    }

    // Sort globally by timestamp (and slug as a tiebreaker) to ensure deterministic ordering.
    rows_all.sort_by(|a, b| {
        let ord_ts = a.ts.cmp(&b.ts);
        if ord_ts != std::cmp::Ordering::Equal {
            return ord_ts;
        }
        a.market_slug.cmp(&b.market_slug)
    });

    let snapshots: Vec<MarketSnapshot> = rows_all
        .into_iter()
        .map(MarketSnapshot::from)
        .collect();

    let result = run_backtest_on_snapshots(
        &snapshots,
        &cfg.bot,
        cfg.initial_capital,
        None,
    );

    log_summary(&result);

    Ok(())
}

#[derive(Serialize)]
struct BacktestSummary<'a> {
    event: &'a str,
    started_at: String,
    finished_at: String,
    initial_capital: f64,
    final_capital: f64,
    total_profit: f64,
    roi_pct: f64,
    trades: usize,
}

fn log_summary(result: &BacktestResult) {
    let now = Utc::now();
    let summary = BacktestSummary {
        event: "backtest_summary",
        started_at: now.to_rfc3339(),
        finished_at: now.to_rfc3339(),
        initial_capital: result.initial_capital,
        final_capital: result.final_capital,
        total_profit: result.total_profit,
        roi_pct: if result.initial_capital != 0.0 {
            (result.final_capital / result.initial_capital - 1.0) * 100.0
        } else {
            0.0
        },
        trades: result.trades.len(),
    };

    let payload =
        serde_json::to_string(&summary).unwrap_or_else(|_| "{\"event\":\"backtest_summary_error\"}".to_string());
    info!(target: "backtest", "{payload}");
}

