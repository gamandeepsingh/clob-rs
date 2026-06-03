mod api;
mod db;
mod engine;
mod models;
mod orderbook;
mod solana;
mod worker;

use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await
        .expect("failed to connect to PostgreSQL");

    db::init(&pool).await.expect("failed to create tables");

    let (tx, _) = broadcast::channel::<api::WsEvent>(256);

    let solana_client = build_solana_client();

    if let Some(ref sol) = solana_client {
        if let Err(e) = sol.ensure_market_initialized().await {
            eprintln!("[solana] market init failed: {e}");
        }
        // Spawn the outbox worker — drains solana_queue every 2 seconds.
        tokio::spawn(worker::run(pool.clone(), Arc::clone(sol)));
        println!("[solana] outbox worker started");
    }

    let state = api::AppState {
        engine: Arc::new(Mutex::new(engine::Engine::new())),
        db: pool,
        tx,
        solana: solana_client,
    };

    let addr = format!("0.0.0.0:{port}");
    let app = api::router(state);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("CLOB listening on http://{addr}");
    axum::serve(listener, app).await.unwrap();
}

fn build_solana_client() -> Option<Arc<solana::SolanaClient>> {
    let rpc_url    = env_or_warn("SOLANA_RPC_URL")?;
    let keypair    = env_or_warn("SOLANA_KEYPAIR_PATH")?;
    let program_id = pubkey_or_warn("SOLANA_PROGRAM_ID")?;
    let base_mint  = pubkey_or_warn("SOLANA_BASE_MINT")?;
    let quote_mint = pubkey_or_warn("SOLANA_QUOTE_MINT")?;

    match solana::SolanaClient::new(&rpc_url, &keypair, program_id, base_mint, quote_mint) {
        Ok(client) => {
            println!("[solana] client ready (program={program_id}, market={})", client.market);
            Some(Arc::new(client))
        }
        Err(e) => {
            eprintln!("[solana] init failed: {e}");
            None
        }
    }
}

fn env_or_warn(key: &str) -> Option<String> {
    match std::env::var(key) {
        Ok(v) => Some(v),
        Err(_) => { println!("[solana] {key} not set — on-chain recording disabled"); None }
    }
}

fn pubkey_or_warn(key: &str) -> Option<solana_sdk::pubkey::Pubkey> {
    match std::env::var(key) {
        Ok(v) => match v.parse() {
            Ok(pk) => Some(pk),
            Err(_) => { eprintln!("[solana] {key}={v:?} is not a valid pubkey"); None }
        },
        Err(_) => { println!("[solana] {key} not set — on-chain recording disabled"); None }
    }
}
