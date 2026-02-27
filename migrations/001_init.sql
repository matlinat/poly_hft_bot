-- Timescale-Extension aktivieren (idempotent)
CREATE EXTENSION IF NOT EXISTS timescaledb;

-- Normalisierte Orderbuch-Snapshots
CREATE TABLE IF NOT EXISTS market_snapshots (
  ts           TIMESTAMPTZ       NOT NULL,
  market_slug  TEXT              NOT NULL,
  up_bid       DOUBLE PRECISION  NOT NULL,
  up_ask       DOUBLE PRECISION  NOT NULL,
  down_bid     DOUBLE PRECISION  NOT NULL,
  down_ask     DOUBLE PRECISION  NOT NULL
);

-- Hypertable anlegen (idempotent)
SELECT create_hypertable('market_snapshots', 'ts', if_not_exists => TRUE);

-- Trade-Events (Execution-Layer)
CREATE TABLE IF NOT EXISTS trade_events (
  ts                     TIMESTAMPTZ       NOT NULL,
  market_slug            TEXT              NOT NULL,
  round_start            TIMESTAMPTZ       NOT NULL,
  leg                    TEXT              NOT NULL,
  client_order_id        TEXT              NOT NULL,
  side                   TEXT              NOT NULL,
  price                  DOUBLE PRECISION  NOT NULL,
  size                   DOUBLE PRECISION  NOT NULL,
  status                 TEXT              NOT NULL,
  expected_locked_profit DOUBLE PRECISION
);

-- Hypertable anlegen (idempotent)
SELECT create_hypertable('trade_events', 'ts', if_not_exists => TRUE);