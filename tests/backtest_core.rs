use polymarket_hft_bot::backtest::core::run_backtest_on_snapshots;
use polymarket_hft_bot::strategy::MarketSnapshot;
use polymarket_hft_bot::types::BotConfig;

use chrono::{DateTime, TimeZone, Utc};

fn ts(s: &str) -> DateTime<Utc> {
    Utc.datetime_from_str(s, "%Y-%m-%dT%H:%M:%S").unwrap()
}

fn snapshot(price_up: f64, price_down: f64, ts_str: &str, slug: &str) -> MarketSnapshot {
    MarketSnapshot {
        ts: ts(ts_str),
        market_slug: slug.to_string(),
        up_bid: price_up * 0.99,
        up_ask: price_up * 1.01,
        down_bid: price_down * 0.99,
        down_ask: price_down * 1.01,
    }
}

fn bot_cfg() -> BotConfig {
    BotConfig {
        shares: 10.0,
        sum_target: 0.95,
        move_pct: 0.1,
        window_min: 3,
        max_concurrent_trades: 2,
        risk_per_trade_pct: 2.0,
        fee_rate: 0.02,
        min_profit_usd: 0.0,
    }
}

#[test]
fn backtest_handles_multiple_markets() {
    let cfg = bot_cfg();
    let snaps = vec![
        snapshot(0.6, 0.4, "2024-01-01T12:00:10", "BTC-USD-15MIN"),
        snapshot(0.45, 0.55, "2024-01-01T12:01:10", "BTC-USD-15MIN"),
        snapshot(0.35, 0.35, "2024-01-01T12:05:00", "BTC-USD-15MIN"),
        snapshot(0.6, 0.4, "2024-01-01T12:00:20", "ETH-USD-15MIN"),
        snapshot(0.45, 0.55, "2024-01-01T12:01:20", "ETH-USD-15MIN"),
        snapshot(0.35, 0.35, "2024-01-01T12:05:20", "ETH-USD-15MIN"),
    ];

    let result = run_backtest_on_snapshots(&snaps, &cfg, 10_000.0, None);

    assert!(result.trades.len() >= 1);
}

