use std::cmp::Reverse;
use std::collections::{BTreeMap, VecDeque};

use crate::models::{Order, Side};

pub struct OrderBook {
    // Keyed by Reverse<price> so the highest bid is always at .first_key_value()
    bids: BTreeMap<Reverse<u64>, VecDeque<Order>>,
    // Keyed by price ascending so the lowest ask is always at .first_key_value()
    asks: BTreeMap<u64, VecDeque<Order>>,
}

impl OrderBook {
    pub fn new() -> Self {
        Self {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
        }
    }

    /// Place a limit order into the book. Does not attempt to match.
    pub fn add_limit_order(&mut self, order: Order) {
        match order.side {
            Side::Buy => {
                self.bids
                    .entry(Reverse(order.price))
                    .or_default()
                    .push_back(order);
            }
            Side::Sell => {
                self.asks
                    .entry(order.price)
                    .or_default()
                    .push_back(order);
            }
        }
    }

    /// Best bid = highest price someone is willing to buy at.
    pub fn best_bid(&self) -> Option<u64> {
        self.bids.first_key_value().map(|(Reverse(price), _)| *price)
    }

    /// Best ask = lowest price someone is willing to sell at.
    pub fn best_ask(&self) -> Option<u64> {
        self.asks.first_key_value().map(|(price, _)| *price)
    }

    // /// Peek at the front order at the best ask level (lowest price, earliest arrival).
    // pub fn best_ask_order(&mut self) -> Option<&mut Order> {
    //     self.asks
    //         .first_entry()
    //         .and_then(|e| e.into_mut().front_mut())
    // }

    // /// Peek at the front order at the best bid level (highest price, earliest arrival).
    // pub fn best_bid_order(&mut self) -> Option<&mut Order> {
    //     self.bids
    //         .first_entry()
    //         .and_then(|e| e.into_mut().front_mut())
    // }

    /// Remove and return the front order at the best ask level.
    pub fn pop_best_ask(&mut self) -> Option<Order> {
        let price = *self.asks.first_key_value()?.0;
        let queue = self.asks.get_mut(&price)?;
        let order = queue.pop_front();
        if queue.is_empty() {
            self.asks.remove(&price);
        }
        order
    }

    /// Remove and return the front order at the best bid level.
    pub fn pop_best_bid(&mut self) -> Option<Order> {
        let key = self.bids.first_key_value().map(|(k, _)| *k)?;
        let queue = self.bids.get_mut(&key)?;
        let order = queue.pop_front();
        if queue.is_empty() {
            self.bids.remove(&key);
        }
        order
    }

    /// Put an order back at the front of its price level (used after partial fill).
    pub fn return_to_front(&mut self, order: Order) {
        match order.side {
            Side::Buy => {
                self.bids
                    .entry(Reverse(order.price))
                    .or_default()
                    .push_front(order);
            }
            Side::Sell => {
                self.asks
                    .entry(order.price)
                    .or_default()
                    .push_front(order);
            }
        }
    }

    pub fn spread(&self) -> Option<u64> {
        Some(self.best_ask()?.checked_sub(self.best_bid()?)?)
    }

    /// All bid levels (price, total_qty) sorted highest price first.
    pub fn bids_snapshot(&self) -> Vec<(u64, u64)> {
        self.bids.iter().map(|(Reverse(price), queue)| {
            let qty: u64 = queue.iter().map(|o| o.remaining).sum();
            (*price, qty)
        }).collect()
    }

    /// All ask levels (price, total_qty) sorted lowest price first.
    pub fn asks_snapshot(&self) -> Vec<(u64, u64)> {
        self.asks.iter().map(|(price, queue)| {
            let qty: u64 = queue.iter().map(|o| o.remaining).sum();
            (*price, qty)
        }).collect()
    }
}
