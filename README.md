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

## Architecture Overview

- **WebSocket ingest**: subscribes to Polymarket CLOB feeds and normalizes book updates into `MarketSnapshot` structs.
- **Strategy engine**: the two-leg crash/hedge strategy holds per-market, per-round state and emits `TwoLegDecision` actions.
- **Execution layer**: converts decisions into CLOB orders (paper/live) and tracks lifecycle, cancellations, and failures.
- **Storage**: persists normalized snapshots and trade events to TimescaleDB; runtime state and locks may live in Redis.
- **Monitoring**: JSON logs (via `tracing`), metrics hooks, and a terminal dashboard for at-a-glance status.

Paper mode uses the exact same code paths as live mode, but simulates fills instead of sending signed orders.

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
GNOSIS_SAFE_ADDRESS=0x... # optional, only if routing via a Safe
```

### 3. Configuration

The main config files live under `config/`:

- `config/default.toml`: local development / paper trading defaults (localhost Postgres/Redis).
- `config/production.toml`: container-friendly production defaults (Docker `db` and `redis` hosts).
- `config/backtest.toml`: configuration for historical snapshot replay backtests.

Key sections:

- **[redis]** / **[postgres]**: connection URLs.
- **[api]**: Polymarket CLOB REST/WebSocket endpoints and API credentials (interpolated from env vars).
- **[bot]**: strategy parameters such as `move_pct`, `sum_target`, and `min_profit_usd`.
- **[markets]**: which Polymarket markets to trade or backtest.
- **[execution]**: mode (`paper` or `live`) and max in-flight orders.

### 4. Run in Paper Mode (Default)

```bash
cargo run -- --config config/default.toml run
```

Execution mode defaults to `paper`. You can override:

```bash
cargo run -- --config config/default.toml --mode live run
```

**Warning:** Live mode will attempt to send real orders to Polymarket. Do not enable live mode unless you fully understand the risks and have verified behavior in paper mode.

### 5. Run Backtests

```bash
cargo run -- backtest --config config/backtest.toml
```

The backtester:

- Loads snapshot ranges from `config/backtest.toml`.
- Fetches normalized snapshots from the `market_snapshots` table in TimescaleDB.
- Replays them deterministically through the two-leg strategy engine.
- Logs a JSON summary event on the `backtest` log target with:
  - initial and final capital
  - total profit
  - ROI in percent
  - number of completed (hedged) trades

Given the same snapshot set and config, the backtest is fully deterministic.

## Deployment Overview

- Use the provided `Dockerfile` and `docker-compose.yml` to spin up:
  - the bot container
  - Postgres/TimescaleDB
  - Redis

### 1. Bring up infrastructure

```bash
docker-compose up -d db redis
```

Run your database migrations to create the `market_snapshots` and `trade_events` tables (see `storage::recorder` docs for schema examples).

### 2. Build and run the bot container

```bash
docker-compose up --build bot
```

This will:

- build the Rust project in a multi-stage Docker image
- run the bot with `config/production.toml` inside the container
- default to `paper` execution mode

To run a backtest inside Docker, you can override the command:

```bash
docker-compose run --rm bot \
  polymarket-hft-bot --config config/production.toml backtest --config config/backtest.toml
```

## Risk and Safety Disclaimer

This project is provided **as-is** without any guarantees.  
Markets are adversarial, APIs may change, infrastructure may fail, and historical performance does **not** guarantee future results.

Before enabling live trading you should:

- thoroughly backtest your parameter choices over multiple, disjoint periods
- run the bot in paper mode for an extended period and study behavior under stress
- verify all credentials and wallet keys are stored and loaded securely
- understand that Polymarket, network, or exchange changes can break assumptions at any time

You are solely responsible for any losses incurred by running this code.


