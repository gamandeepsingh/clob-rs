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

use crate::{db, engine::Engine, models::{OrderType, Side, Trade}};

// ── Shared state ─────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    /// std::sync::Mutex because matching is pure sync — never held across .await.
    pub engine: Arc<Mutex<Engine>>,
    pub db: PgPool,
    /// Broadcast channel: one sender, every WebSocket connection gets a receiver.
    pub tx: broadcast::Sender<WsEvent>,
}

// ── WebSocket event types ─────────────────────────────────────────────────────

/// Every connected WebSocket client receives these as JSON.
/// The `tag` field means the JSON looks like: {"type":"trade","data":{...}}
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

// ── HTTP request / response shapes ───────────────────────────────────────────

#[derive(Deserialize)]
pub struct OrderRequest {
    pub side: Side,
    pub order_type: OrderType,
    /// Required for limit orders, omit for market orders.
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

// ── Router ────────────────────────────────────────────────────────────────────

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/orders", post(submit_order))
        .route("/orderbook", get(get_orderbook))
        .route("/ws", get(ws_handler))
        .with_state(state)
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn submit_order(
    State(state): State<AppState>,
    Json(req): Json<OrderRequest>,
) -> Result<Json<OrderResponse>, StatusCode> {
    let price = req.price.unwrap_or(0);

    // Hold the mutex only for the synchronous matching loop, then drop it
    // before any async work so we don't block other requests.
    let (order_id, trades, best_bid, best_ask, spread) = {
        let mut engine = state
            .engine
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let (order_id, trades) = engine.submit(req.side, req.order_type, price, req.quantity);
        let best_bid = engine.best_bid();
        let best_ask = engine.best_ask();
        let spread = engine.spread();
        (order_id, trades, best_bid, best_ask, spread)
    }; // ← lock released here

    // Persist to DB (errors are logged but don't fail the response).
    let now = now_micros();
    if let Err(e) = db::save_order(&state.db, order_id, req.side.as_str(), req.order_type.as_str(), price, req.quantity, now).await {
        eprintln!("db save_order error: {e}");
    }
    for trade in &trades {
        if let Err(e) = db::save_trade(&state.db, trade).await {
            eprintln!("db save_trade error: {e}");
        }
    }

    // Broadcast to all WebSocket subscribers.
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
                        if socket.send(Message::Text(text)).await.is_err() {
                            return; // client disconnected
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                    // Lagged means the client was too slow and missed some events.
                    // We just continue — no need to disconnect.
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
