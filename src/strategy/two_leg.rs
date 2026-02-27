use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    strategy::{params::TwoLegParams, MarketSnapshot},
    utils::{
        math::{locked_profit, position_size_kelly},
        time::{round_end, round_start, seconds_remaining, within_leg1_window},
    },
};

/// Side of the binary market a given leg is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LegSide {
    Up,
    Down,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LegPosition {
    pub side: LegSide,
    pub entry_price: f64,
    pub shares: f64,
}

/// Public summary of per-round state for monitoring/backtesting.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum TwoLegState {
    Idle,
    Leg1Open {
        round_start: DateTime<Utc>,
        leg1: LegPosition,
        baseline_mid: f64,
    },
    Hedged {
        round_start: DateTime<Utc>,
        leg1: LegPosition,
        leg2_price: f64,
        locked_profit: f64,
    },
}

/// Decision emitted by the strategy engine to be consumed by the execution layer.
#[derive(Clone, Debug)]
pub enum TwoLegDecision {
    /// Open the first (directional) leg of the position.
    OpenLeg1 {
        market_slug: String,
        round_start: DateTime<Utc>,
        side: LegSide,
        shares: f64,
        limit_price: f64,
    },
    /// Open the hedge leg that locks in profit for the round.
    OpenLeg2 {
        market_slug: String,
        round_start: DateTime<Utc>,
        side: LegSide,
        shares: f64,
        limit_price: f64,
        expected_locked_profit: f64,
    },
}

#[derive(Hash, Eq, PartialEq, Clone, Debug, Serialize, Deserialize)]
struct RoundKey {
    market_slug: String,
    round_start: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct RoundInternal {
    round_start: DateTime<Utc>,
    baseline_mid: f64,
    leg1: Option<LegPosition>,
    hedged: bool,
}

impl RoundInternal {
    fn new(round_start: DateTime<Utc>, baseline_mid: f64) -> Self {
        Self {
            round_start,
            baseline_mid,
            leg1: None,
            hedged: false,
        }
    }
}

/// Two-leg crash+hedge strategy engine maintaining per-market, per-round state.
#[derive(Debug)]
pub struct TwoLegEngine {
    params: TwoLegParams,
    rounds: HashMap<RoundKey, RoundInternal>,
}

impl TwoLegEngine {
    pub fn new(params: TwoLegParams) -> Self {
        Self {
            params,
            rounds: HashMap::new(),
        }
    }

    /// Process a new market snapshot and return any trading decisions.
    ///
    /// `available_capital` is the current account capital in quote currency and is
    /// used for Kelly sizing and risk caps.
    pub fn on_snapshot(
        &mut self,
        snapshot: MarketSnapshot,
        available_capital: f64,
    ) -> Vec<TwoLegDecision> {
        let now = snapshot.ts;

        // Drop stale rounds that have fully expired.
        self.drop_expired_rounds(now);

        let current_round_start = round_start(now);
        let key = RoundKey {
            market_slug: snapshot.market_slug.clone(),
            round_start: current_round_start,
        };

        let active_unhedged = self.active_unhedged_trades();

        let baseline_mid = snapshot.mid_up();
        let round = self
            .rounds
            .entry(key.clone())
            .or_insert_with(|| RoundInternal::new(current_round_start, baseline_mid));

        let mut decisions = Vec::new();

        match (&round.leg1, round.hedged) {
            (None, false) => {
                // Potential Leg 1 entry.
                if let Some(decision) =
                    maybe_open_leg1(&self.params, active_unhedged, round, &snapshot, available_capital)
                {
                    decisions.push(decision);
                }
            }
            (Some(leg1), false) => {
                // Leg 1 is open, consider hedge.
                if let Some(decision) = maybe_open_leg2(&self.params, leg1, round, &snapshot) {
                    round.hedged = true;
                    decisions.push(decision);
                }
            }
            _ => {
                // Already hedged or otherwise idle; nothing to do.
            }
        }

        decisions
    }

    /// Snapshot of state for a given market/round, if it exists.
    pub fn state_for(&self, market_slug: &str, round_start_ts: DateTime<Utc>) -> Option<TwoLegState> {
        let key = RoundKey {
            market_slug: market_slug.to_string(),
            round_start: round_start_ts,
        };
        self.rounds.get(&key).map(|r| {
            match (&r.leg1, r.hedged) {
                (None, _) => TwoLegState::Idle,
                (Some(leg1), false) => TwoLegState::Leg1Open {
                    round_start: r.round_start,
                    leg1: leg1.clone(),
                    baseline_mid: r.baseline_mid,
                },
                (Some(leg1), true) => TwoLegState::Hedged {
                    round_start: r.round_start,
                    leg1: leg1.clone(),
                    leg2_price: 0.0,
                    locked_profit: 0.0,
                },
            }
        })
    }

    /// Number of active (unhedged) trades across markets.
    pub fn active_unhedged_trades(&self) -> usize {
        self.rounds
            .values()
            .filter(|r| r.leg1.is_some() && !r.hedged)
            .count()
    }

    fn drop_expired_rounds(&mut self, now: DateTime<Utc>) {
        self.rounds
            .retain(|_, r| round_end(r.round_start) >= now);
    }

}

fn maybe_open_leg1(
    params: &TwoLegParams,
    active_unhedged_trades: usize,
    round: &mut RoundInternal,
    snapshot: &MarketSnapshot,
    available_capital: f64,
) -> Option<TwoLegDecision> {
        if available_capital <= 0.0 {
            return None;
        }

        // Respect global cap on concurrent unhedged trades.
        if active_unhedged_trades >= params.max_concurrent_trades {
            return None;
        }

        // Only trade early in the round.
        if !within_leg1_window(snapshot.ts, params.window_min) {
            return None;
        }

        let baseline_mid = if round.baseline_mid > 0.0 {
            round.baseline_mid
        } else {
            snapshot.mid_up()
        };

        let current_mid = snapshot.mid_up();
        if baseline_mid <= 0.0 || current_mid <= 0.0 {
            return None;
        }

        // Crash detection: require price drop from baseline of at least move_pct.
        let drop = (baseline_mid - current_mid) / baseline_mid;
        if drop < self.params.move_pct {
            return None;
        }

        // Estimate win probability from crash severity and current price.
        let mut p = (1.0 - current_mid) * (1.0 + params.move_pct);
        if !p.is_finite() {
            return None;
        }
        p = p.clamp(0.01, 0.99);

        let price_for_sizing = snapshot.up_ask.max(1e-6);
        let kelly_shares = position_size_kelly(
            available_capital,
            price_for_sizing,
            p,
            params.fee_rate,
            params.risk_per_trade_pct,
        );

        let mut shares = kelly_shares.min(params.base_shares);
        if !shares.is_finite() || shares <= 0.0 {
            return None;
        }

        // Ensure we do not request negative or absurdly large size.
        shares = shares.clamp(0.0, params.base_shares);

        let leg1 = LegPosition {
            side: LegSide::Up,
            entry_price: snapshot.up_ask,
            shares,
        };
        round.leg1 = Some(leg1.clone());
        round.baseline_mid = baseline_mid;

        Some(TwoLegDecision::OpenLeg1 {
            market_slug: snapshot.market_slug.clone(),
            round_start: round.round_start,
            side: leg1.side,
            shares: leg1.shares,
            limit_price: snapshot.up_ask,
        })
    }

fn maybe_open_leg2(
    params: &TwoLegParams,
    leg1: &LegPosition,
    round: &RoundInternal,
    snapshot: &MarketSnapshot,
) -> Option<TwoLegDecision> {
        // Avoid hedging in the last seconds of the round.
        if seconds_remaining(snapshot.ts) <= 3 {
            return None;
        }

        let (hedge_side, hedge_price) = match leg1.side {
            LegSide::Up => (LegSide::Down, snapshot.down_ask),
            LegSide::Down => (LegSide::Up, snapshot.up_ask),
        };

        if hedge_price <= 0.0 {
            return None;
        }

        let expected_profit =
            locked_profit(leg1.entry_price, hedge_price, leg1.shares, params.fee_rate);

        // Enforce both profit and total-cost filters.
        let total_cost = leg1.entry_price + hedge_price;
        if expected_profit < params.min_profit_usd || total_cost > params.sum_target {
            return None;
        }

        Some(TwoLegDecision::OpenLeg2 {
            market_slug: snapshot.market_slug.clone(),
            round_start: round.round_start,
            side: hedge_side,
            shares: leg1.shares,
            limit_price: hedge_price,
            expected_locked_profit: expected_profit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn ts(s: &str) -> DateTime<Utc> {
        Utc.datetime_from_str(s, "%Y-%m-%dT%H:%M:%S").unwrap()
    }

    fn snapshot(price_up: f64, price_down: f64, ts_str: &str) -> MarketSnapshot {
        MarketSnapshot {
            ts: ts(ts_str),
            market_slug: "BTC_15m".to_string(),
            up_bid: price_up * 0.99,
            up_ask: price_up * 1.01,
            down_bid: price_down * 0.99,
            down_ask: price_down * 1.01,
        }
    }

    fn default_params() -> TwoLegParams {
        TwoLegParams {
            base_shares: 10.0,
            sum_target: 0.95,
            move_pct: 0.1,
            window_min: 3,
            max_concurrent_trades: 1,
            risk_per_trade_pct: 2.0,
            fee_rate: 0.02,
            min_profit_usd: 0.10,
        }
    }

    #[test]
    fn opens_leg1_on_crash() {
        let mut engine = TwoLegEngine::new(default_params());
        // First snapshot sets baseline.
        let s1 = snapshot(0.6, 0.4, "2024-01-01T12:00:10");
        let decisions = engine.on_snapshot(s1, 1_000.0);
        assert!(decisions.is_empty());

        // Big crash within early window.
        let s2 = snapshot(0.45, 0.55, "2024-01-01T12:01:10");
        let decisions = engine.on_snapshot(s2, 1_000.0);
        assert!(
            decisions.iter().any(|d| matches!(d, TwoLegDecision::OpenLeg1 { .. })),
            "expected Leg1 open decision"
        );
    }

    #[test]
    fn opens_leg2_when_profit_threshold_met() {
        let mut params = default_params();
        params.min_profit_usd = 0.0; // make it easy to trigger.
        let mut engine = TwoLegEngine::new(params);

        // Baseline.
        let s1 = snapshot(0.6, 0.4, "2024-01-01T12:00:10");
        engine.on_snapshot(s1, 10_000.0);

        // Crash → open Leg1.
        let s2 = snapshot(0.4, 0.6, "2024-01-01T12:01:00");
        let _ = engine.on_snapshot(s2, 10_000.0);

        // Prices move such that total sum is low enough to lock profit.
        let s3 = snapshot(0.35, 0.35, "2024-01-01T12:05:00");
        let decisions = engine.on_snapshot(s3, 10_000.0);
        assert!(
            decisions.iter().any(|d| matches!(d, TwoLegDecision::OpenLeg2 { .. })),
            "expected Leg2 hedge decision"
        );
    }
}

