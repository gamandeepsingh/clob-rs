use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Buy => "buy",
            Side::Sell => "sell",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderType {
    Limit,
    Market,
}

impl OrderType {
    pub fn as_str(self) -> &'static str {
        match self {
            OrderType::Limit => "limit",
            OrderType::Market => "market",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    Open,
    PartiallyFilled,
    Filled,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
pub struct Order {
    pub id: u64,
    pub side: Side,
    pub order_type: OrderType,
    pub status: OrderStatus,
    /// Price in minor units (e.g. cents). Ignored for Market orders.
    pub price: u64,
    /// Total quantity originally requested.
    pub quantity: u64,
    /// How much is still unfilled.
    pub remaining: u64,
    /// Unix timestamp in microseconds — used for time priority.
    pub timestamp: u64,
}

impl Order {
    pub fn new(id: u64, side: Side, order_type: OrderType, price: u64, quantity: u64, timestamp: u64) -> Self {
        Self {
            id,
            side,
            order_type,
            status: OrderStatus::Open,
            price,
            quantity,
            remaining: quantity,
            timestamp,
        }
    }

    pub fn is_filled(&self) -> bool {
        self.remaining == 0
    }
}

/// Produced when a buy order and a sell order successfully match.
#[derive(Debug, Clone, Serialize)]
pub struct Trade {
    pub buy_order_id: u64,
    pub sell_order_id: u64,
    /// The price at which the trade executed.
    pub price: u64,
    /// How many units changed hands.
    pub quantity: u64,
    pub timestamp: u64,
}
