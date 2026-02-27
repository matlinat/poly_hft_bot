use std::collections::HashMap;

use chrono::{DateTime, Utc};

use crate::{
    strategy::{MarketSnapshot, TwoLegDecision, TwoLegEngine, TwoLegParams},
    types::BotConfig,
    utils::math::locked_profit,
};

#[derive(Clone, Debug)]
pub struct BacktestTrade {
    pub market_slug: String,
    pub round_start: DateTime<Utc>,
    pub leg1_price: f64,
    pub leg2_price: f64,
    pub shares: f64,
    pub locked_profit: f64,
}

#[derive(Clone, Debug)]
pub struct BacktestResult {
    pub initial_capital: f64,
    pub final_capital: f64,
    pub total_profit: f64,
    pub trades: Vec<BacktestTrade>,
}

#[derive(Hash, Eq, PartialEq, Clone, Debug)]
struct PositionKey {
    market_slug: String,
    round_start: DateTime<Utc>,
}

#[derive(Clone, Debug)]
struct OpenPosition {
    leg1_price: f64,
    shares: f64,
}

/// Deterministically replay a sequence of normalized snapshots through the strategy engine.
///
/// The caller is responsible for providing snapshots in time-ascending order. Given the same
/// snapshots and config, the result is fully deterministic.
pub fn run_backtest_on_snapshots(
    snapshots: &[MarketSnapshot],
    bot_cfg: &BotConfig,
    initial_capital: f64,
    max_steps: Option<usize>,
) -> BacktestResult {
    let params = TwoLegParams::from(bot_cfg);
    let mut engine = TwoLegEngine::new(params);

    let mut capital = initial_capital;
    let mut trades = Vec::new();
    let mut open_positions: HashMap<PositionKey, OpenPosition> = HashMap::new();

    let mut processed = 0usize;

    for snapshot in snapshots.iter().cloned() {
        if let Some(limit) = max_steps {
            if processed >= limit {
                break;
            }
        }
        processed += 1;

        let decisions = engine.on_snapshot(snapshot.clone(), capital);
        for decision in decisions {
            match decision {
                TwoLegDecision::OpenLeg1 {
                    market_slug,
                    round_start,
                    shares,
                    limit_price,
                    ..
                } => {
                    let key = PositionKey {
                        market_slug,
                        round_start,
                    };
                    // Only record if there isn't already an open position for this round.
                    open_positions
                        .entry(key)
                        .or_insert(OpenPosition { leg1_price: limit_price, shares });
                }
                TwoLegDecision::OpenLeg2 {
                    market_slug,
                    round_start,
                    shares,
                    limit_price,
                    expected_locked_profit,
                    ..
                } => {
                    let key = PositionKey {
                        market_slug: market_slug.clone(),
                        round_start,
                    };
                    if let Some(pos) = open_positions.remove(&key) {
                        let profit = if expected_locked_profit.is_finite() {
                            expected_locked_profit
                        } else {
                            locked_profit(pos.leg1_price, limit_price, pos.shares, bot_cfg.fee_rate)
                        };
                        capital += profit;
                        trades.push(BacktestTrade {
                            market_slug,
                            round_start,
                            leg1_price: pos.leg1_price,
                            leg2_price: limit_price,
                            shares,
                            locked_profit: profit,
                        });
                    }
                }
            }
        }
    }

    let total_profit = capital - initial_capital;

    BacktestResult {
        initial_capital,
        final_capital: capital,
        total_profit,
        trades,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    use crate::strategy::MarketSnapshot;

    fn ts(s: &str) -> DateTime<Utc> {
        Utc.datetime_from_str(s, "%Y-%m-%dT%H:%M:%S").unwrap()
    }

    fn snapshot(price_up: f64, price_down: f64, ts_str: &str) -> MarketSnapshot {
        MarketSnapshot {
            ts: ts(ts_str),
            market_slug: "BTC-USD-15MIN".to_string(),
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
            max_concurrent_trades: 1,
            risk_per_trade_pct: 2.0,
            fee_rate: 0.02,
            min_profit_usd: 0.0,
        }
    }

    #[test]
    fn deterministic_results_for_same_input() {
        let cfg = bot_cfg();
        let snaps = vec![
            snapshot(0.6, 0.4, "2024-01-01T12:00:10"),
            snapshot(0.45, 0.55, "2024-01-01T12:01:10"),
            snapshot(0.35, 0.35, "2024-01-01T12:05:00"),
        ];

        let r1 = run_backtest_on_snapshots(&snaps, &cfg, 10_000.0, None);
        let r2 = run_backtest_on_snapshots(&snaps, &cfg, 10_000.0, None);

        assert!((r1.total_profit - r2.total_profit).abs() < 1e-9);
        assert_eq!(r1.trades.len(), r2.trades.len());
    }

    #[test]
    fn records_profitable_trade_when_hedged() {
        let cfg = bot_cfg();
        let snaps = vec![
            snapshot(0.6, 0.4, "2024-01-01T12:00:10"),
            snapshot(0.4, 0.6, "2024-01-01T12:01:00"),
            snapshot(0.35, 0.35, "2024-01-01T12:05:00"),
        ];

        let result = run_backtest_on_snapshots(&snaps, &cfg, 10_000.0, None);
        assert!(!result.trades.is_empty());
        assert!(result.total_profit > 0.0);
    }
}

