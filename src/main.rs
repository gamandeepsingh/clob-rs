mod api;
mod db;
mod engine;
mod models;
mod orderbook;

use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok(); // loads .env if present, silently ignored if missing

    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set (e.g. postgres://user:pass@localhost/clob)");

    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string());

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .expect("failed to connect to PostgreSQL");

    db::init(&pool).await.expect("failed to create tables");

    // Channel capacity 256: if a slow WebSocket client falls more than 256
    // events behind it gets a Lagged error and skips ahead — it won't block others.
    let (tx, _) = broadcast::channel::<api::WsEvent>(256);

    let state = api::AppState {
        engine: Arc::new(Mutex::new(engine::Engine::new())),
        db: pool,
        tx,
    };

    let addr = format!("0.0.0.0:{port}");
    let app = api::router(state);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("CLOB listening on http://{addr}");
    axum::serve(listener, app).await.unwrap();
}
