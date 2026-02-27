use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Side of an order on the CLOB.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum OrderSide {
    Buy,
    Sell,
}

/// Order type; we only support simple limit orders for now.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum OrderType {
    Limit,
}

/// High-level lifecycle state for an order.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum OrderStatus {
    New,
    Open,
    PartiallyFilled,
    Filled,
    Canceled,
    Rejected,
    Failed,
}

/// Time-in-force semantics for orders.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TimeInForce {
    Gtc,
    Ioc,
    Fok,
}

impl Default for TimeInForce {
    fn default() -> Self {
        TimeInForce::Gtc
    }
}

/// Identifier used for tracking orders locally and, where supported, with the venue.
pub type OrderId = Uuid;

/// Request to place a new order.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderRequest {
    /// Human-meaningful market slug (e.g. "BTC-USD-15MIN").
    pub market_slug: String,
    /// Polymarket token identifier for the instrument being traded.
    pub token_id: String,
    pub side: OrderSide,
    /// Limit price in quote currency (0-1 for binary markets).
    pub price: f64,
    /// Order size in base units (shares).
    pub size: f64,
    /// Optional client-generated identifier for reconciliation.
    pub client_order_id: String,
    pub order_type: OrderType,
    pub time_in_force: TimeInForce,
}

/// Local view of an order, including lifecycle and fill information.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Order {
    pub id: OrderId,
    pub request: OrderRequest,
    pub status: OrderStatus,
    /// Total size filled so far.
    pub filled_size: f64,
    /// Average price across fills, if any.
    pub avg_fill_price: f64,
}

impl Order {
    pub fn new(id: OrderId, request: OrderRequest) -> Self {
        Self {
            id,
            request,
            status: OrderStatus::New,
            filled_size: 0.0,
            avg_fill_price: 0.0,
        }
    }
}

