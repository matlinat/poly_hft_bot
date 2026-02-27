use thiserror::Error;

pub mod auth;
pub mod clob;
pub mod gamma;
pub mod websocket;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("HTTP status {status}: {body}")]
    HttpStatus {
        status: reqwest::StatusCode,
        body: String,
    },

    #[error("websocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),

    #[error("serialization error: {0}")]
    SerdeJson(#[from] serde_json::Error),

    #[error("HMAC error: {0}")]
    Hmac(String),

    #[error("EIP-712 error: {0}")]
    Eip712(String),

    #[error("configuration error: {0}")]
    Config(String),
}

pub type ClientResult<T> = Result<T, ClientError>;

