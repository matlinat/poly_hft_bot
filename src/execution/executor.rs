use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::client::clob::ClobClient;
use crate::client::ClientError;
use crate::monitoring::metrics::METRICS;
use crate::strategy::{LegSide, TwoLegDecision};
use crate::types::{AppConfig, ExecutionMode, MarketConfig};

use super::order::{
    Order, OrderId, OrderRequest, OrderSide, OrderStatus, OrderType, TimeInForce,
};

#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("client error: {0}")]
    Client(#[from] ClientError),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("circuit breaker open")]
    CircuitOpen,

    #[error("order not found: {0}")]
    OrderNotFound(String),

    #[error("other execution error: {0}")]
    Other(String),
}

pub type ExecutionResult<T> = Result<T, ExecutionError>;

/// Simple on-process circuit breaker for execution failures.
#[derive(Debug)]
struct CircuitBreaker {
    failures: u32,
    threshold: u32,
    cooldown: Duration,
    opened_at: Option<Instant>,
}

impl CircuitBreaker {
    fn new(threshold: u32, cooldown: Duration) -> Self {
        Self {
            failures: 0,
            threshold,
            cooldown,
            opened_at: None,
        }
    }

    fn is_open(&self) -> bool {
        match self.opened_at {
            None => false,
            Some(opened) => opened.elapsed() < self.cooldown,
        }
    }

    fn allow(&mut self) -> bool {
        if self.is_open() {
            return false;
        }
        true
    }

    fn on_success(&mut self) {
        self.failures = 0;
        self.opened_at = None;
    }

    fn on_failure(&mut self) {
        self.failures = self.failures.saturating_add(1);
        if self.failures >= self.threshold {
            self.opened_at = Some(Instant::now());
            warn!(
                failures = self.failures,
                "execution circuit breaker opened after consecutive failures"
            );
        }
    }
}

/// Backend for execution – either simulated (paper) or live CLOB.
enum ExecutionBackend {
    Paper(PaperExecutor),
    Live(LiveExecutor),
}

/// High-level order executor that owns backend adapter, circuit breaker and local order book.
pub struct OrderExecutor {
    backend: ExecutionBackend,
    breaker: CircuitBreaker,
    markets_by_slug: HashMap<String, MarketConfig>,
    orders: HashMap<OrderId, Order>,
}

impl OrderExecutor {
    pub fn from_config(cfg: &AppConfig) -> ExecutionResult<Self> {
        let markets_by_slug = cfg
            .markets
            .markets
            .iter()
            .cloned()
            .map(|m| (m.slug.clone(), m))
            .collect::<HashMap<_, _>>();

        if markets_by_slug.is_empty() {
            return Err(ExecutionError::Config(
                "no markets configured for execution".to_string(),
            ));
        }

        let backend = match cfg.execution.mode {
            ExecutionMode::Paper => ExecutionBackend::Paper(PaperExecutor::new()),
            ExecutionMode::Live => {
                let clob = ClobClient::new(&cfg.api)?;
                ExecutionBackend::Live(LiveExecutor::new(clob))
            }
        };

        Ok(Self {
            backend,
            breaker: CircuitBreaker::new(5, Duration::from_secs(30)),
            markets_by_slug,
            orders: HashMap::new(),
        })
    }

    /// Convert a high-level strategy decision into an order request and send it to the backend.
    pub async fn execute_decision(&mut self, decision: TwoLegDecision) -> ExecutionResult<OrderId> {
        if !self.breaker.allow() {
            return Err(ExecutionError::CircuitOpen);
        }

        let req = self.decision_to_order_request(&decision)?;

        let result = match &self.backend {
            ExecutionBackend::Paper(paper) => paper.execute_order(&req).await,
            ExecutionBackend::Live(live) => live.execute_order(&req).await,
        };

        match result {
            Ok(mut order) => {
                // Newly submitted orders start as Open or Filled depending on backend behavior.
                if matches!(order.status, OrderStatus::New) {
                    order.status = OrderStatus::Open;
                }
                let id = order.id;
                self.orders.insert(id, order);
                self.breaker.on_success();
                Ok(id)
            }
            Err(err) => {
                self.breaker.on_failure();
                // Tag failures with market slug so monitoring and alerting can react.
                let market_slug = match &decision {
                    TwoLegDecision::OpenLeg1 { market_slug, .. } => market_slug.as_str(),
                    TwoLegDecision::OpenLeg2 { market_slug, .. } => market_slug.as_str(),
                };
                METRICS.record_order_failed(market_slug, &err.to_string());
                Err(err)
            }
        }
    }

    /// Cancel an existing order if supported by backend.
    pub async fn cancel_order(&mut self, id: OrderId) -> ExecutionResult<()> {
        if !self.breaker.allow() {
            return Err(ExecutionError::CircuitOpen);
        }

        if !self.orders.contains_key(&id) {
            return Err(ExecutionError::OrderNotFound(id.to_string()));
        }

        let result = match &self.backend {
            ExecutionBackend::Paper(paper) => paper.cancel_order(id).await,
            ExecutionBackend::Live(live) => live.cancel_order(id).await,
        };

        match result {
            Ok(()) => {
                if let Some(order) = self.orders.get_mut(&id) {
                    if matches!(order.status, OrderStatus::Filled | OrderStatus::Rejected) {
                        // Leave terminal states unchanged.
                    } else {
                        order.status = OrderStatus::Canceled;
                    }
                }
                self.breaker.on_success();
                Ok(())
            }
            Err(err) => {
                self.breaker.on_failure();
                Err(err)
            }
        }
    }

    /// Refresh local view of an order from the backend, if supported.
    pub async fn reconcile_order(&mut self, id: OrderId) -> ExecutionResult<Order> {
        if !self.breaker.allow() {
            return Err(ExecutionError::CircuitOpen);
        }

        let result = match &self.backend {
            ExecutionBackend::Paper(paper) => paper.refresh_order(id).await,
            ExecutionBackend::Live(live) => live.refresh_order(id).await,
        };

        match result {
            Ok(order) => {
                self.orders.insert(id, order.clone());
                self.breaker.on_success();
                Ok(order)
            }
            Err(err) => {
                self.breaker.on_failure();
                Err(err)
            }
        }
    }

    /// Read-only access to an order in the local book.
    pub fn order(&self, id: &OrderId) -> Option<&Order> {
        self.orders.get(id)
    }

    fn decision_to_order_request(&self, decision: &TwoLegDecision) -> ExecutionResult<OrderRequest> {
        let (market_slug, round_start, leg_side, shares, limit_price, expected_profit, leg_label) =
            match decision {
                TwoLegDecision::OpenLeg1 {
                    market_slug,
                    round_start,
                    side,
                    shares,
                    limit_price,
                } => (
                    market_slug.clone(),
                    *round_start,
                    *side,
                    *shares,
                    *limit_price,
                    None,
                    "leg1",
                ),
                TwoLegDecision::OpenLeg2 {
                    market_slug,
                    round_start,
                    side,
                    shares,
                    limit_price,
                    expected_locked_profit,
                } => (
                    market_slug.clone(),
                    *round_start,
                    *side,
                    *shares,
                    *limit_price,
                    Some(*expected_locked_profit),
                    "leg2",
                ),
            };

        let market = self
            .markets_by_slug
            .get(&market_slug)
            .ok_or_else(|| ExecutionError::Config(format!("unknown market slug: {market_slug}")))?;

        let token_id = match leg_side {
            LegSide::Up => market.up_token_id.clone(),
            LegSide::Down => market.down_token_id.clone(),
        };

        let client_order_id = format!(
            "{}-{}-{}",
            market_slug,
            round_start.to_rfc3339(),
            match decision {
                TwoLegDecision::OpenLeg1 { .. } => "leg1",
                TwoLegDecision::OpenLeg2 { .. } => "leg2",
            }
        );

        if let Some(p) = expected_profit {
            info!(
                market = %market_slug,
                %round_start,
                %token_id,
                side = ?leg_side,
                shares,
                price = limit_price,
                expected_profit = p,
                "submitting hedge order (Leg2)"
            );
        } else {
            info!(
                market = %market_slug,
                %round_start,
                %token_id,
                side = ?leg_side,
                shares,
                price = limit_price,
                "submitting directional order (Leg1)"
            );
        }

        // Emit high-level metrics hook for monitoring.
        METRICS.record_order_submitted(&market_slug, leg_label);

        Ok(OrderRequest {
            market_slug,
            token_id,
            side: OrderSide::Buy,
            price: limit_price,
            size: shares,
            client_order_id,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
        })
    }
}

/// Extremely simple paper-trading adapter: fills all orders immediately at limit price.
struct PaperExecutor;

impl PaperExecutor {
    fn new() -> Self {
        Self
    }

    async fn execute_order(&self, req: &OrderRequest) -> ExecutionResult<Order> {
        // Simulate small network/venue latency.
        tokio::time::sleep(Duration::from_millis(5)).await;

        let id = OrderId::new_v4();
        let mut order = Order::new(id, req.clone());
        order.status = OrderStatus::Filled;
        order.filled_size = req.size;
        order.avg_fill_price = req.price;

        Ok(order)
    }

    async fn cancel_order(&self, _id: OrderId) -> ExecutionResult<()> {
        // Paper mode treats cancellation as always-successful.
        Ok(())
    }

    async fn refresh_order(&self, id: OrderId) -> ExecutionResult<Order> {
        // In pure paper mode, everything is filled immediately; synthesize a filled order.
        let dummy_req = OrderRequest {
            market_slug: "paper".to_string(),
            token_id: "paper".to_string(),
            side: OrderSide::Buy,
            price: 0.5,
            size: 0.0,
            client_order_id: id.to_string(),
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
        };
        let mut order = Order::new(id, dummy_req);
        order.status = OrderStatus::Filled;
        Ok(order)
    }
}

/// Live Polymarket CLOB adapter. This is intentionally conservative and keeps the
/// response model minimal; it can be extended as we integrate more endpoints.
struct LiveExecutor {
    clob: ClobClient,
}

impl LiveExecutor {
    fn new(clob: ClobClient) -> Self {
        Self { clob }
    }

    async fn execute_order(&self, req: &OrderRequest) -> ExecutionResult<Order> {
        #[derive(Serialize)]
        struct PlaceOrderRequest<'a> {
            token_id: &'a str,
            side: &'a str,
            price: f64,
            size: f64,
            client_order_id: &'a str,
            #[serde(rename = "type")]
            order_type: &'a str,
            time_in_force: &'a str,
        }

        #[derive(Deserialize)]
        struct PlaceOrderResponse {
            id: String,
            status: String,
            filled_size: Option<f64>,
            avg_fill_price: Option<f64>,
        }

        let payload = PlaceOrderRequest {
            token_id: &req.token_id,
            side: match req.side {
                OrderSide::Buy => "buy",
                OrderSide::Sell => "sell",
            },
            price: req.price,
            size: req.size,
            client_order_id: &req.client_order_id,
            order_type: "limit",
            time_in_force: match req.time_in_force {
                TimeInForce::Gtc => "gtc",
                TimeInForce::Ioc => "ioc",
                TimeInForce::Fok => "fok",
            },
        };

        let resp: PlaceOrderResponse = self.clob.post_private("/orders", &payload).await?;

        let id = resp
            .id
            .parse::<OrderId>()
            .unwrap_or_else(|_| OrderId::new_v4());

        let mut order = Order::new(id, req.clone());
        order.status = map_status(&resp.status);
        order.filled_size = resp.filled_size.unwrap_or(0.0);
        order.avg_fill_price = resp.avg_fill_price.unwrap_or(0.0);

        Ok(order)
    }

    async fn cancel_order(&self, id: OrderId) -> ExecutionResult<()> {
        #[derive(Deserialize)]
        struct CancelResponse {
            status: String,
        }

        let path = format!("/orders/{id}");
        // Polymarket CLOB typically uses DELETE for cancellation; treat non-success as error.
        let resp: CancelResponse =
            self.clob
                .post_private::<(), _>(&path, &())
                .await
                .map_err(ExecutionError::from)?;

        if resp.status.to_lowercase() == "canceled" {
            Ok(())
        } else {
            Err(ExecutionError::Other(format!(
                "unexpected cancel status {status}",
                status = resp.status
            )))
        }
    }

    async fn refresh_order(&self, id: OrderId) -> ExecutionResult<Order> {
        #[derive(Deserialize)]
        struct OrderResponse {
            id: String,
            status: String,
            filled_size: Option<f64>,
            avg_fill_price: Option<f64>,
            token_id: String,
            price: f64,
            size: f64,
        }

        let path = format!("/orders/{id}");
        let resp: OrderResponse = self.clob.get_private(&path).await?;

        let parsed_id = resp
            .id
            .parse::<OrderId>()
            .unwrap_or_else(|_| OrderId::new_v4());

        let req = OrderRequest {
            market_slug: "unknown".to_string(),
            token_id: resp.token_id,
            side: OrderSide::Buy, // direction not strictly needed for reconciliation here
            price: resp.price,
            size: resp.size,
            client_order_id: parsed_id.to_string(),
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::Gtc,
        };

        let mut order = Order::new(parsed_id, req);
        order.status = map_status(&resp.status);
        order.filled_size = resp.filled_size.unwrap_or(0.0);
        order.avg_fill_price = resp.avg_fill_price.unwrap_or(0.0);

        Ok(order)
    }
}

fn map_status(s: &str) -> OrderStatus {
    match s.to_lowercase().as_str() {
        "new" => OrderStatus::New,
        "open" => OrderStatus::Open,
        "partially_filled" | "partially-filled" => OrderStatus::PartiallyFilled,
        "filled" => OrderStatus::Filled,
        "canceled" | "cancelled" => OrderStatus::Canceled,
        "rejected" => OrderStatus::Rejected,
        _ => OrderStatus::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BotConfig, ExecutionConfig, MarketsConfig, PostgresConfig, RedisConfig};

    fn dummy_app_config(mode: ExecutionMode) -> AppConfig {
        AppConfig {
            redis: RedisConfig {
                url: "redis://localhost".to_string(),
            },
            postgres: PostgresConfig {
                url: "postgres://localhost".to_string(),
            },
            api: crate::types::ApiConfig {
                base_url: "https://clob.polymarket.com".to_string(),
                ws_url: "wss://clob.polymarket.com/ws".to_string(),
                api_key: "key".to_string(),
                api_secret: "secret".to_string(),
                api_passphrase: "pass".to_string(),
                wallet_private_key: "priv".to_string(),
                gnosis_safe_address: Some("0x0000000000000000000000000000000000000001".to_string()),
            },
            bot: BotConfig {
                shares: 10.0,
                sum_target: 0.95,
                move_pct: 0.1,
                window_min: 2,
                max_concurrent_trades: 1,
                risk_per_trade_pct: 2.0,
                fee_rate: 0.02,
                min_profit_usd: 0.1,
            },
            markets: MarketsConfig {
                markets: vec![MarketConfig {
                    slug: "BTC-USD-15MIN".to_string(),
                    up_token_id: "BTC_15M_UP".to_string(),
                    down_token_id: "BTC_15M_DOWN".to_string(),
                }],
            },
            execution: ExecutionConfig {
                mode,
                max_parallel_orders: 32,
            },
        }
    }

    #[test]
    fn circuit_breaker_opens_after_failures() {
        let mut cb = CircuitBreaker::new(3, Duration::from_secs(60));
        assert!(cb.allow());
        cb.on_failure();
        cb.on_failure();
        assert!(cb.allow());
        cb.on_failure();
        assert!(cb.is_open());
        assert!(!cb.allow());
    }

    #[test]
    fn map_status_basic() {
        assert_eq!(map_status("new"), OrderStatus::New);
        assert_eq!(map_status("open"), OrderStatus::Open);
        assert_eq!(map_status("filled"), OrderStatus::Filled);
        assert_eq!(map_status("canceled"), OrderStatus::Canceled);
        assert_eq!(map_status("rejected"), OrderStatus::Rejected);
        assert_eq!(map_status("somethingelse"), OrderStatus::Failed);
    }

    #[test]
    fn build_executor_from_config_paper() {
        let cfg = dummy_app_config(ExecutionMode::Paper);
        let exec = OrderExecutor::from_config(&cfg);
        assert!(exec.is_ok());
    }
}

