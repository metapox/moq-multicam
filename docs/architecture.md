# Architecture

## Overview

```
Vehicle/Edge                     Cloud                        Operator
────────────                  ──────────                   ──────────────

┌──────────┐                 ┌──────────┐                 ┌────────────┐
│ Camera×8 │                 │          │                 │ Browser    │
│ front    │──┐              │ moq-relay│──WebTransport──│ Multi-cam  │
│ rear     │  │  QUIC/       │(external)│                │ viewer     │
│ left     │──┼──MoQ─────── │          │──WebTransport──│            │
│ right    │  │  (5G/LTE)   │          │                 └────────────┘
│ ...×4    │──┘              └──────────┘
└──────────┘
      │
┌──────────┐
│ bridge   │  ← One process, one QUIC connection, 16 pipelines
└──────────┘
```

## Data Flow (single frame, end to end)

```
① Camera captures image (raw ~6MB)
        ↓
② GStreamer encodes to H.264 (a few KB~tens of KB)
        ↓
③ hang OrderedProducer wraps into Legacy container (varint timestamp + payload)
        ↓
④ bridge writes to moq-lite Track within a per-camera Broadcast
   Broadcast: vehicle/truck-01/camera/front
   Track: video (or video-low)
   One Group = one keyframe interval (GOP)
        ↓
⑤ QUIC sends over the internet to relay
   Connection survives 5G↔LTE handover via QUIC connection migration
        ↓
⑥ relay forwards only to browsers that subscribed to this Track
   Under bandwidth pressure, lower-priority streams degrade first (QUIC stream priority)
        ↓
⑦ Browser receives via WebTransport
   @moq/lite subscribe with priority (P0 focus, P200 background)
        ↓
⑧ WebCodecs VideoDecoder decodes H.264 → Canvas 2D drawImage
```

## Broadcast-per-camera Design

Each camera is a separate MoQ Broadcast. This enables:
- **Independent subscribe/unsubscribe** per camera (no wasted bandwidth)
- **Per-camera priority** at the QUIC stream level
- **Independent failure** — one camera crash doesn't affect others

```
vehicle/truck-01/camera/front   (Broadcast)
  ├── video          (Track, 640×480 HQ, ~2 Mbps)
  ├── video-low      (Track, 320×240 LQ, ~500 kbps)
  └── catalog.json   (Track, codec/resolution metadata)

vehicle/truck-01/camera/rear    (Broadcast)
  ├── video
  ├── video-low
  └── catalog.json

... (8 cameras total)

vehicle/truck-01/meta           (Broadcast)
  └── manifest       (Track, JSON list of camera broadcasts)
```

The manifest track enables dynamic camera discovery — the browser reads it
to learn which cameras are available without hardcoding names.

## Crate Responsibilities

```
┌──────────────────────────────────────────────────────────────┐
│                    moq-multicam-core                          │
│                                                              │
│  TrackPath        CameraConfig    Quality     moq-lite       │
│  (naming)         (priority)      (High/Low)  re-exports     │
│                                                              │
│  Thin wrapper over moq-lite to absorb upstream API changes.  │
└──────────┬───────────────────────────────────────────────────┘
           │
           ▼
┌──────────────────────────┐
│  moq-multicam-bridge     │
│                          │
│  Video source (swappable)│
│  ├── GStreamer           │
│  ├── ffmpeg (stdin pipe) │
│  └── Test source         │
│                          │
│  H.264 encode            │
│  hang direct write       │
│  Error recovery          │
│  QUIC reconnection       │
│                          │
│  Runs on vehicle/edge    │
└──────────────────────────┘
           │
           ▼
┌──────────────────────────┐
│  moq-multicam-cli        │
│                          │
│  publish-fmp4            │
│    ├── stdin mode        │
│    ├── ffmpeg multicam   │
│    └── gstreamer multicam│
│  publish (test source)   │
│  subscribe               │
│                          │
│  Single binary           │
└──────────────────────────┘

┌──────────────────────────┐
│  web/ (JavaScript)       │
│                          │
│  @moq/lite subscribe     │
│  @moq/hang catalog parse │
│  WebCodecs VideoDecoder  │
│  Canvas 2D rendering     │
│  Focus camera switching  │
│  Priority control        │
│  Stats overlay           │
│                          │
│  Runs in browser         │
└──────────────────────────┘
```

The relay is **not** a crate in this project. We use
[moq-relay](https://github.com/moq-dev/moq) directly.

## MoQ Protocol Stack

```
┌─────────────────────────┐
│  moq-multicam           │  Track naming, camera groups, priority, rendition switching
├─────────────────────────┤
│  hang                   │  Media layer: catalog, Legacy container, timestamps
├─────────────────────────┤
│  moq-lite               │  Pub/Sub: Broadcasts, Tracks, Groups, Frames
├─────────────────────────┤
│  moq-native             │  QUIC connection helper (Quinn + TLS + WebTransport)
├─────────────────────────┤
│  Quinn / QUIC           │  Transport: streams, datagrams, connection migration
└─────────────────────────┘
```

moq-multicam-core wraps moq-lite types so upstream breaking changes are
absorbed in one place. moq-lite is 0.x with frequent API changes.

## Browser Viewer

The viewer uses `@moq/lite` and `@moq/hang` directly (not `@moq/watch`).
This gives full control over subscriber priority.

```
Connection.Reload (auto-reconnect WebTransport)
  → conn.consume("vehicle/truck-01/camera/front")  → Broadcast
    → broadcast.subscribe("catalog.json", 100)      → catalog (codec info)
    → broadcast.subscribe("video", 0)               → Track (priority 0 = focus)
      → track.nextGroupOrdered()                    → Group (GOP)
        → group.readFrame()                         → raw frame (varint ts + H.264)
          → Varint.decode()                         → timestamp + payload
            → VideoDecoder.decode(EncodedVideoChunk) → VideoFrame
              → canvas.drawImage(frame)
```

On focus switch:
1. Close old track subscription
2. Previous focus → `subscribe("video-low", 200)` (LQ + low priority)
3. New focus → `subscribe("video", 0)` (HQ + high priority)
4. Reconfigure VideoDecoder with new resolution

## Bandwidth Adaptation

Two mechanisms work together:

### 1. Rendition switching (application level)
Focus camera subscribes to `video` (640×480, ~2 Mbps).
Background cameras subscribe to `video-low` (320×240, ~500 kbps).
Switching happens on focus change.

### 2. Subscriber priority (QUIC level)
Focus camera: priority 0. Background cameras: priority 200.
The relay's QUIC implementation (quinn) prioritizes lower-numbered streams
when the send buffer has multiple streams competing for bandwidth.

```
Normal (20 Mbps available):
  All 8 cameras receive their subscribed rendition normally.

Degraded (5 Mbps available):
  Focus camera (video, P0)     → maintained at ~2 Mbps
  Background cameras (video-low, P200) → some may drop frames or stall
```

Additionally, moq-lite's Group-based model means late-arriving Groups are
skipped — the viewer always shows the latest available frame rather than
buffering old data.

## Bridge: Swappable Video Sources

```
Video source options:
  ├── GStreamer    ← Most versatile. USB/IP/RTSP cameras. Used in Docker.
  ├── ffmpeg       ← Stdin pipe. No GStreamer dependency needed.
  ├── Test source  ← Dummy frames. For dev/testing without cameras.
  └── (future) V4L2  ← Linux camera API. Lightweight.
```

Target platforms:
| Use case           | Hardware                    | OS          |
|--------------------|-----------------------------|-------------|
| Autonomous driving | NVIDIA Orin (ARM)           | Linux       |
| Surveillance       | Raspberry Pi / NUC          | Linux       |
| Dev/demo           | Docker on any OS            | macOS/Linux |

## Error Recovery

### QUIC Connection Loss

```
bridge ──QUIC──→ relay
         ↓ connection lost
bridge: detect → reconnect with new QUIC session
         → resume publishing from latest Group (keyframe)
         → subscribers see a brief freeze, then resume
```

QUIC connection migration handles most network transitions transparently.

### GStreamer Pipeline Error

```
GStreamer error (camera disconnect, encoder crash)
  → bridge detects pipeline failure
  → attempt pipeline restart after 2s delay
  → re-publish Track on success
  → other cameras continue unaffected
```

## Track Naming Convention

```
vehicle/{vehicle_id}/camera/{camera_name}/video       # HQ rendition
vehicle/{vehicle_id}/camera/{camera_name}/video-low   # LQ rendition
vehicle/{vehicle_id}/camera/{camera_name}/catalog.json # Codec metadata
vehicle/{vehicle_id}/meta/manifest                    # Camera discovery
vehicle/{vehicle_id}/control/command                  # Operator commands (Phase 2+)
```

Note: The full path `vehicle/truck-01/camera/front/video` is composed of
the Broadcast path (`vehicle/truck-01/camera/front`) and the Track name
(`video`). In the MoQ protocol, these are separate — the Broadcast path
is used for announce/consume, and the Track name is used for subscribe.

## Bidirectional Communication (Phase 2+)

```
Video:    bridge ──publish──→ relay ──subscribe──→ browser
Control:  browser ──publish──→ relay ──subscribe──→ bridge
```

moq-lite pub/sub is bidirectional by design. Control commands will use a
dedicated Track within a control Broadcast.

## TLS and Certificates

| Environment | Approach |
|-------------|----------|
| Development | Self-signed certs generated by moq-native |
| Docker demo | Self-signed certs, `--tls-disable-verify` |
| Production  | Let's Encrypt or custom CA |

## Known Limitations

- No authentication (open relay)
- No recording/playback
- Unidirectional only (no operator → vehicle commands yet)
- Fixed bitrate on publisher side (subscriber-side rendition switching works)
- USB camera support not yet tested (GStreamer test sources only)
- No adaptive bitrate on publisher (publisher always sends both renditions)
- ffmpeg mode is single-rendition only (multi-rendition requires GStreamer)
