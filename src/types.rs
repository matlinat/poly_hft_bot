use std::fs;

use anyhow::Context;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionMode {
    Paper,
    Live,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RedisConfig {
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PostgresConfig {
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ApiConfig {
    pub base_url: String,
    pub ws_url: String,
    pub api_key: String,
    pub api_secret: String,
    pub api_passphrase: String,
    pub wallet_private_key: String,
    pub gnosis_safe_address: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BotConfig {
    pub shares: f64,
    pub sum_target: f64,
    pub move_pct: f64,
    pub window_min: u64,
    pub max_concurrent_trades: usize,
    pub risk_per_trade_pct: f64,
    pub fee_rate: f64,
    pub min_profit_usd: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketConfig {
    pub slug: String,
    /// Coin for 15m dynamic markets (e.g. "btc", "eth", "sol"). If set, token IDs are resolved from Gamma API at startup.
    #[serde(default)]
    pub coin: Option<String>,
    #[serde(default)]
    pub up_token_id: Option<String>,
    #[serde(default)]
    pub down_token_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MarketsConfig {
    pub markets: Vec<MarketConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionConfig {
    pub mode: ExecutionMode,
    pub max_parallel_orders: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppConfig {
    pub redis: RedisConfig,
    pub postgres: PostgresConfig,
    pub api: ApiConfig,
    pub bot: BotConfig,
    pub markets: MarketsConfig,
    pub execution: ExecutionConfig,
}

impl AppConfig {
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let contents = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file at {path}"))?;
        let cfg: Self = toml::from_str(&contents)
            .with_context(|| format!("failed to deserialize TOML config at {path}"))?;
        Ok(cfg)
    }
}

