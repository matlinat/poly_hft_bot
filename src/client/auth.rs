use std::time::{SystemTime, UNIX_EPOCH};

use alloy::{
    primitives::{Address, U256},
    signers::local::PrivateKeySigner,
    signers::Signer,
};
use alloy_sol_types::{eip712_domain, sol};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::types::ApiConfig;

use super::{ClientError, ClientResult};

type HmacSha256 = Hmac<Sha256>;

const CLOB_DOMAIN_NAME: &str = "ClobAuthDomain";
const CLOB_VERSION: &str = "1";
const MSG_TO_SIGN: &str = "This message attests that I control the given wallet";

sol! {
    /// EIP-712 struct for CLOB L1 authentication.
    struct ClobAuth {
        address address;
        string timestamp;
        uint256 nonce;
        string message;
    }
}

/// Build an alloy private key signer from the configured wallet private key.
pub fn build_signer_from_config(config: &ApiConfig, chain_id: u64) -> ClientResult<PrivateKeySigner> {
    let mut signer: PrivateKeySigner = config
        .wallet_private_key
        .parse()
        .map_err(|e| ClientError::Config(format!("failed to parse wallet private key: {e}")))?;

    signer.set_chain_id(Some(chain_id.into()));
    Ok(signer)
}

/// Build the canonical Polymarket CLOB EIP-712 signature used for L1 auth headers.
pub async fn build_clob_eip712_signature(
    signer: &PrivateKeySigner,
    chain_id: u64,
    timestamp: i64,
    nonce: u64,
) -> ClientResult<String> {
    let ts = timestamp.to_string();
    let address: Address = signer.address();

    let domain = eip712_domain! {
        name: CLOB_DOMAIN_NAME,
        version: CLOB_VERSION,
        chain_id: chain_id,
    };

    let payload = ClobAuth {
        address,
        timestamp: ts.into(),
        nonce: U256::from(nonce),
        message: MSG_TO_SIGN.into(),
    };

    let sig = signer
        .sign_typed_data(&payload, &domain)
        .await
        .map_err(|e| ClientError::Eip712(e.to_string()))?;

    Ok(sig.to_string())
}

fn sanitize_base64_secret(secret: &str) -> String {
    secret
        .chars()
        .filter_map(|c| match c {
            '-' => Some('+'),
            '_' => Some('/'),
            'A'..='Z' | 'a'..='z' | '0'..='9' | '+' | '/' | '=' => Some(c),
            _ => None,
        })
        .collect()
}

/// Build the canonical Polymarket CLOB HMAC signature for L2 auth headers.
pub fn build_poly_hmac_signature(
    secret: &str,
    timestamp: i64,
    method: &str,
    request_path: &str,
    body: Option<&str>,
) -> ClientResult<String> {
    let mut message = format!("{timestamp}{method}{request_path}");
    if let Some(body) = body {
        message.push_str(body);
    }

    let sanitized = sanitize_base64_secret(secret);
    let key_bytes = BASE64_STANDARD
        .decode(sanitized)
        .map_err(|e| ClientError::Hmac(format!("invalid base64 secret: {e}")))?;

    let mut mac =
        HmacSha256::new_from_slice(&key_bytes).map_err(|e| ClientError::Hmac(e.to_string()))?;
    mac.update(message.as_bytes());
    let signature = mac.finalize().into_bytes();

    let b64 = BASE64_STANDARD.encode(signature);
    let sig_url_safe = b64.replace('+', "-").replace('/', "_");
    Ok(sig_url_safe)
}

/// Convenience helper to get the current UNIX timestamp in seconds.
pub fn current_unix_timestamp() -> i64 {
    Utc::now().timestamp()
}

/// Convenience helper to get a high-entropy nonce from the system clock.
pub fn default_nonce() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or_else(|_| rand::random())
}

