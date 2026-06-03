# clob-rs

![Rust](https://img.shields.io/badge/Rust-1.85+-orange?logo=rust&logoColor=white)
![Tokio](https://img.shields.io/badge/Async-Tokio-blue?logo=tokio&logoColor=white)
![Axum](https://img.shields.io/badge/Web-Axum-blueviolet)
![PostgreSQL](https://img.shields.io/badge/DB-PostgreSQL-4169E1?logo=postgresql&logoColor=white)
![Solana](https://img.shields.io/badge/Onchain-Solana-9945FF?logo=solana&logoColor=white)
![WebSocket](https://img.shields.io/badge/Realtime-WebSocket-green)
![License](https://img.shields.io/badge/License-MIT-brightgreen)

A high-performance Central Limit Order Book (CLOB) exchange engine written in Rust. Supports limit and market orders, price-time priority matching, real-time WebSocket streaming, PostgreSQL persistence, and optional on-chain recording via a Solana Anchor program — all built on Tokio and Axum.

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
- **On-chain recording (optional)** — each order and fill is recorded on Solana via an Anchor program; trades emit immutable `TradeEvent` logs queryable via RPC
- **Zero-float arithmetic** — prices are stored as integers (minor units, e.g. cents) to avoid floating-point errors

## Architecture

```
POST /orders
     │
     ▼
 submit_order handler
     ├── lock engine → match orders → unlock        (sync, microseconds)
     ├── save order + trades to PostgreSQL           (async)
     ├── solana.place_order()  → Order PDA on-chain (async, optional)
     ├── solana.settle_match() → fill recorded,
     │                           TradeEvent emitted  (async, optional)
     └── broadcast WsEvent to all WebSocket clients
                                                          │
                                                   each connected client
                                                   receives via ws://
```

**Key design decisions:**
- `BTreeMap` over `HashMap` for the order book — keeps price levels sorted so best bid/ask is O(log n)
- `VecDeque` per price level — O(1) pop from front for time-priority FIFO
- `std::sync::Mutex` (not async) for the engine — matching is pure CPU work, never held across `.await`
- `tokio::sync::broadcast` for WebSocket fan-out — one send, every subscriber receives
- Anchor PDA per order — address is deterministically derived from `[b"order", market, order_id]`, no index needed
- `emit!` for trade events — stored in transaction log, costs no rent

## Project Structure

```
clob-rs/
  src/
    main.rs       — Tokio runtime, DB pool, Solana client, server startup
    models.rs     — Order, Trade, Side, OrderType, OrderStatus
    orderbook.rs  — Two sorted BTreeMaps, no matching logic
    engine.rs     — Matching loop, produces Vec<Trade>
    api.rs        — Axum routes, AppState, WebSocket handler
    db.rs         — PostgreSQL queries (init tables, save order/trade)
    solana.rs     — Solana client (place_order, settle_match instructions)

  clob-program/
    programs/clob-program/src/lib.rs — Anchor program (on-chain state + instructions)
```

## API Reference

### `POST /orders`

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

**Response:**
```json
{
  "bids": [{ "price": 9800, "quantity": 10 }],
  "asks": [{ "price": 10200, "quantity": 1 }],
  "spread": 400
}
```

Bids sorted highest-first. Asks sorted lowest-first. `spread` is `null` if either side is empty.

---

### `GET /ws` — WebSocket

**Trade event:**
```json
{ "type": "trade", "data": { "buy_order_id": 3, "sell_order_id": 1, "price": 10000, "quantity": 5, "timestamp": 1748822400000000 } }
```

**Book update event:**
```json
{ "type": "book_update", "data": { "best_bid": 9800, "best_ask": 10200, "spread": 400 } }
```

## On-chain Program (Solana)

The Anchor program lives in `clob-program/`. It exposes four instructions:

| Instruction | What it does |
|---|---|
| `initialize_market` | Creates the Market PDA for a trading pair (run once) |
| `place_order` | Creates an Order PDA; increments `market.order_count` |
| `cancel_order` | Closes the Order PDA; rent returned to trader |
| `settle_match` | Updates remaining on both orders, emits `TradeEvent`, closes filled orders |

### Token Mint Addresses

To configure a market you need the mint addresses of the two tokens:

| Token | Devnet | Mainnet |
|---|---|---|
| Wrapped SOL | `So11111111111111111111111111111111111111112` | same |
| USDC | `4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU` | `EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v` |

Find any token's mint on [solscan.io](https://solscan.io) or create a test token with `spl-token create-token`.

### Deploying the program

```bash
cd clob-program

# Configure Solana CLI for devnet
solana config set --url https://api.devnet.solana.com

# Fund your wallet
solana airdrop 2

# Build and deploy
anchor build
anchor deploy

# The Program ID is printed — copy it to SOLANA_PROGRAM_ID in .env
```

### Initializing the market (run once after deploy)

```bash
# From clob-program/
anchor run initialize  # or write a small ts script using the IDL
```

Or call it via the Anchor TypeScript client using the IDL in `clob-program/target/idl/`.

## Database Schema

Tables are created automatically on startup.

```sql
CREATE TABLE orders (
    id          BIGINT PRIMARY KEY,
    side        TEXT   NOT NULL,
    order_type  TEXT   NOT NULL,
    status      TEXT   NOT NULL,
    price       BIGINT NOT NULL,
    quantity    BIGINT NOT NULL,
    remaining   BIGINT NOT NULL,
    created_at  BIGINT NOT NULL
);

CREATE TABLE trades (
    id            BIGSERIAL PRIMARY KEY,
    buy_order_id  BIGINT NOT NULL,
    sell_order_id BIGINT NOT NULL,
    price         BIGINT NOT NULL,
    quantity      BIGINT NOT NULL,
    executed_at   BIGINT NOT NULL
);
```

## Environment Variables

| Variable | Required | Default | Description |
|---|---|---|---|
| `DATABASE_URL` | Yes | — | PostgreSQL connection string |
| `PORT` | No | `3000` | HTTP server port |
| `SOLANA_RPC_URL` | No | — | Solana RPC endpoint (enables on-chain recording) |
| `SOLANA_KEYPAIR_PATH` | No | — | Path to authority keypair JSON file |
| `SOLANA_PROGRAM_ID` | No | — | Deployed program address |
| `SOLANA_BASE_MINT` | No | — | Base token mint address (e.g. Wrapped SOL) |
| `SOLANA_QUOTE_MINT` | No | — | Quote token mint address (e.g. USDC) |

All five Solana variables must be set together. If any is missing, on-chain recording is skipped silently.

## Running Locally

**Prerequisites:** Rust 1.85+, PostgreSQL

```bash
# 1. Clone
git clone https://github.com/your-username/clob-rs
cd clob-rs

# 2. Set up env
cp .env.example .env
# Edit .env — DATABASE_URL is required; Solana vars are optional

# 3. Run
cargo run
```

## Docker

### With Docker Compose (recommended — includes Postgres)

```bash
docker compose up --build
```

Data is persisted in a named volume (`postgres_data`). To tear down completely:
```bash
docker compose down -v
```

### With Solana enabled in Docker

Uncomment the Solana env vars in `docker-compose.yml` and mount your keypair:

```yaml
environment:
  SOLANA_RPC_URL: https://api.devnet.solana.com
  SOLANA_KEYPAIR_PATH: /app/keypair.json
  SOLANA_PROGRAM_ID: <your-program-id>
  SOLANA_BASE_MINT: So11111111111111111111111111111111111111112
  SOLANA_QUOTE_MINT: 4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU
volumes:
  - ~/.config/solana/id.json:/app/keypair.json:ro
```

### Using a cloud database

```bash
docker build -t clob-rs .

docker run -p 3000:3000 \
  -e DATABASE_URL="postgres://user:pass@your-host/dbname?sslmode=require" \
  clob-rs
```

## Testing

Open `requests.http` in VS Code with the [REST Client](https://marketplace.visualstudio.com/items?itemName=humao.rest-client) extension.

For WebSocket:
```bash
npm install -g wscat
wscat -c ws://localhost:3000/ws
```
