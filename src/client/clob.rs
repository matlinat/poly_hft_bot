use std::time::Duration;
use reqwest::{Client, Method};
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::time::sleep;

use crate::types::ApiConfig;

use super::auth::{build_poly_hmac_signature, current_unix_timestamp};
use super::{ClientError, ClientResult};

const DEFAULT_MAX_RETRIES: u32 = 3;

pub struct ClobClient {
    http: Client,
    base_url: String,
    address: String,
    api_key: String,
    api_secret: String,
    api_passphrase: String,
    max_retries: u32,
}

impl ClobClient {
    pub fn new(config: &ApiConfig) -> ClientResult<Self> {
        let http = Client::builder()
            .user_agent("polymarket-hft-bot/0.1")
            .build()
            .map_err(ClientError::Http)?;

        let address = if let Some(addr) = &config.gnosis_safe_address {
            if !addr.is_empty() {
                addr.clone()
            } else {
                return Err(ClientError::Config(
                    "gnosis_safe_address must be configured for L2 auth".to_string(),
                ));
            }
        } else {
            return Err(ClientError::Config(
                "gnosis_safe_address must be configured for L2 auth".to_string(),
            ));
        };

        Ok(Self {
            http,
            base_url: config.base_url.clone(),
            address,
            api_key: config.api_key.clone(),
            api_secret: config.api_secret.clone(),
            api_passphrase: config.api_passphrase.clone(),
            max_retries: DEFAULT_MAX_RETRIES,
        })
    }

    fn build_url_and_path(&self, path: &str) -> (String, String) {
        let request_path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };

        let url = format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            request_path.trim_start_matches('/')
        );

        (url, request_path)
    }

    fn backoff(attempt: u32) -> Duration {
        let capped = attempt.min(5);
        let millis = 500 * (1_u64 << capped);
        Duration::from_millis(millis.min(8_000))
    }

    async fn send_signed_request<TBody, TResp>(
        &self,
        method: Method,
        path: &str,
        body: Option<&TBody>,
    ) -> ClientResult<TResp>
    where
        TBody: Serialize + ?Sized,
        TResp: DeserializeOwned,
    {
        let (url, request_path) = self.build_url_and_path(path);
        let timestamp = current_unix_timestamp();

        let body_json = if let Some(body) = body {
            Some(serde_json::to_string(body)?)
        } else {
            None
        };

        let signature = build_poly_hmac_signature(
            &self.api_secret,
            timestamp,
            method.as_str(),
            &request_path,
            body_json.as_deref(),
        )?;

        let mut attempt = 0;
        loop {
            let mut req = self
                .http
                .request(method.clone(), &url)
                .header("POLY_ADDRESS", &self.address)
                .header("POLY_SIGNATURE", &signature)
                .header("POLY_TIMESTAMP", timestamp.to_string())
                .header("POLY_API_KEY", &self.api_key)
                .header("POLY_PASSPHRASE", &self.api_passphrase);

            if let Some(body) = &body_json {
                req = req
                    .header("Content-Type", "application/json")
                    .body(body.clone());
            }

            match req.send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        let parsed = resp.json::<TResp>().await?;
                        return Ok(parsed);
                    }

                    if resp.status().is_server_error() && attempt < self.max_retries {
                        attempt += 1;
                        sleep(Self::backoff(attempt)).await;
                        continue;
                    }

                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    return Err(ClientError::HttpStatus { status, body });
                }
                Err(err) => {
                    if attempt < self.max_retries {
                        attempt += 1;
                        sleep(Self::backoff(attempt)).await;
                        continue;
                    }

                    return Err(ClientError::Http(err));
                }
            }
        }
    }

    pub async fn get_private<TResp>(&self, path: &str) -> ClientResult<TResp>
    where
        TResp: DeserializeOwned,
    {
        self.send_signed_request(Method::GET, path, Option::<&()>::None)
            .await
    }

    pub async fn post_private<TBody, TResp>(
        &self,
        path: &str,
        body: &TBody,
    ) -> ClientResult<TResp>
    where
        TBody: Serialize + ?Sized,
        TResp: DeserializeOwned,
    {
        self.send_signed_request(Method::POST, path, Some(body)).await
    }

    /// Simple helper for public GET endpoints that do not require auth.
    pub async fn get_public<TResp>(&self, path: &str) -> ClientResult<TResp>
    where
        TResp: DeserializeOwned,
    {
        let (url, _) = self.build_url_and_path(path);
        let resp = self.http.get(url).send().await?;
        if resp.status().is_success() {
            let parsed = resp.json::<TResp>().await?;
            Ok(parsed)
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            Err(ClientError::HttpStatus { status, body })
        }
    }
}

