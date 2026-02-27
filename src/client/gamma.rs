//! Polymarket Gamma API client for resolving 15-minute market slugs and token IDs.
//!
//! 15m markets use dynamic slugs: `{coin}-updown-15m-{round_start_unix}`.
//! Round start is current time in seconds floored to 900-second (15 min) buckets.

use serde::Deserialize;

use super::{ClientError, ClientResult};

const GAMMA_API_BASE: &str = "https://gamma-api.polymarket.com";

/// Current Unix timestamp floored to the start of the current 15-minute round (UTC).
pub fn current_15m_round_ts() -> i64 {
    chrono::Utc::now().timestamp() / 900 * 900
}

/// Build the Gamma API slug for a 15m UP/DOWN market.
/// `coin` should be lowercase, e.g. "btc", "eth", "sol".
pub fn slug_15m(coin: &str, round_ts: i64) -> String {
    format!("{}-updown-15m-{}", coin.to_lowercase(), round_ts)
}

/// Resolved market with CLOB token IDs for UP (Yes) and DOWN (No).
#[derive(Clone, Debug)]
pub struct ResolvedMarket {
    /// Logical name for logging/strategy (e.g. "BTC-USD-15MIN").
    pub slug: String,
    pub up_token_id: String,
    pub down_token_id: String,
}

#[derive(Debug, Deserialize)]
struct GammaMarketRow {
    #[serde(default, rename = "clobTokenIds")]
    clob_token_ids: Option<Vec<String>>,
    #[serde(default)]
    tokens: Option<Vec<GammaToken>>,
}

#[derive(Debug, Deserialize)]
struct GammaToken {
    token_id: String,
    outcome: String,
}

/// Fetch a single market by slug from the Gamma API.
/// Returns token IDs for Yes (index 0) and No (index 1), or None if not found/invalid.
pub async fn fetch_market_by_slug(
    http: &reqwest::Client,
    slug: &str,
) -> ClientResult<Option<ResolvedMarket>> {
    let url = format!("{}/markets", GAMMA_API_BASE);
    let resp = http
        .get(&url)
        .query(&[("slug", slug)])
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(ClientError::HttpStatus { status, body });
    }

    let rows: Vec<GammaMarketRow> = resp.json().await?;
    let row = match rows.first() {
        Some(r) => r,
        None => return Ok(None),
    };

    let (up_token_id, down_token_id) = if let Some(ids) = &row.clob_token_ids {
        if ids.len() >= 2 {
            (ids[0].clone(), ids[1].clone())
        } else {
            return Ok(None);
        }
    } else if let Some(tokens) = &row.tokens {
        let yes_id = tokens.iter().find(|t| t.outcome.eq_ignore_ascii_case("yes")).map(|t| t.token_id.clone());
        let no_id = tokens.iter().find(|t| t.outcome.eq_ignore_ascii_case("no")).map(|t| t.token_id.clone());
        match (yes_id, no_id) {
            (Some(y), Some(n)) => (y, n),
            _ => return Ok(None),
        }
    } else {
        return Ok(None);
    };

    Ok(Some(ResolvedMarket {
        slug: slug.to_string(),
        up_token_id,
        down_token_id,
    }))
}

/// Resolve the current 15m market for a coin and return token IDs.
/// `logical_slug` is used as the display name (e.g. "BTC-USD-15MIN").
pub async fn resolve_15m_market(
    http: &reqwest::Client,
    coin: &str,
    logical_slug: &str,
) -> ClientResult<Option<ResolvedMarket>> {
    let round_ts = current_15m_round_ts();
    let slug = slug_15m(coin, round_ts);
    let mut market = fetch_market_by_slug(http, &slug).await?;
    if let Some(ref mut m) = market {
        m.slug = logical_slug.to_string();
    }
    Ok(market)
}
