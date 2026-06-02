# clob-rs

![Rust](https://img.shields.io/badge/Rust-1.85+-orange?logo=rust&logoColor=white)
![Tokio](https://img.shields.io/badge/Async-Tokio-blue?logo=tokio&logoColor=white)
![Axum](https://img.shields.io/badge/Web-Axum-blueviolet)
![PostgreSQL](https://img.shields.io/badge/DB-PostgreSQL-4169E1?logo=postgresql&logoColor=white)
![WebSocket](https://img.shields.io/badge/Realtime-WebSocket-green)
![License](https://img.shields.io/badge/License-MIT-brightgreen)

A high-performance Central Limit Order Book (CLOB) exchange engine written in Rust. Supports limit and market orders, price-time priority matching, real-time WebSocket streaming, and PostgreSQL persistence, all built on Tokio and Axum.

## What is a CLOB?

A Central Limit Order Book is the core matching engine behind every exchange. It maintains two sorted queues of orders:

```
SELL side (asks) — people willing to sell
  $102.00 — 3 units
  $100.00 — 5 units
────────── spread ──────────
  $98.00  — 8 units
  $95.00  — 12 units
BUY side (bids) — people willing to buy
```

When a new order arrives, the engine checks if prices cross. If they do, a trade is produced. Orders that don't match immediately rest in the book waiting for a counterparty.

## Features

- **Limit orders** — rest in the book at a specified price until matched
- **Market orders** — fill immediately at the best available price
- **Price-time priority** — best price wins; ties go to earliest arrival
- **Partial fills** — large orders sweep multiple price levels, generating one trade per level
- **Real-time WebSocket** — every trade and book update is broadcast instantly to all connected clients
- **PostgreSQL persistence** — all orders and trades are saved; tables are created automatically on first run
- **Zero-float arithmetic** — prices are stored as integers (minor units, e.g. cents) to avoid floating-point errors

## Architecture

```
POST /orders
     │
     ▼
 submit_order handler
     ├── lock engine → match orders → unlock   (sync, microseconds)
     ├── save order + trades to PostgreSQL      (async)
     └── broadcast WsEvent to all WebSocket clients
                                                     │
                                              each connected client
                                              receives via ws://
```

**Key design decisions:**
- `BTreeMap` over `HashMap` for the order book — keeps price levels sorted so best bid/ask is always O(log n)
- `VecDeque` per price level — O(1) pop from the front for time-priority FIFO
- `std::sync::Mutex` (not async) for the engine — matching is pure CPU work, never held across `.await`
- `tokio::sync::broadcast` for WebSocket fan-out — one send, every subscriber receives

## Project Structure

```
src/
  main.rs       — Tokio runtime, DB pool, server startup
  models.rs     — Order, Trade, Side, OrderType, OrderStatus
  orderbook.rs  — Two sorted BTreeMaps, no matching logic
  engine.rs     — Matching loop, produces Vec<Trade>
  api.rs        — Axum routes, AppState, WebSocket handler
  db.rs         — PostgreSQL queries (init tables, save order/trade)
```

## API Reference

### `POST /orders`

Submit a new order.

**Request body:**
```json
{
  "side": "buy" | "sell",
  "order_type": "limit" | "market",
  "price": 10000,
  "quantity": 5
}
```

> `price` is in minor units (e.g. `10000` = $100.00). Omit `price` for market orders.

**Response:**
```json
{
  "order_id": 3,
  "trades": [
    {
      "buy_order_id": 3,
      "sell_order_id": 1,
      "price": 10000,
      "quantity": 5,
      "timestamp": 1748822400000000
    }
  ]
}
```

An empty `trades` array means the order rested in the book without matching.

---

### `GET /orderbook`

Returns a full snapshot of the current book.

**Response:**
```json
{
  "bids": [
    { "price": 9800, "quantity": 10 },
    { "price": 9500, "quantity": 4 }
  ],
  "asks": [
    { "price": 10200, "quantity": 1 },
    { "price": 10500, "quantity": 8 }
  ],
  "spread": 400
}
```

Bids are sorted highest price first. Asks are sorted lowest price first. `spread` is `null` if either side is empty.

---

### `GET /ws` — WebSocket

Connect and receive real-time events as JSON.

**Trade event** (emitted for every match):
```json
{
  "type": "trade",
  "data": {
    "buy_order_id": 3,
    "sell_order_id": 1,
    "price": 10000,
    "quantity": 5,
    "timestamp": 1748822400000000
  }
}
```

**Book update event** (emitted after every order submission):
```json
{
  "type": "book_update",
  "data": {
    "best_bid": 9800,
    "best_ask": 10200,
    "spread": 400
  }
}
```

## Database Schema

Tables are created automatically on startup — no migrations needed.

```sql
CREATE TABLE orders (
    id          BIGINT PRIMARY KEY,
    side        TEXT   NOT NULL,   -- 'buy' | 'sell'
    order_type  TEXT   NOT NULL,   -- 'limit' | 'market'
    status      TEXT   NOT NULL,   -- 'open' | 'partially_filled' | 'filled' | 'cancelled'
    price       BIGINT NOT NULL,
    quantity    BIGINT NOT NULL,
    remaining   BIGINT NOT NULL,
    created_at  BIGINT NOT NULL    -- Unix microseconds
);

CREATE TABLE trades (
    id            BIGSERIAL PRIMARY KEY,
    buy_order_id  BIGINT NOT NULL,
    sell_order_id BIGINT NOT NULL,
    price         BIGINT NOT NULL,
    quantity      BIGINT NOT NULL,
    executed_at   BIGINT NOT NULL  -- Unix microseconds
);
```

## Environment Variables

| Variable | Required | Default | Description |
|---|---|---|---|
| `DATABASE_URL` | Yes | — | PostgreSQL connection string |
| `PORT` | No | `3000` | Port the HTTP server listens on |

## Running Locally

**Prerequisites:** Rust 1.85+, PostgreSQL

```bash
# 1. Clone the repo
git clone https://github.com/your-username/clob-rs
cd clob-rs

# 2. Create a .env file
cp .env.example .env
# Edit .env with your DATABASE_URL

# 3. Run
cargo run
```

The server starts at `http://localhost:3000`. Tables are created on first run.

## Docker

### With Docker Compose (recommended — includes Postgres)

```bash
docker compose up --build
```

This starts a PostgreSQL container and the CLOB engine together. Data is persisted in a named volume (`postgres_data`).

To stop and remove everything including the volume:
```bash
docker compose down -v
```

### Using a cloud database (Neon, Supabase, etc.)

Build the image and pass your own `DATABASE_URL`:

```bash
docker build -t clob-rs .

docker run -p 3000:3000 \
  -e DATABASE_URL="postgres://user:pass@your-host/dbname?sslmode=require" \
  -e PORT=3000 \
  clob-rs
```

## Testing with the HTTP client

Open `requests.http` in VS Code with the [REST Client](https://marketplace.visualstudio.com/items?itemName=humao.rest-client) extension and click **Send Request** above each block.

For WebSocket testing:
```bash
# Install wscat
npm install -g wscat

# Connect
wscat -c ws://localhost:3000/ws
```

Then submit orders via the HTTP client and watch trades stream in the WebSocket terminal.
