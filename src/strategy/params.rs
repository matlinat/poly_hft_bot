use crate::types::BotConfig;

/// Parameters for the two-leg crash+hedge strategy, derived from high-level bot config.
#[derive(Clone, Debug)]
pub struct TwoLegParams {
    /// Hard cap on shares per leg.
    pub base_shares: f64,
    /// Maximum total cost of the two legs (price_up + price_down) to consider a hedge.
    pub sum_target: f64,
    /// Minimum relative move required to consider a crash within a round (e.g. 0.1 = 10%).
    pub move_pct: f64,
    /// Minutes from round start during which Leg 1 can be opened.
    pub window_min: u64,
    /// Maximum number of concurrent unhedged trades allowed across markets.
    pub max_concurrent_trades: usize,
    /// Maximum risk per trade as a percentage of available capital.
    pub risk_per_trade_pct: f64,
    /// Proportional fee rate applied on notional (e.g. 0.02 for 2%).
    pub fee_rate: f64,
    /// Minimum locked-in profit (USD) required before opening the hedge leg.
    pub min_profit_usd: f64,
}

impl From<&BotConfig> for TwoLegParams {
    fn from(cfg: &BotConfig) -> Self {
        Self {
            base_shares: cfg.shares,
            sum_target: cfg.sum_target,
            move_pct: cfg.move_pct,
            window_min: cfg.window_min,
            max_concurrent_trades: cfg.max_concurrent_trades,
            risk_per_trade_pct: cfg.risk_per_trade_pct,
            fee_rate: cfg.fee_rate,
            min_profit_usd: cfg.min_profit_usd,
        }
    }
}

