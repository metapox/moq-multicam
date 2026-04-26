# Architecture

## Crate Structure

```
moq-multicam/
├── moq-multicam-core     # Shared types: track naming, camera metadata, plugin traits
├── moq-multicam-bridge   # Video source → MoQ publisher (GStreamer, V4L2)
├── moq-multicam-relay    # Relay server with multi-camera features
└── moq-multicam-cli      # CLI tool (binary: moq-multicam)
```

### Dependency Graph

```
core ← bridge
core ← relay
core + bridge ← cli
```

This mirrors the MoQ pub/sub architecture: publishers (bridge) and subscribers are fully decoupled through a relay. Separating bridge and relay into distinct crates enforces this at the code level.

### core

Shared definitions used by both publisher and subscriber sides:

- Track naming convention (`vehicle/{id}/camera/{name}/video`)
- Camera metadata types
- Plugin traits for extensibility (AI inference, custom processing)

### bridge

Converts video sources into MoQ tracks. Pipeline:

```
Camera/GStreamer → H.264 encode → fMP4 segment (via hang) → moq-lite Track
```

### relay

Wraps moq-relay with multi-camera awareness:

- Track discovery and routing
- Priority-based bandwidth adaptation (e.g., front camera > side cameras)
- Camera group management per vehicle

### cli

User-facing binary that composes bridge + core for publishing, and connects to relay.

## MoQ Protocol Stack

```
┌─────────────────────────┐
│  Application (multicam) │  Track naming, camera management
├─────────────────────────┤
│  hang                   │  Media layer: catalog, fMP4/CMAF container, timestamps
├─────────────────────────┤
│  moq-lite               │  Pub/Sub transport: Broadcasts, Tracks, Groups, Frames
├─────────────────────────┤
│  moq-native             │  QUIC connection helper (Quinn + TLS + WebTransport)
├─────────────────────────┤
│  Quinn / QUIC           │  Transport: streams, datagrams, connection migration
└─────────────────────────┘
```

### Why moq-lite (not IETF moq-transport)

moq-lite is a forwards-compatible subset of the IETF moq-transport draft.

- **Stability**: Controlled by moq-dev, not subject to IETF draft churn
- **Maturity**: Production-tested at quic.video
- **Compatibility**: moq-lite clients work with any moq-transport CDN, so migration to the full spec is seamless

### What hang does

MoQ itself is a generic byte-stream pub/sub — it knows nothing about video. hang provides:

- **Catalog**: Metadata describing which Track carries which codec/resolution
- **Container**: fMP4/CMAF segment generation and parsing
- **Sync**: Keyframe-aligned Groups for timestamp synchronization

### What moq-native does

Abstracts Quinn QUIC connections: TLS certificate handling, WebTransport session establishment, reconnection. Used by both bridge and relay. The `iroh` feature (P2P networking) is disabled — not needed for our use case.

## Data Flow

```
[Camera] → GStreamer → H.264 → [bridge] → fMP4 segments
                                    │
                                    ▼
                              moq-lite Track
                                    │
                              QUIC / WebTransport
                                    │
                                    ▼
                               [relay]
                                    │
                         ┌──────────┼──────────┐
                         ▼          ▼          ▼
                    [Browser]  [Browser]  [Browser]
                    WebTransport + WebCodecs
```

## Track Naming Convention

```
vehicle/{vehicle_id}/camera/{camera_name}/video       # Main quality
vehicle/{vehicle_id}/camera/{camera_name}/video-low   # Low quality
vehicle/{vehicle_id}/meta/status                      # Vehicle status
vehicle/{vehicle_id}/meta/detections                  # AI detections
```
