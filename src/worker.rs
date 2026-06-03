use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use sqlx::PgPool;

use crate::{db, models::{OrderType, Side}, solana::SolanaClient};

/// Runs forever: drains solana_queue every 2 seconds.
/// Safe to run alongside the HTTP server — all state is in the DB.
pub async fn run(pool: PgPool, solana: Arc<SolanaClient>) {
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if let Err(e) = drain(&pool, &solana).await {
            eprintln!("[worker] drain error: {e}");
        }
    }
}

async fn drain(pool: &PgPool, solana: &SolanaClient) -> anyhow::Result<()> {
    while let Some(event) = db::dequeue(pool).await? {
        match process(pool, solana, &event).await {
            Ok(()) => {
                db::queue_done(pool, event.id).await?;
            }
            Err(e) => {
                let msg = e.to_string();
                eprintln!("[worker] event {} ({}) failed (attempt {}): {msg}", event.id, event.event_type, event.retries + 1);
                db::queue_retry(pool, event.id, &msg).await?;
            }
        }
    }
    Ok(())
}

async fn process(pool: &PgPool, solana: &SolanaClient, event: &db::QueueEvent) -> anyhow::Result<()> {
    match event.event_type.as_str() {
        "place_order" => {
            let p: PlaceOrderPayload = serde_json::from_str(&event.payload)?;
            let side = if p.side == 0 { Side::Buy } else { Side::Sell };
            let order_type = if p.order_type == 0 { OrderType::Limit } else { OrderType::Market };

            let on_chain_id = solana.place_order(side, order_type, p.price, p.quantity).await?;

            // Persist the mapping so settle_match can resolve PDAs even after a restart.
            db::save_order_mapping(pool, p.in_memory_id, on_chain_id).await?;
        }

        "settle_match" => {
            let p: SettleMatchPayload = serde_json::from_str(&event.payload)?;

            let buy_on_chain = db::get_order_mapping(pool, p.buy_in_memory_id).await?
                .ok_or_else(|| anyhow::anyhow!("no on-chain mapping for buy order {}", p.buy_in_memory_id))?;
            let sell_on_chain = db::get_order_mapping(pool, p.sell_in_memory_id).await?
                .ok_or_else(|| anyhow::anyhow!("no on-chain mapping for sell order {}", p.sell_in_memory_id))?;

            solana.settle_match(buy_on_chain, sell_on_chain, p.fill_qty, p.price).await?;
        }

        t => anyhow::bail!("unknown event type: {t}"),
    }
    Ok(())
}

#[derive(Deserialize)]
struct PlaceOrderPayload {
    in_memory_id: u64,
    side: u8,        // 0 = Buy, 1 = Sell
    order_type: u8,  // 0 = Limit, 1 = Market
    price: u64,
    quantity: u64,
}

#[derive(Deserialize)]
struct SettleMatchPayload {
    buy_in_memory_id: u64,
    sell_in_memory_id: u64,
    fill_qty: u64,
    price: u64,
}
