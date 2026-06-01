mod engine;
mod models;
mod orderbook;

use engine::Engine;
use models::{OrderType, Side};

fn main() {
    let mut eng = Engine::new();

    // Two resting sell limit orders sitting in the book.
    eng.submit(Side::Sell, OrderType::Limit, 10_000, 5);  // sell 5 @ $100.00
    eng.submit(Side::Sell, OrderType::Limit, 10_200, 3);  // sell 3 @ $102.00

    println!("Best ask: {:?}", eng.best_ask());  // should be 10_000
    println!("Best bid: {:?}", eng.best_bid());  // None — no buyers yet

    // A buyer comes in willing to pay up to $103.00 for 7 units.
    // Should sweep the $100 level (5 units) and the $102 level (2 units).
    let trades = eng.submit(Side::Buy, OrderType::Limit, 10_300, 7);

    println!("\nTrades produced: {}", trades.len());
    for t in &trades {
        println!(
            "  trade: qty={} @ price={} (buy_order={}, sell_order={})",
            t.quantity, t.price, t.buy_order_id, t.sell_order_id
        );
    }

    // One sell order still has 1 unit left at $102 (we only bought 2 of its 3).
    println!("\nBest ask after match: {:?}", eng.best_ask());  // 10_200
    println!("Spread: {:?}", eng.spread());                    // None — no bids left
}
