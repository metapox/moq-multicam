# moq-multicam

Low-latency multi-camera streaming over [MoQ (Media over QUIC)](https://moq.dev/).

A reference implementation for priority-based multi-camera streaming — built on QUIC stream priorities to keep the most important camera flowing when bandwidth is constrained.

> **Status**: Experimental / personal project. MoQ itself is still an [IETF draft](https://datatracker.ietf.org/doc/draft-ietf-moq-transport/) and the ecosystem is evolving rapidly. This project depends on a [fork of moq-dev/moq](https://github.com/metapox/moq) with patches for [SUBSCRIBE_UPDATE JS API (#1363)](https://github.com/moq-dev/moq/issues/1363) and [PriorityQueue in-flight update (#1370)](https://github.com/moq-dev/moq/issues/1370) that are not yet upstream. **Not production-ready** — this is a solo exploration of what MoQ can do for multi-camera streaming. Feedback and ideas welcome.

## What This Project Does

moq-multicam is a **multi-camera streaming framework** that demonstrates how MoQ's track-level pub/sub and QUIC stream priorities can solve a real problem: keeping the right camera visible when bandwidth drops.

**Core idea**: Each camera is an independent MoQ broadcast. Each subscription has a priority. When the network is constrained, QUIC delivers high-priority streams first — the focused camera keeps flowing while background cameras gracefully degrade.

**Showcase**: Autonomous vehicle teleoperation, where an operator remotely monitors multiple dashcams over LTE/5G and needs instant camera switching without video interruption.

### Why MoQ (and not WebRTC/RTSP/ROS2)?

| Capability | WebRTC | RTSP | ROS2 DDS | **MoQ** |
|---|---|---|---|---|
| WAN / firewall traversal | ✅ | ❌ | ❌ | ✅ (WebTransport) |
| CDN-like fan-out | ❌ (SFU limits) | ❌ | ❌ | ✅ (relay) |
| Per-stream priority | ❌ | ❌ | Partial | ✅ (QUIC native) |
| Subscribe individual cameras | ❌ | ✅ | ✅ | ✅ (track-level) |
| Connection migration (handover) | ❌ | ❌ | ❌ | ✅ (QUIC) |
| Browser native | ✅ | ❌ | ❌ | ✅ (WebTransport + WebCodecs) |

### What's Not Here (Yet)

- **No production relay** — uses moq-dev's reference relay, which is single-node
- **No auth / encryption beyond TLS** — no viewer authentication or DRM
- **No adaptive bitrate** — priority-based degradation only, no ABR ladder
- **Software encode only** — openh264 (no GPU / hardware encode)
- **Upstream patches pending** — depends on moq-dev/moq fork for SUBSCRIBE_UPDATE

## Demo: Priority-Based Bandwidth Adaptation

![Bandwidth adaptation demo](docs/demo-bandwidth-adaptation.gif)

**The problem**: In teleoperation, an operator monitors multiple cameras over a constrained network (LTE/5G). When bandwidth drops, all cameras degrade equally — the operator loses situational awareness on the camera that matters most.

**The solution**: Each camera subscription has a priority. The focused camera gets priority 0 (highest); background cameras get priority 200. When bandwidth is limited, QUIC's stream priority ensures the focused camera keeps receiving frames while background cameras gracefully degrade.

**What the demo shows**:
1. Two cameras streaming (front/rear dashcam footage)
2. Bandwidth throttled to 2 Mbps via `tc` — the red banner appears
3. Focus camera maintains ~16 FPS / 1.6 Mbps; background camera drops
4. Operator clicks to switch focus — the new focus camera **instantly** recovers without re-subscribing (via `SUBSCRIBE_UPDATE`)
5. Bandwidth restored — all cameras recover

This works because moq-multicam uses [SUBSCRIBE_UPDATE](https://github.com/moq-dev/moq/issues/1363) to change priority without closing the subscription, and a [patched PriorityQueue](https://github.com/moq-dev/moq/issues/1370) that propagates priority changes to in-flight QUIC streams.

## Features

- **8 cameras simultaneous** — one process, one QUIC connection
- **Broadcast per camera** — independent subscribe/unsubscribe per camera
- **Subscriber priority** — focus camera P0, background cameras P200; relay prioritizes under bandwidth pressure
- **Multi-rendition** — HQ 640×480 + LQ 320×240 per camera, switched on focus change
- **Teleoperation control channel** — browser → vehicle bidirectional commands over the same QUIC connection
- **E2E latency measurement** — pixel-embedded timestamps for glass-to-glass latency tracking
- **Real-time stats overlay** — RTT, recv bandwidth, per-camera FPS/bitrate, decode queue, frame drops
- **QUIC auto-reconnect** — survives network transitions (5G↔LTE)
- **Docker Compose** — one command to run everything

## Quick Start

```bash
docker compose up
```

Open http://localhost:5173 — 8 test cameras streaming via openh264 → MoQ → WebTransport → WebCodecs.

Click a thumbnail to switch focus. The focused camera gets high quality (640×480) + priority 0; others get low quality (320×240) + priority 200.

### What's running

| Service | Description |
|---|---|
| `relay` | [moq-relay](https://github.com/moq-dev/moq) server (QUIC + WebTransport) |
| `publisher` | openh264 → H.264 → hang → relay (8 cameras, 1 process) |
| `web` | Browser viewer (Vite dev server, @moq/lite + WebCodecs) |

## Architecture

```
openh264 (8 cameras × 2 renditions = 16 pipelines)
  → hang OrderedProducer (H.264 Annex B direct write)
  → Broadcast per camera:
      vehicle/truck-01/camera/front  → video, video-low, catalog.json
      vehicle/truck-01/camera/rear   → video, video-low, catalog.json
      ...
      vehicle/truck-01/meta          → manifest (camera discovery)
      vehicle/truck-01/control       → command (operator → vehicle)
  → relay (QUIC)
  → browser (WebTransport + @moq/lite + WebCodecs + Canvas 2D)
```

See [docs/architecture.md](docs/architecture.md) for details.

### Crates

| Crate | Description |
|---|---|
| `moq-multicam-core` | Shared types: track naming, camera config, moq-lite wrapper |
| `moq-multicam-bridge` | Video source → MoQ publish (openh264, V4L2, ffmpeg, test source) |
| `moq-multicam-cli` | CLI: `publish`, `subscribe` |

## CLI Usage

```bash
# Multi-camera with openh264 (8 cameras, default source)
moq-multicam publish \
  --camera front --camera rear --camera left --camera right \
  --camera front-left --camera front-right --camera rear-left --camera rear-right \
  --source openh264 --tls-disable-verify

# Multi-camera with ffmpeg
moq-multicam publish --camera front --camera rear --source ffmpeg --tls-disable-verify

# Single camera from stdin (backward compatible)
ffmpeg ... | moq-multicam publish --broadcast "vehicle/truck-01/camera/front" --tls-disable-verify
```

## Development

For fast iteration, use the dev compose file. Source is bind-mounted so changes don't require image rebuilds — incremental builds take ~7 seconds.

```bash
# One-time: build the relay image
cd ../moq-fork
docker build -t moq-relay:local -f Dockerfile.relay .

# Start dev environment
cd ../moq-multicam  # (or the worktree)
docker compose -f compose.dev.yml up -d

# Build (incremental, debug)
docker compose -f compose.dev.yml exec publisher-dev \
  cargo build -p moq-multicam-cli --features openh264

# Run
docker compose -f compose.dev.yml exec publisher-dev \
  cargo run -p moq-multicam-cli --features openh264 -- \
  publish --relay https://relay:4443 --camera front --camera rear \
  --source openh264 --tls-disable-verify

# Run tests
docker compose -f compose.dev.yml exec publisher-dev \
  cargo test --workspace --features openh264
```

The viewer runs at http://localhost:5173 (same as production compose).

## Manual Setup

### Prerequisites

- [Rust](https://rustup.rs/) (1.85+)
- [moq-relay](https://github.com/moq-dev/moq) (`cargo install moq-relay`)
- [Node.js](https://nodejs.org/) (for the browser viewer)
- ffmpeg (for `--source ffmpeg`)

### Build

```bash
# With openh264 (default)
cargo build -p moq-multicam-cli --features openh264

# With V4L2 camera support (Linux only)
cargo build -p moq-multicam-cli --features v4l
```

### Run

```bash
# Terminal 1: relay
moq-relay --server-bind "[::]:4443" \
  --tls-generate localhost --tls-disable-verify \
  --auth-public "" --web-http-listen "[::]:4443"

# Terminal 2: publisher
./target/debug/moq-multicam publish \
  --camera front --camera rear --source ffmpeg --tls-disable-verify

# Terminal 3: browser
cd web && npm install && npm run dev
```

Open http://localhost:5173 in Chrome.

## Tech Stack

| Layer | Choice |
|---|---|
| QUIC | quinn (via moq-native 0.13) |
| MoQ protocol | moq-lite 0.15 |
| Media container | hang 0.15 (Container::Legacy) |
| Video encode | H.264 via openh264 (+ V4L2 for real cameras) |
| Browser | @moq/lite + @moq/hang + WebCodecs VideoDecoder + Canvas 2D |
| Async runtime | tokio |

## Roadmap

- [x] **Phase 0a**: E2E pipeline — test source → relay → browser
- [x] **Phase 0b**: Multi-camera, direct hang write, error recovery, Docker
- [x] **Phase 1**: Broadcast per camera, 8 cameras, rendition switching, subscriber priority, stats overlay
- [ ] **Phase 1**: USB camera support (requires real hardware)
- [x] **Phase 2**: E2E latency measurement, teleoperation control channel
- [ ] **Phase 2**: Bandwidth adaptation demo, AI inference plugin, demo video, CI, tutorial

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.
