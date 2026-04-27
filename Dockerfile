# syntax=docker/dockerfile:1

# --- Build relay first (changes rarely, cached independently) ---
FROM rust:1.89-bookworm AS relay-builder
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/tmp/cargo-target \
    CARGO_TARGET_DIR=/tmp/cargo-target \
    cargo install moq-relay --version ^0.10 --root /usr/local

# --- Build app ---
FROM rust:1.89-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
    pkg-config cmake \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release -p moq-multicam-cli --features gstreamer \
    && cp target/release/moq-multicam /usr/local/bin/

# --- Runtime stage ---
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates ffmpeg \
    gstreamer1.0-tools gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-libav \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /usr/local/bin/moq-multicam /usr/local/bin/
ENTRYPOINT ["moq-multicam"]

# --- Relay stage ---
FROM debian:bookworm-slim AS relay
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=relay-builder /usr/local/bin/moq-relay /usr/local/bin/
ENTRYPOINT ["moq-relay"]

# --- Dev stage ---
FROM rust:1.89-bookworm AS dev
RUN apt-get update && apt-get install -y --no-install-recommends \
    libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
    gstreamer1.0-tools gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
    gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly gstreamer1.0-libav \
    pkg-config cmake ffmpeg \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
