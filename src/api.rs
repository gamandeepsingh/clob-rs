use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::broadcast;

use crate::{db, engine::Engine, models::{OrderType, Side, Trade}, solana::SolanaClient};

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<Mutex<Engine>>,
    pub db: PgPool,
    pub tx: broadcast::Sender<WsEvent>,
    pub solana: Option<Arc<SolanaClient>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum WsEvent {
    Trade(Trade),
    BookUpdate(BookSummary),
}

#[derive(Debug, Clone, Serialize)]
pub struct BookSummary {
    pub best_bid: Option<u64>,
    pub best_ask: Option<u64>,
    pub spread: Option<u64>,
}

#[derive(Deserialize)]
pub struct OrderRequest {
    pub side: Side,
    pub order_type: OrderType,
    pub price: Option<u64>,
    pub quantity: u64,
}

#[derive(Serialize)]
pub struct OrderResponse {
    pub order_id: u64,
    pub trades: Vec<Trade>,
}

#[derive(Serialize)]
pub struct PriceLevel {
    pub price: u64,
    pub quantity: u64,
}

#[derive(Serialize)]
pub struct OrderBookSnapshot {
    pub bids: Vec<PriceLevel>,
    pub asks: Vec<PriceLevel>,
    pub spread: Option<u64>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/orders", post(submit_order))
        .route("/orderbook", get(get_orderbook))
        .route("/ws", get(ws_handler))
        .with_state(state)
}

async fn submit_order(
    State(state): State<AppState>,
    Json(req): Json<OrderRequest>,
) -> Result<Json<OrderResponse>, StatusCode> {
    let price = req.price.unwrap_or(0);

    let (order_id, trades, best_bid, best_ask, spread) = {
        let mut engine = state.engine.lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let (order_id, trades) = engine.submit(req.side, req.order_type, price, req.quantity);
        let best_bid = engine.best_bid();
        let best_ask = engine.best_ask();
        let spread = engine.spread();
        (order_id, trades, best_bid, best_ask, spread)
    };

    let now = now_micros();

    // DB writes are fail-fast: if either fails we return 500 before touching the outbox.
    db::save_order(&state.db, order_id, req.side.as_str(), req.order_type.as_str(), price, req.quantity, now)
        .await.map_err(|e| { eprintln!("db save_order: {e}"); StatusCode::INTERNAL_SERVER_ERROR })?;

    for trade in &trades {
        db::save_trade(&state.db, trade)
            .await.map_err(|e| { eprintln!("db save_trade: {e}"); StatusCode::INTERNAL_SERVER_ERROR })?;
    }

    // If Solana is configured, enqueue events to the outbox.
    // The background worker picks these up and retries on failure — no data is lost.
    if state.solana.is_some() {
        let side_byte = req.side as u8;
        let ot_byte   = req.order_type as u8;

        let place_payload = serde_json::json!({
            "in_memory_id": order_id,
            "side": side_byte,
            "order_type": ot_byte,
            "price": price,
            "quantity": req.quantity,
        }).to_string();

        if let Err(e) = db::enqueue(&state.db, "place_order", &place_payload, now).await {
            eprintln!("outbox enqueue place_order: {e}");
        }

        for trade in &trades {
            let settle_payload = serde_json::json!({
                "buy_in_memory_id":  trade.buy_order_id,
                "sell_in_memory_id": trade.sell_order_id,
                "fill_qty":          trade.quantity,
                "price":             trade.price,
            }).to_string();

            if let Err(e) = db::enqueue(&state.db, "settle_match", &settle_payload, now).await {
                eprintln!("outbox enqueue settle_match: {e}");
            }
        }
    }

    for trade in &trades {
        let _ = state.tx.send(WsEvent::Trade(trade.clone()));
    }
    let _ = state.tx.send(WsEvent::BookUpdate(BookSummary { best_bid, best_ask, spread }));

    Ok(Json(OrderResponse { order_id, trades }))
}

async fn get_orderbook(State(state): State<AppState>) -> Json<OrderBookSnapshot> {
    let (bids, asks, spread) = {
        let engine = state.engine.lock().unwrap();
        (engine.bids_snapshot(), engine.asks_snapshot(), engine.spread())
    };

    let to_levels = |v: Vec<(u64, u64)>| {
        v.into_iter()
            .map(|(price, quantity)| PriceLevel { price, quantity })
            .collect()
    };

    Json(OrderBookSnapshot {
        bids: to_levels(bids),
        asks: to_levels(asks),
        spread,
    })
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    let mut rx = state.tx.subscribe();

    loop {
        tokio::select! {
            result = rx.recv() => {
                match result {
                    Ok(event) => {
                        let text = serde_json::to_string(&event).unwrap_or_default();
                        if socket.send(Message::Text(text)).await.is_err() { return; }
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => return,
                    _ => {}
                }
            }
        }
    }
}

fn now_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_micros() as u64
}
