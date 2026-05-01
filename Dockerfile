# syntax=docker/dockerfile:1

# --- Build app (openh264 only) ---
FROM rust:1.89-bookworm AS builder-openh264

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config cmake \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY moq-fork /moq-fork
COPY moq-multicam .
# Fix paths: worktree uses ../../moq-fork, Docker has /moq-fork
RUN sed -i 's|path = "../../moq-fork/|path = "/moq-fork/|g' Cargo.toml
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target-openh264,id=target-openh264-fork \
    CARGO_TARGET_DIR=/app/target-openh264 \
    cargo build --release -p moq-multicam-cli --features openh264 \
    && cp /app/target-openh264/release/moq-multicam /usr/local/bin/

# --- Runtime stage (openh264, lightweight) ---
FROM debian:bookworm-slim AS runtime-openh264
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates iproute2 ffmpeg \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder-openh264 /usr/local/bin/moq-multicam /usr/local/bin/
ENTRYPOINT ["moq-multicam"]
