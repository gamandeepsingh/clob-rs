use sqlx::PgPool;

use crate::models::Trade;

pub async fn init(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS orders (
            id          BIGINT PRIMARY KEY,
            side        TEXT   NOT NULL,
            order_type  TEXT   NOT NULL,
            status      TEXT   NOT NULL,
            price       BIGINT NOT NULL,
            quantity    BIGINT NOT NULL,
            remaining   BIGINT NOT NULL,
            created_at  BIGINT NOT NULL
        )",
    ).execute(pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS trades (
            id            BIGSERIAL PRIMARY KEY,
            buy_order_id  BIGINT NOT NULL,
            sell_order_id BIGINT NOT NULL,
            price         BIGINT NOT NULL,
            quantity      BIGINT NOT NULL,
            executed_at   BIGINT NOT NULL
        )",
    ).execute(pool).await?;

    // Outbox: Solana operations waiting to be confirmed on-chain.
    // retries < 10 is the worker's cut-off; after that the row stays for manual inspection.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS solana_queue (
            id          BIGSERIAL PRIMARY KEY,
            event_type  TEXT NOT NULL,
            payload     TEXT NOT NULL,
            retries     INT  NOT NULL DEFAULT 0,
            last_error  TEXT,
            created_at  BIGINT NOT NULL
        )",
    ).execute(pool).await?;

    // Maps in-memory order_id → on-chain order_id (PDA seed).
    // Persists across restarts so settle_match can always find the right PDAs.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS solana_order_map (
            in_memory_id BIGINT PRIMARY KEY,
            on_chain_id  BIGINT NOT NULL
        )",
    ).execute(pool).await?;

    Ok(())
}

// ── Orders / Trades ───────────────────────────────────────────────────────────

pub async fn save_order(
    pool: &PgPool,
    order_id: u64,
    side: &str,
    order_type: &str,
    price: u64,
    quantity: u64,
    timestamp: u64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO orders (id, side, order_type, status, price, quantity, remaining, created_at)
         VALUES ($1, $2, $3, 'open', $4, $5, $5, $6)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(order_id as i64).bind(side).bind(order_type)
    .bind(price as i64).bind(quantity as i64).bind(timestamp as i64)
    .execute(pool).await?;
    Ok(())
}

pub async fn save_trade(pool: &PgPool, trade: &Trade) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO trades (buy_order_id, sell_order_id, price, quantity, executed_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(trade.buy_order_id as i64).bind(trade.sell_order_id as i64)
    .bind(trade.price as i64).bind(trade.quantity as i64).bind(trade.timestamp as i64)
    .execute(pool).await?;
    Ok(())
}

// ── Solana outbox queue ───────────────────────────────────────────────────────

pub async fn enqueue(
    pool: &PgPool,
    event_type: &str,
    payload: &str,
    now: u64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO solana_queue (event_type, payload, created_at) VALUES ($1, $2, $3)",
    )
    .bind(event_type).bind(payload).bind(now as i64)
    .execute(pool).await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
pub struct QueueEvent {
    pub id: i64,
    pub event_type: String,
    pub payload: String,
    pub retries: i32,
}

pub async fn dequeue(pool: &PgPool) -> Result<Option<QueueEvent>, sqlx::Error> {
    sqlx::query_as::<_, QueueEvent>(
        "SELECT id, event_type, payload, retries
         FROM solana_queue
         WHERE retries < 10
         ORDER BY id ASC
         LIMIT 1",
    )
    .fetch_optional(pool).await
}

pub async fn queue_done(pool: &PgPool, id: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM solana_queue WHERE id = $1")
        .bind(id).execute(pool).await?;
    Ok(())
}

pub async fn queue_retry(pool: &PgPool, id: i64, error: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE solana_queue SET retries = retries + 1, last_error = $2 WHERE id = $1",
    )
    .bind(id).bind(error).execute(pool).await?;
    Ok(())
}

// ── Solana order ID mapping ───────────────────────────────────────────────────

pub async fn save_order_mapping(
    pool: &PgPool,
    in_memory_id: u64,
    on_chain_id: u64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO solana_order_map (in_memory_id, on_chain_id)
         VALUES ($1, $2)
         ON CONFLICT (in_memory_id) DO NOTHING",
    )
    .bind(in_memory_id as i64).bind(on_chain_id as i64)
    .execute(pool).await?;
    Ok(())
}

pub async fn get_order_mapping(
    pool: &PgPool,
    in_memory_id: u64,
) -> Result<Option<u64>, sqlx::Error> {
    let row = sqlx::query_as::<_, (i64,)>(
        "SELECT on_chain_id FROM solana_order_map WHERE in_memory_id = $1",
    )
    .bind(in_memory_id as i64)
    .fetch_optional(pool).await?;
    Ok(row.map(|(v,)| v as u64))
}
