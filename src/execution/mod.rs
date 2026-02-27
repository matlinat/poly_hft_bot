mod executor;
pub mod order;

use tracing::info;

use crate::types::AppConfig;

pub use executor::{ExecutionError, ExecutionResult, OrderExecutor};

/// Entrypoint used by `main.rs` to start the trading bot.
///
/// For now this function only initializes the execution engine in the configured
/// mode (paper/live). Wiring to the full event pipeline (WebSocket ingestion,
/// strategy engine, storage, etc.) will be added incrementally.
pub async fn run_bot(cfg: AppConfig) -> anyhow::Result<()> {
    let executor = OrderExecutor::from_config(&cfg)?;
    let mode = match cfg.execution.mode {
        crate::types::ExecutionMode::Paper => "paper",
        crate::types::ExecutionMode::Live => "live",
    };

    info!(
        target: "bot",
        execution_mode = mode,
        max_parallel_orders = cfg.execution.max_parallel_orders,
        markets = cfg.markets.markets.len(),
        "execution engine initialized"
    );

    // In this iteration we only stand up the execution layer; higher-level
    // orchestration will call into `OrderExecutor` as strategy decisions arrive.
    drop(executor);
    Ok(())
}

