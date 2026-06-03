# ── build ─────────────────────────────────────────────────────────────────────
FROM rust:1.85-slim AS builder

WORKDIR /app

# openssl-sys (pulled in by solana-client) needs these at compile time.
RUN apt-get update \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Cache dependencies separately from source so rebuilds are fast.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main(){}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Now copy real source and rebuild only what changed.
COPY src ./src
RUN touch src/main.rs && cargo build --release

# ── run ───────────────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

# libssl3       → runtime TLS for Solana RPC and cloud DB connections
# ca-certificates → verify SSL certs (Neon, Supabase, devnet RPC)
RUN apt-get update \
    && apt-get install -y --no-install-recommends libssl3 ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/clob-rs .

EXPOSE 3000

CMD ["./clob-rs"]
