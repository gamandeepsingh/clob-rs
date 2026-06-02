use std::time::{SystemTime, UNIX_EPOCH};

use crate::models::{Order, OrderStatus, OrderType, Side, Trade};
use crate::orderbook::OrderBook;

pub struct Engine {
    book: OrderBook,
    next_order_id: u64,
}

impl Engine {
    pub fn new() -> Self {
        Self {
            book: OrderBook::new(),
            next_order_id: 1,
        }
    }

    /// Submit a new order. Returns (assigned_order_id, trades_produced).
    pub fn submit(&mut self, side: Side, order_type: OrderType, price: u64, quantity: u64) -> (u64, Vec<Trade>) {
        let id = self.next_id();
        let ts = now_micros();
        let mut incoming = Order::new(id, side, order_type, price, quantity, ts);
        let mut trades = Vec::new();

        loop {
            if incoming.is_filled() {
                break;
            }

            // Try to find the best resting order on the opposite side.
            // Also check whether the prices cross before popping.
            let crosses = match incoming.side {
                Side::Buy => match incoming.order_type {
                    OrderType::Market => self.book.best_ask().is_some(),
                    OrderType::Limit => self
                        .book
                        .best_ask()
                        .is_some_and(|ask| incoming.price >= ask),
                },
                Side::Sell => match incoming.order_type {
                    OrderType::Market => self.book.best_bid().is_some(),
                    OrderType::Limit => self
                        .book
                        .best_bid()
                        .is_some_and(|bid| incoming.price <= bid),
                },
            };

            if !crosses {
                break;
            }

            // Pop the resting order (we own it now, so we can mutate freely).
            let mut resting = match incoming.side {
                Side::Buy => self.book.pop_best_ask().unwrap(),
                Side::Sell => self.book.pop_best_bid().unwrap(),
            };

            // How much can we trade? The smaller of the two remaining quantities.
            let fill_qty = incoming.remaining.min(resting.remaining);
            // Resting order's price always wins.
            let trade_price = resting.price;
            let trade_ts = now_micros();

            // Reduce remaining on both orders.
            incoming.remaining -= fill_qty;
            resting.remaining -= fill_qty;

            // Update statuses.
            update_status(&mut incoming);
            update_status(&mut resting);

            let (buy_id, sell_id) = match incoming.side {
                Side::Buy => (incoming.id, resting.id),
                Side::Sell => (resting.id, incoming.id),
            };

            trades.push(Trade {
                buy_order_id: buy_id,
                sell_order_id: sell_id,
                price: trade_price,
                quantity: fill_qty,
                timestamp: trade_ts,
            });

            // If the resting order still has quantity left, put it back at the
            // front so it keeps its time priority for the next incoming order.
            if !resting.is_filled() {
                self.book.return_to_front(resting);
            }
        }

        // Any unfilled remainder of a limit order rests in the book.
        // A market order with leftover is simply dropped (no price to queue at).
        if !incoming.is_filled() && incoming.order_type == OrderType::Limit {
            self.book.add_limit_order(incoming);
        }

        (id, trades)
    }

    pub fn best_bid(&self) -> Option<u64> {
        self.book.best_bid()
    }

    pub fn best_ask(&self) -> Option<u64> {
        self.book.best_ask()
    }

    pub fn spread(&self) -> Option<u64> {
        self.book.spread()
    }

    pub fn bids_snapshot(&self) -> Vec<(u64, u64)> {
        self.book.bids_snapshot()
    }

    pub fn asks_snapshot(&self) -> Vec<(u64, u64)> {
        self.book.asks_snapshot()
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_order_id;
        self.next_order_id += 1;
        id
    }
}

fn update_status(order: &mut Order) {
    order.status = if order.remaining == 0 {
        OrderStatus::Filled
    } else if order.remaining < order.quantity {
        OrderStatus::PartiallyFilled
    } else {
        OrderStatus::Open
    };
}

fn now_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64
}
