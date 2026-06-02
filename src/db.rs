use sqlx::PgPool;

use crate::models::Trade;

/// Create tables on first run if they don't already exist.
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
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS trades (
            id            BIGSERIAL PRIMARY KEY,
            buy_order_id  BIGINT NOT NULL,
            sell_order_id BIGINT NOT NULL,
            price         BIGINT NOT NULL,
            quantity      BIGINT NOT NULL,
            executed_at   BIGINT NOT NULL
        )",
    )
    .execute(pool)
    .await?;

    Ok(())
}

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
    .bind(order_id as i64)
    .bind(side)
    .bind(order_type)
    .bind(price as i64)
    .bind(quantity as i64)
    .bind(timestamp as i64)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn save_trade(pool: &PgPool, trade: &Trade) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO trades (buy_order_id, sell_order_id, price, quantity, executed_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(trade.buy_order_id as i64)
    .bind(trade.sell_order_id as i64)
    .bind(trade.price as i64)
    .bind(trade.quantity as i64)
    .bind(trade.timestamp as i64)
    .execute(pool)
    .await?;
    Ok(())
}
