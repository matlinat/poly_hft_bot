use std::fs;

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::types::{BotConfig, PostgresConfig};

/// Configuration for a single market backtest range.
#[derive(Clone, Debug, Deserialize)]
pub struct MarketBacktestRange {
    pub slug: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

/// Top-level backtest configuration loaded from TOML.
#[derive(Clone, Debug, Deserialize)]
pub struct BacktestConfig {
    pub postgres: PostgresConfig,
    pub bot: BotConfig,
    /// Initial virtual account capital used for sizing and ROI.
    pub initial_capital: f64,
    /// Per-market time ranges to replay.
    pub markets: Vec<MarketBacktestRange>,
}

impl BacktestConfig {
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read backtest config file at {path}"))?;
        let cfg: Self = toml::from_str(&contents)
            .with_context(|| format!("failed to deserialize backtest TOML at {path}"))?;
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_backtest_config_toml() {
        let toml = r#"
            [postgres]
            url = "postgres://user:pass@localhost:5432/db"

            [bot]
            shares = 10.0
            sum_target = 0.95
            move_pct = 0.1
            window_min = 2
            max_concurrent_trades = 1
            risk_per_trade_pct = 2.0
            fee_rate = 0.02
            min_profit_usd = 0.5

            initial_capital = 10000.0

            [[markets]]
            slug = "BTC-USD-15MIN"
            start = "2024-01-01T00:00:00Z"
            end = "2024-01-02T00:00:00Z"
        "#;

        let cfg: BacktestConfig = toml::from_str(toml).expect("failed to parse backtest config");
        assert_eq!(cfg.postgres.url, "postgres://user:pass@localhost:5432/db");
        assert!((cfg.initial_capital - 10_000.0).abs() < f64::EPSILON);
        assert_eq!(cfg.markets.len(), 1);
        assert_eq!(cfg.markets[0].slug, "BTC-USD-15MIN");
    }
}

