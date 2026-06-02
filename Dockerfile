# ── build ────────────────────────────────────────────────────────────
FROM rust:1.85-slim AS builder

WORKDIR /app

# Cache dependencies separately from source so rebuilds are fast.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main(){}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Now copy real source and rebuild only what changed.
COPY src ./src
RUN touch src/main.rs && cargo build --release

# ── run ──────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

# ca-certificates is needed to verify SSL when connecting to cloud databases
# (e.g. Neon, Supabase). Not needed for a plain local Postgres.
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/clob-rs .

EXPOSE 3000

CMD ["./clob-rs"]
