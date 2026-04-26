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
   Connection survives 5G↔LTE handover
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
┌──────────────────────────────────────────────────────────┐
│                    moq-multicam-core                      │
│                                                          │
│  Track naming     Camera types    Priority   Plugin trait │
│                                                          │
│  Shared definitions used by all crates. No runtime logic.│
└──────────┬───────────────────────────────┬───────────────┘
           │                               │
           ▼                               ▼
┌──────────────────────┐      ┌──────────────────────────┐
│  moq-multicam-bridge │      │  moq-multicam-relay      │
│                      │      │                          │
│  Camera capture      │      │  Fan-out relay           │
│  (GStreamer)         │      │  Camera group management │
│  H.264 encode        │      │  Priority-based          │
│  fMP4 segmentation   │      │  bandwidth allocation    │
│  Multi-track publish │      │  Vehicle online/offline  │
│  Adaptive bitrate    │      │                          │
│                      │      │  (wraps moq-relay)       │
│  Runs on vehicle/edge│      │  Runs on cloud           │
└──────────────────────┘      └──────────────────────────┘
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

## Bandwidth Adaptation (core value of moq-multicam)

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

This is enabled by: core's priority definitions + relay's bandwidth allocation + bridge's adaptive bitrate + moq-lite's Group skipping (drop old data, prioritize new).

## Track Naming Convention

```
vehicle/{vehicle_id}/camera/{camera_name}/video       # Main quality
vehicle/{vehicle_id}/camera/{camera_name}/video-low   # Low quality
vehicle/{vehicle_id}/meta/status                      # Vehicle status
vehicle/{vehicle_id}/meta/detections                  # AI detections
```

## Why moq-lite (not IETF moq-transport)

moq-lite is a forwards-compatible subset of the IETF moq-transport draft.

- **Stability**: Controlled by moq-dev, not subject to IETF draft churn
- **Maturity**: Production-tested at quic.video
- **Compatibility**: moq-lite clients work with any moq-transport CDN, so migration to the full spec is seamless
