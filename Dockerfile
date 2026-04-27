FROM rust:1.85 AS builder
WORKDIR /app
COPY . .
RUN cargo build --release -p moq-multicam-cli

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates ffmpeg \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/moq-multicam /usr/local/bin/
ENTRYPOINT ["moq-multicam"]
