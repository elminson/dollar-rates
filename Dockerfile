FROM rust:1.88 AS builder

WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    chromium \
    fonts-liberation \
    libnss3 \
    libgbm1 \
    libasound2 \
    libatk-bridge2.0-0 \
    libatk1.0-0 \
    libcups2 \
    libdrm2 \
    libglib2.0-0 \
    libgtk-3-0 \
    libnspr4 \
    libx11-xcb1 \
    libxcomposite1 \
    libxdamage1 \
    libxrandr2 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/dollar-rates /usr/local/bin/dollar-rates

ENV PORT=10000
ENV CHROMIUM_PATH=/usr/bin/chromium
EXPOSE 10000

CMD ["dollar-rates"]
