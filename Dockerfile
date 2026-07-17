FROM rust:1.88-bookworm AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --locked --release

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 liveu

WORKDIR /app
COPY --from=builder /build/target/release/liveu_stats_bot /usr/local/bin/liveu_stats_bot

USER liveu
ENTRYPOINT ["liveu_stats_bot"]
