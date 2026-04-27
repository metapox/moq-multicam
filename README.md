# moq-multicam

Low-latency multi-camera streaming over [MoQ (Media over QUIC)](https://moq.dev/).

Stream multiple cameras to browsers in real-time with sub-second latency, track-level subscribe/unsubscribe, and priority-based bandwidth adaptation.

> **Status**: Phase 0b complete — GStreamer + direct hang write, 1 Broadcast multi-camera

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

```bash
docker compose up
```

Open http://localhost:5173 — two test cameras streaming to the browser via GStreamer → MoQ → WebTransport.

### What's running

| Service | Description |
|---|---|
| `relay` | MoQ relay server (QUIC + WebTransport) |
| `publisher` | GStreamer → H.264 → hang → relay (2 cameras, 1 process) |
| `web` | Browser viewer (@moq/watch) |

## Architecture

```
GStreamer (capture/encode)
  → hang OrderedProducer (H.264 Annex B direct write)
  → 1 Broadcast "vehicle/truck-01"
    ├── Track "camera/front/video"
    └── Track "camera/rear/video"
  → relay (QUIC)
  → browser (WebTransport + WebCodecs)
```

See [docs/architecture.md](docs/architecture.md) for details.

### Crates

| Crate | Description |
|---|---|
| `moq-multicam-core` | Shared types: track naming, camera config, moq-lite wrapper |
| `moq-multicam-bridge` | Video source → MoQ publisher (GStreamer, ffmpeg, test source) |
| `moq-multicam-relay` | Relay server (placeholder, uses moq-relay directly) |
| `moq-multicam-cli` | CLI: `publish-fmp4`, `publish`, `subscribe` |

## CLI Usage

```bash
# Multi-camera with GStreamer (requires gstreamer feature)
moq-multicam publish-fmp4 --camera front --camera rear --source gstreamer --tls-disable-verify

# Multi-camera with ffmpeg (no GStreamer needed)
moq-multicam publish-fmp4 --camera front --camera rear --source ffmpeg --tls-disable-verify

# Single camera from stdin (backward compatible)
ffmpeg ... | moq-multicam publish-fmp4 --broadcast "vehicle/truck-01/camera/front" --tls-disable-verify

# Subscribe and log received data
moq-multicam subscribe --relay https://localhost:4443 --tls-disable-verify
```

## Manual Setup

### Prerequisites

- [Rust](https://rustup.rs/) (1.89+)
- [moq-relay](https://github.com/moq-dev/moq) (`cargo install moq-relay`)
- [Node.js](https://nodejs.org/) (for the browser viewer)
- GStreamer (for `--source gstreamer`), or ffmpeg (for `--source ffmpeg`)

### Build

```bash
# Without GStreamer
cargo build -p moq-multicam-cli

# With GStreamer
cargo build -p moq-multicam-cli --features gstreamer
```

### Run

```bash
# Terminal 1: relay
moq-relay --server-bind "[::]:4443" \
  --tls-generate localhost --tls-disable-verify \
  --auth-public "" --web-http-listen "[::]:4443"

# Terminal 2: publisher
./target/debug/moq-multicam publish-fmp4 \
  --camera front --camera rear --source ffmpeg --tls-disable-verify

# Terminal 3: browser
cd web && npm install && npm run dev
```

Open http://localhost:5173 in Chrome.

## Tech Stack

- **Rust** + tokio
- **MoQ**: [moq-lite](https://crates.io/crates/moq-lite) 0.15
- **QUIC**: quinn (via moq-native)
- **Media**: [hang](https://crates.io/crates/hang) 0.15 (Container::Legacy, avc3)
- **Video**: H.264 via GStreamer (x264enc) or ffmpeg
- **Browser**: @moq/watch + WebTransport + WebCodecs

## Roadmap

- [x] **Phase 0a**: E2E pipeline — test source → relay → browser
- [x] **Phase 0b**: GStreamer, 1-process multi-camera, direct hang write, error recovery, Docker
- [ ] **Phase 1**: Adaptive bitrate, AI plugin system, multi-camera viewer UI
- [ ] **Phase 2**: Autonomous driving teleoperation showcase

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.
