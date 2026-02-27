use clap::{Parser, Subcommand};
use dotenv::dotenv;
use tracing_subscriber::EnvFilter;

use polymarket_hft_bot::{
    backtest,
    execution,
    monitoring,
    types::{AppConfig, ExecutionMode},
};

#[derive(Parser, Debug)]
#[command(name = "polymarket-hft-bot")]
#[command(about = "Polymarket 15m BTC/crypto two-leg arbitrage bot", long_about = None)]
struct Cli {
    /// Path to configuration file
    #[arg(short, long, default_value = "config/default.toml")]
    config: String,

    /// Override execution mode (paper/live)
    #[arg(long)]
    mode: Option<ExecutionMode>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run the trading bot
    Run {},
    /// Run backtests using recorded snapshots
    Backtest {
        /// Optional path to backtest configuration
        #[arg(short, long)]
        config: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .json()
        .init();

    let cli = Cli::parse();

    let mut settings = AppConfig::from_file(&cli.config)?;
    if let Some(mode) = cli.mode {
        settings.execution.mode = mode;
    }

    match cli.command.unwrap_or(Commands::Run {}) {
        Commands::Run {} => {
            monitoring::logger::log_startup(&settings);
            execution::run_bot(settings).await?;
        }
        Commands::Backtest { config } => {
            let backtest_config_path = config.unwrap_or_else(|| "config/backtest.toml".to_string());
            let backtest_cfg = backtest::config::BacktestConfig::from_file(&backtest_config_path)?;
            backtest::runner::run_backtest(backtest_cfg).await?;
        }
    }

    Ok(())
}

