use serde::Serialize;
use tracing::info;

use crate::types::AppConfig;

#[derive(Serialize)]
struct StartupLog<'a> {
    event: &'a str,
    execution_mode: &'a str,
    markets: Vec<&'a str>,
}

pub fn log_startup(cfg: &AppConfig) {
    let mode = match cfg.execution.mode {
        crate::types::ExecutionMode::Paper => "paper",
        crate::types::ExecutionMode::Live => "live",
    };
    let markets = cfg.markets.markets.iter().map(|m| m.slug.as_str()).collect();
    let payload = StartupLog {
        event: "startup",
        execution_mode: mode,
        markets,
    };
    info!(target: "bot", startup = serde_json::to_string(&payload).unwrap_or_default().as_str());
}

