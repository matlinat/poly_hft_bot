use sqlx::postgres::PgPoolOptions;
use sqlx::{Pool, Postgres};

use crate::types::{PostgresConfig, RedisConfig};

pub mod models;
pub mod recorder;
pub mod state;

pub type PgPool = Pool<Postgres>;

/// Create a PostgreSQL/TimescaleDB connection pool using the provided config.
///
/// This uses a small, conservative pool size suitable for a single bot
/// instance. Connection establishment is performed eagerly so misconfiguration
/// is surfaced early at startup.
pub async fn create_pg_pool(cfg: &PostgresConfig) -> anyhow::Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(std::time::Duration::from_secs(15))
        .connect(&cfg.url)
        .await?;
    Ok(pool)
}

/// Create a Redis client using the provided config.
///
/// The returned client can be turned into an async connection manager by
/// downstream components when needed.
pub fn create_redis_client(cfg: &RedisConfig) -> anyhow::Result<redis::Client> {
    let client = redis::Client::open(cfg.url.as_str())?;
    Ok(client)
}

