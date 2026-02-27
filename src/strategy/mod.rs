use chrono::{DateTime, Utc};

pub mod params;
pub mod two_leg;

pub use params::TwoLegParams;
pub use two_leg::{LegSide, TwoLegDecision, TwoLegEngine, TwoLegState};

/// Normalized snapshot of a Polymarket 15-minute UP/DOWN market.
#[derive(Clone, Debug)]
pub struct MarketSnapshot {
    pub ts: DateTime<Utc>,
    pub market_slug: String,
    /// Best bid/ask for the UP token (0-1 price).
    pub up_bid: f64,
    pub up_ask: f64,
    /// Best bid/ask for the DOWN token (0-1 price).
    pub down_bid: f64,
    pub down_ask: f64,
}

impl MarketSnapshot {
    /// Mid price for the UP token.
    pub fn mid_up(&self) -> f64 {
        0.5 * (self.up_bid + self.up_ask)
    }

    /// Mid price for the DOWN token.
    pub fn mid_down(&self) -> f64 {
        0.5 * (self.down_bid + self.down_ask)
    }
}

