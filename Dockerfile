FROM rust:1.88 AS builder

WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates libssl3 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/dollar-rates /usr/local/bin/dollar-rates

ENV PORT=10000
EXPOSE 10000

CMD ["dollar-rates"]
