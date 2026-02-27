use chrono::{DateTime, Utc};
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde_json;

use crate::strategy::TwoLegState;
use crate::types::RedisConfig;

/// Manages runtime strategy state persisted in Redis.
///
/// State is keyed by `two_leg:{market_slug}:{round_start_iso}` where
/// `round_start_iso` is the RFC3339 representation of the round start time.
pub struct RedisStateManager {
    conn: ConnectionManager,
}

impl RedisStateManager {
    pub async fn new(cfg: &RedisConfig) -> anyhow::Result<Self> {
        let client = redis::Client::open(cfg.url.as_str())?;
        let conn = ConnectionManager::new(client).await?;
        Ok(Self { conn })
    }

    fn key(market_slug: &str, round_start: DateTime<Utc>) -> String {
        format!("two_leg:{}:{}", market_slug, round_start.to_rfc3339())
    }

    pub async fn save_round_state(
        &mut self,
        market_slug: &str,
        round_start: DateTime<Utc>,
        state: &TwoLegState,
    ) -> anyhow::Result<()> {
        let key = Self::key(market_slug, round_start);
        let val = serde_json::to_string(state)?;
        self.conn.set(key, val).await?;
        Ok(())
    }

    pub async fn load_round_state(
        &mut self,
        market_slug: &str,
        round_start: DateTime<Utc>,
    ) -> anyhow::Result<Option<TwoLegState>> {
        let key = Self::key(market_slug, round_start);
        let v: Option<String> = self.conn.get(key).await?;
        if let Some(json) = v {
            let state = serde_json::from_str(&json)?;
            Ok(Some(state))
        } else {
            Ok(None)
        }
    }

    pub async fn delete_round_state(
        &mut self,
        market_slug: &str,
        round_start: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        let key = Self::key(market_slug, round_start);
        let _: () = self.conn.del(key).await?;
        Ok(())
    }
}

