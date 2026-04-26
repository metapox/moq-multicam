# moq-multicam

Low-latency multi-camera streaming over [MoQ (Media over QUIC)](https://quic.video/).

Stream multiple cameras to browsers in real-time with sub-second latency, track-level subscribe/unsubscribe, and priority-based bandwidth adaptation.

> **Status**: Phase 0 — Foundation (v0.1 in progress)

## Why

Existing solutions don't handle **multi-camera × WAN × scalable** well:

| Solution | Limitation |
|---|---|
| GStreamer + RTSP | No CDN scaling, poor WAN support |
| WebRTC (LiveKit) | SFU scaling limits, heavy on edge devices |
| ROS2 DDS | NAT/firewall issues over WAN, no video compression standard |
| AWS Kinesis Video | Vendor lock-in, 1-5s latency |

MoQ provides QUIC connection migration (mobile handover resilience), relay-based fan-out (CDN-like scaling), and track-level pub/sub (subscribe only to cameras you need).

## Architecture

```
Cameras → [moq-multicam-bridge] → WAN (MoQ/QUIC) → [moq-multicam-relay] → Browser (multi-view)
```

## Crates

| Crate | Description |
|---|---|
| `moq-multicam-core` | Core library: track management, sync, plugin traits |
| `moq-multicam-bridge` | Video source → MoQ publisher (GStreamer, V4L2) |
| `moq-multicam-relay` | Relay server (wraps moq-relay with multi-camera features) |
| `moq-multicam-cli` | CLI tool |

## Tech Stack

- **Rust** + tokio
- **MoQ**: [moq-lite](https://crates.io/crates/moq-lite) (moq-dev/moq)
- **QUIC**: quinn
- **Video**: gstreamer-rs, H.264
- **Browser**: TypeScript + WebTransport + WebCodecs

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.
