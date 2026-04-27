# moq-multicam

Low-latency multi-camera streaming over [MoQ (Media over QUIC)](https://moq.dev/).

Stream multiple cameras to browsers in real-time with sub-second latency, track-level subscribe/unsubscribe, and priority-based bandwidth adaptation.

> **Status**: Phase 0a — Foundation (E2E pipeline working, browser display confirmed)

## Why

Existing solutions don't handle **multi-camera × WAN × scalable** well:

| Solution | Limitation |
|---|---|
| GStreamer + RTSP | No CDN scaling, poor WAN support |
| WebRTC (LiveKit) | SFU scaling limits, heavy on edge devices |
| ROS2 DDS | NAT/firewall issues over WAN, no video compression standard |
| AWS Kinesis Video | Vendor lock-in, 1-5s latency |

MoQ provides QUIC connection migration (mobile handover resilience), relay-based fan-out (CDN-like scaling), and track-level pub/sub (subscribe only to cameras you need).

## Quick Start

### Option A: Docker Compose (easiest)

```bash
docker compose up
```

Open http://localhost:5173 — two test cameras streaming to the browser.

### Option B: Manual Setup

#### Prerequisites

- [Rust](https://rustup.rs/) (1.85+)
- [ffmpeg](https://ffmpeg.org/) (`brew install ffmpeg` / `apt install ffmpeg`)
- [moq-relay](https://github.com/moq-dev/moq) (`cargo install moq-relay`)
- [Node.js](https://nodejs.org/) (for the browser viewer)

### 1. Build

```bash
git clone https://github.com/metapox/moq-multicam.git
cd moq-multicam
cargo build -p moq-multicam-cli
```

### 2. Start the relay

```bash
moq-relay --server-bind "[::]:4443" \
  --tls-generate localhost \
  --tls-disable-verify \
  --auth-public "" \
  --web-http-listen "[::]:4443"
```

### 3. Publish a test camera

```bash
ffmpeg -hide_banner -v quiet \
  -f lavfi -i "testsrc=size=640x480:rate=30" \
  -c:v libx264 -preset ultrafast -tune zerolatency -g 30 \
  -f mp4 -movflags "cmaf+separate_moof+delay_moov+skip_trailer+frag_every_frame" - \
  | ./target/debug/moq-multicam publish-fmp4 \
    --broadcast "vehicle/truck-01/camera/front" \
    --tls-disable-verify
```

### 4. View in browser

```bash
cd web
npm install
npm run dev
```

Open http://localhost:5173 in Chrome.

## Architecture

```
Cameras → [moq-multicam-bridge] → WAN (MoQ/QUIC) → [moq-multicam-relay] → Browser (multi-view)
```

See [docs/architecture.md](docs/architecture.md) for the full architecture document.

### Crates

| Crate | Description |
|---|---|
| `moq-multicam-core` | Shared types: track naming, camera config, moq-lite wrapper |
| `moq-multicam-bridge` | Video source → MoQ publisher (test source, GStreamer in Phase 0b) |
| `moq-multicam-relay` | Relay server with multi-camera features (wraps moq-relay) |
| `moq-multicam-cli` | CLI tool (`moq-multicam publish`, `subscribe`, `publish-fmp4`) |

## CLI Usage

```bash
# Publish dummy test data (no ffmpeg needed, no video in browser)
moq-multicam publish --relay https://localhost:4443 --tls-disable-verify

# Publish fMP4 from ffmpeg (video visible in browser)
ffmpeg ... | moq-multicam publish-fmp4 --broadcast "vehicle/truck-01/camera/front" --tls-disable-verify

# Subscribe and log received data
moq-multicam subscribe --relay https://localhost:4443 --tls-disable-verify
```

## Examples

| Example | Description |
|---|---|
| `minimal-pubsub` | In-memory pub/sub, no network |
| `multicam-test-source` | 2 cameras with test source, in-memory |
| `quic-publish` | Publish to relay over QUIC |
| `quic-subscribe` | Subscribe from relay over QUIC |

```bash
cargo run -p minimal-pubsub
cargo run -p multicam-test-source
```

## Tech Stack

- **Rust** + tokio
- **MoQ**: [moq-lite](https://crates.io/crates/moq-lite) 0.15 (moq-dev/moq)
- **QUIC**: quinn (via moq-native)
- **Media**: hang 0.15 + moq-mux 0.3 (fMP4/CMAF)
- **Video**: H.264 (GStreamer in Phase 0b)
- **Browser**: @moq/watch + WebTransport + WebCodecs

## Roadmap

- [x] **Phase 0a**: E2E pipeline with test source → relay → browser
- [ ] **Phase 0b**: GStreamer integration, 1-process multi-camera
- [ ] **Phase 1**: Adaptive bitrate, AI plugin system
- [ ] **Phase 2**: Autonomous driving teleoperation showcase

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.
