# Architecture

## Overview

```
Vehicle/Edge                     Cloud                        Operator
────────────                  ──────────                   ──────────────

┌─────────┐                  ┌──────────┐                 ┌────────────┐
│ Camera×4 │                 │          │                 │ Browser    │
│ front    │──┐              │          │──WebTransport──│ Multi-cam  │
│ rear     │  │  QUIC/       │  relay   │                │ viewer     │
│ left     │──┼──MoQ─────── │          │──WebTransport──│            │
│ right    │  │  (5G/LTE)   │          │                 └────────────┘
└─────────┘  │              └──────────┘
             ▼
        ┌─────────┐
        │ bridge  │  ← One process bundles all cameras
        └─────────┘
```

## Data Flow (single frame, end to end)

```
① Camera captures image (raw ~6MB)
        ↓
② GStreamer encodes to H.264 (a few KB~tens of KB)
        ↓
③ hang wraps into fMP4 segment (with codec/resolution metadata)
        ↓
④ bridge writes to moq-lite Track
   Track name: vehicle/truck-01/camera/front/video
   Written per Group (one Group = one keyframe interval)
        ↓
⑤ QUIC sends over the internet to relay
   Connection survives 5G↔LTE handover via QUIC connection migration
        ↓
⑥ relay forwards only to browsers that subscribed to this Track
   Under bandwidth pressure, lower-priority cameras degrade first
        ↓
⑦ Browser receives via WebTransport
        ↓
⑧ WebCodecs decodes H.264 → renders to Canvas
```

## Crate Responsibilities

```
┌──────────────────────────────────────────────────────────────┐
│                    moq-multicam-core                          │
│                                                              │
│  Track naming     Camera types    Priority    moq-lite       │
│                                               abstraction    │
│                                                              │
│  Shared definitions. Thin wrapper over moq-lite to absorb    │
│  upstream API changes in one place.                          │
└──────────┬───────────────────────────────────┬───────────────┘
           │                                   │
           ▼                                   ▼
┌──────────────────────────┐      ┌──────────────────────────┐
│  moq-multicam-bridge     │      │  moq-multicam-relay      │
│                          │      │                          │
│  Video source (swappable)│      │  Fan-out relay           │
│  ├── GStreamer           │      │  Camera group management │
│  ├── V4L2               │      │  Priority-based          │
│  ├── Test source         │      │  bandwidth allocation    │
│  H.264 encode            │      │  Vehicle online/offline  │
│  fMP4 segmentation       │      │  Heartbeat monitoring    │
│  Multi-track publish     │      │                          │
│  Error recovery          │      │  (wraps moq-relay)       │
│  QUIC reconnection       │      │                          │
│                          │      │  Runs on cloud           │
│  Runs on vehicle/edge    │      │                          │
└──────────────────────────┘      └──────────────────────────┘
           │                               │
           └───────────┬───────────────────┘
                       ▼
           ┌──────────────────────┐
           │  moq-multicam-cli   │
           │                     │
           │  moq-multicam publish  ← starts bridge
           │  moq-multicam relay    ← starts relay
           │                     │
           │  Single binary      │
           └──────────────────────┘

           ┌──────────────────────┐
           │  web/ (TypeScript)  │
           │                     │
           │  Multi-camera grid  │
           │  Camera switching   │
           │  Dynamic subscribe  │
           │                     │
           │  Runs in browser    │
           └──────────────────────┘
```

## MoQ Protocol Stack

```
┌─────────────────────────┐
│  moq-multicam           │  Track naming, camera groups, priority, adaptive bitrate
├─────────────────────────┤
│  hang                   │  Media layer: catalog, fMP4/CMAF container, timestamps
├─────────────────────────┤
│  moq-lite               │  Pub/Sub: Broadcasts, Tracks, Groups, Frames
├─────────────────────────┤
│  moq-native             │  QUIC connection helper (Quinn + TLS + WebTransport)
├─────────────────────────┤
│  Quinn / QUIC           │  Transport: streams, datagrams, connection migration
└─────────────────────────┘
```

moq-multicam-core provides a thin abstraction over moq-lite. All crates access
moq-lite through core, so upstream breaking changes are absorbed in one place.
moq-lite is 0.x and its API is still evolving (e.g. `Origin::produce()` vs
`Origin::random().produce()` between published and unreleased versions).

## Bridge: Swappable Video Sources

bridge runs on diverse hardware. The video source is swappable:

```
┌─────────────────────────────────────────┐
│  moq-multicam-bridge                    │
│                                         │
│  ┌───────────────┐   ┌───────────────┐  │
│  │ Video source  │   │ MoQ output    │  │
│  │ (swappable)   │──→│ (common)      │  │
│  └───────────────┘   └───────────────┘  │
└─────────────────────────────────────────┘

Video source options:
  ├── GStreamer    ← Most versatile. USB/IP/RTSP cameras
  ├── V4L2        ← Linux camera API. Lightweight
  ├── Test source  ← Dummy frames. For dev/demo without cameras
  └── (future) ROS2  ← Direct feed from Autoware
```

Target platforms:
| Use case           | Hardware                    | OS          |
|--------------------|-----------------------------|-------------|
| Autonomous driving | NVIDIA Orin (ARM)           | Linux       |
| Surveillance       | Raspberry Pi / NUC (ARM/x86)| Linux       |
| Drone              | Jetson Nano (ARM)           | Linux       |
| Factory            | Server (x86)                | Linux       |
| Dev/demo           | MacBook / PC (ARM/x86)      | macOS/Linux |

## Bandwidth Adaptation

```
Normal (20 Mbps):
  front  → high quality (5 Mbps)   priority=0
  rear   → high quality (5 Mbps)   priority=1
  left   → high quality (5 Mbps)   priority=2
  right  → high quality (5 Mbps)   priority=2

Degraded (10 Mbps):
  front  → high quality (5 Mbps)   ← highest priority, maintained
  rear   → high quality (5 Mbps)   ← maintained
  left   → low quality (0.5 Mbps)  ← degraded
  right  → low quality (0.5 Mbps)  ← degraded

Severe (4 Mbps):
  front  → high quality (3 Mbps)   ← slightly reduced but maintained
  rear   → low quality (0.5 Mbps)  ← degraded
  left   → paused                  ← suspended
  right  → paused                  ← suspended
```

Enabled by: core's priority definitions + relay's bandwidth allocation +
bridge's adaptive bitrate + moq-lite's Group skipping (drop old data,
prioritize new).

Note: Adaptive bitrate is Phase 1+. Phase 0 uses fixed bitrate.

## Error Recovery

### QUIC Connection Loss (bridge → relay)

```
bridge ──QUIC──→ relay
         ↓ connection lost (tunnel, handover, etc.)
bridge: detect loss → reconnect with new QUIC session
         → resume publishing from latest Group (keyframe)
         → subscribers see a brief freeze, then resume
```

QUIC connection migration handles most network transitions (5G↔LTE)
transparently. Full disconnection requires reconnection, but moq-lite's
Group-based model means subscribers can resume from the next keyframe
without corrupted frames.

### GStreamer Pipeline Error

```
GStreamer error (camera disconnect, encoder crash)
  → bridge detects pipeline failure
  → drop affected Track (subscribers see camera offline)
  → attempt pipeline restart
  → re-publish Track on success
  → other cameras continue unaffected
```

### Relay Monitoring

relay tracks bridge connections via heartbeat. When a bridge disconnects:
- Mark vehicle as offline
- Notify all subscribers (browser shows "camera offline" UI)
- When bridge reconnects, re-announce the broadcast

## Bidirectional Communication (Phase 2+)

Teleoperation requires operator → vehicle commands, not just video.

```
Video:    bridge ──publish──→ relay ──subscribe──→ browser
Control:  browser ──publish──→ relay ──subscribe──→ bridge
```

Both directions use moq-lite pub/sub. Control commands use a dedicated Track:

```
vehicle/{vehicle_id}/control/command    # Operator → vehicle commands
```

This is out of scope for Phase 0-1 but the architecture supports it
natively — moq-lite pub/sub is bidirectional by design.

## Track Naming Convention

```
vehicle/{vehicle_id}/camera/{camera_name}/video       # Main quality
vehicle/{vehicle_id}/camera/{camera_name}/video-low   # Low quality
vehicle/{vehicle_id}/meta/status                      # Vehicle status
vehicle/{vehicle_id}/meta/detections                  # AI detections
vehicle/{vehicle_id}/control/command                  # Operator commands (Phase 2+)
```

## TLS and Certificates

QUIC requires TLS 1.3. Certificate handling per environment:

| Environment | Approach                                    |
|-------------|---------------------------------------------|
| Development | Self-signed certs generated by moq-native    |
| Docker demo | Pre-generated certs bundled in container     |
| Production  | Let's Encrypt or custom CA                  |

moq-native handles TLS setup. For development, it can generate and trust
self-signed certificates automatically.

## Why moq-lite (not IETF moq-transport)

moq-lite is a forwards-compatible subset of the IETF moq-transport draft.

- **Stability**: Controlled by moq-dev, not subject to IETF draft churn
- **Maturity**: Production-tested at quic.video
- **Compatibility**: moq-lite clients work with any moq-transport CDN, so migration to the full spec is seamless

### Risk: moq-lite API instability

moq-lite is 0.x with frequent breaking changes. Mitigation:
moq-multicam-core wraps moq-lite types, so upstream changes are absorbed
in one place rather than across all crates.

### Risk: IETF moq-transport divergence

If the final RFC diverges from moq-lite, the core abstraction layer
provides a migration path. The rest of moq-multicam doesn't depend on
moq-lite directly.

## Known Limitations (Phase 0)

- Fixed bitrate only (no adaptive bitrate)
- 2 cameras max (4-8 cameras in Phase 1)
- No authentication (open relay)
- No recording/playback
- Unidirectional only (no operator → vehicle commands)
- No metrics/observability beyond tracing logs
