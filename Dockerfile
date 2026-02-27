FROM rust:nightly as builder

WORKDIR /app

RUN apt-get update && apt-get install -y pkg-config libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock* ./
COPY src ./src
COPY config ./config

RUN cargo build --release

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/polymarket-hft-bot /usr/local/bin/polymarket-hft-bot
COPY config ./config

ENV RUST_LOG=info

CMD ["polymarket-hft-bot", "--config", "config/production.toml", "run"]

