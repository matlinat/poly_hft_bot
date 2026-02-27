# Polymarket HFT Bot (Educational/Research Only)

This project is an educational, research-focused high-frequency trading (HFT) bot for Polymarket 15-minute crypto markets (BTC/ETH/SOL).  
It implements a two-leg crash/hedge arbitrage strategy, with paper-trading as the default for safety.

**Use at your own risk. This is not financial advice. Live trading against real markets is risky and may result in total loss of funds.**

## Features

- Event-driven architecture: WebSocket ingestor → strategy engine → execution → storage/monitoring
- Two-leg arbitrage for 15-minute UP/DOWN prediction markets
- Multi-market scanning (BTC, ETH, SOL 15m)
- Profit threshold filter and Kelly-based position sizing (capped by risk limits)
- Paper trading by default; live execution only when explicitly enabled
- TimescaleDB (PostgreSQL) snapshot recorder and trade storage
- Redis-backed state management
- Backtester that replays snapshots and reports ROI
- JSON-structured logging and terminal dashboard
- Dockerfile and docker-compose for deployment

## Quick Start

### 1. Prerequisites

- Rust toolchain (stable)
- Docker and docker-compose (for infra)
- Access to a PostgreSQL instance (ideally TimescaleDB-enabled)
- Access to a Redis instance

### 2. Environment Variables

Create a `.env` file (or export these variables):

```bash
POLYMARKET_API_KEY=your_api_key
POLYMARKET_API_SECRET=your_api_secret
WALLET_PRIVATE_KEY=0x...
GNOSIS_SAFE_ADDRESS=0x... # optional
```

### 3. Run in Paper Mode (Default)

```bash
cargo run -- --config config/default.toml run
```

Execution mode defaults to `paper`. You can override:

```bash
cargo run -- --config config/default.toml --mode live run
```

**Warning:** Live mode will attempt to send real orders to Polymarket. Do not enable live mode unless you fully understand the risks and have verified behavior in paper mode.

### 4. Run Backtests

```bash
cargo run -- backtest --config config/backtest.toml
```

The backtester replays stored snapshots and evaluates the two-leg strategy under configurable parameters.

## Deployment Overview

- Use the provided `Dockerfile` and `docker-compose.yml` to spin up:
  - The bot container
  - Postgres/TimescaleDB
  - Redis
- Place the bot on a VPS geographically close to Polymarket’s infrastructure for lower latency.

## Disclaimer

This project is provided **as-is** without any guarantees.  
Markets are adversarial, APIs may change, infrastructure may fail, and historical performance does not guarantee future results.

