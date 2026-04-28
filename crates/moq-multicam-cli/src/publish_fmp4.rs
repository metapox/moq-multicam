//! Publish video to a relay — single camera (stdin fMP4) or multi-camera.
//!
//! Single camera (stdin pipe, backward compatible):
//!   ffmpeg ... | moq-multicam publish-fmp4 --broadcast vehicle/truck-01/camera/front
//!
//! Multi-camera:
//!   moq-multicam publish-fmp4 --camera front --camera rear --source gstreamer

use std::time::Duration;

use anyhow::Result;
use tokio::task::JoinSet;
use url::Url;

use moq_multicam_core::CameraConfig;
#[cfg(any(feature = "gstreamer", feature = "openh264", feature = "v4l"))]
use moq_multicam_bridge::VideoSource;

/// Video source backend.
#[derive(Clone, Copy, Debug)]
pub enum SourceKind {
    Ffmpeg,
    #[cfg(feature = "gstreamer")]
    Gstreamer,
    #[cfg(feature = "openh264")]
    OpenH264,
    #[cfg(feature = "v4l")]
    V4l,
}

/// Rendition configuration for adaptive bitrate.
struct Rendition {
    track_name: &'static str,
    width: u32,
    height: u32,
    bitrate_kbps: u32,
    priority_offset: u8,
}

const RENDITIONS: &[Rendition] = &[
    Rendition { track_name: "video", width: 640, height: 480, bitrate_kbps: 2000, priority_offset: 0 },
    Rendition { track_name: "video-low", width: 320, height: 240, bitrate_kbps: 500, priority_offset: 2 },
];

/// Single camera: read fMP4 from stdin (backward compatible).
pub async fn run_stdin(relay: Url, broadcast_path: &str, tls_disable_verify: bool) -> Result<()> {
    let origin = moq_lite::Origin::produce();

    let mut broadcast = moq_lite::Broadcast::produce();
    let catalog = moq_mux::CatalogProducer::new(&mut broadcast)?;
    let fmp4 = moq_mux::import::Fmp4::new(
        broadcast.clone(), catalog,
        moq_mux::import::Fmp4Config { passthrough: false },
    );

    origin.publish_broadcast(broadcast_path, broadcast.consume());
    tracing::info!(broadcast = broadcast_path, "publishing fMP4 from stdin");

    let client = make_client(tls_disable_verify)?;
    let reconnect = client.with_publish(origin.consume()).reconnect(relay);

    let stdin_handle = tokio::spawn(read_stdin(fmp4));
    tokio::select! {
        res = reconnect.closed() => res?,
        res = stdin_handle => res??,
    }
    Ok(())
}

/// Multi-camera: all cameras in one process + one QUIC connection.
pub async fn run_multicam(
    relay: Url,
    vehicle_id: &str,
    cameras: &[CameraConfig],
    source_kind: SourceKind,
    tls_disable_verify: bool,
) -> Result<()> {
    let origin = moq_lite::Origin::produce();
    // Separate origin for subscribing to operator commands
    let subscribe_origin = moq_lite::Origin::produce();

    let client = make_client(tls_disable_verify)?;
    tracing::info!(%relay, "connecting to relay (with auto-reconnect)...");

    let reconnect = client
        .with_publish(origin.consume())
        .with_consume(subscribe_origin.clone())
        .reconnect(relay);

    // Subscribe to command track from operator
    let vehicle = vehicle_id.to_string();
    tokio::spawn(async move {
        subscribe_commands(subscribe_origin, &vehicle).await;
    });

    match source_kind {
        SourceKind::Ffmpeg => run_multicam_ffmpeg(&origin, vehicle_id, cameras, reconnect).await,
        #[cfg(feature = "gstreamer")]
        SourceKind::Gstreamer => run_multicam_gstreamer(&origin, vehicle_id, cameras, reconnect).await,
        #[cfg(feature = "openh264")]
        SourceKind::OpenH264 => run_multicam_openh264(&origin, vehicle_id, cameras, reconnect).await,
        #[cfg(feature = "v4l")]
        SourceKind::V4l => run_multicam_v4l(&origin, vehicle_id, cameras, reconnect).await,
    }
}

// ---------------------------------------------------------------------------
// Ffmpeg mode
// ---------------------------------------------------------------------------

async fn run_multicam_ffmpeg(
    origin: &moq_lite::OriginProducer,
    vehicle_id: &str,
    cameras: &[CameraConfig],
    reconnect: moq_native::Reconnect,
) -> Result<()> {
    let mut join_set = JoinSet::new();

    for cam in cameras {
        let path = format!("vehicle/{}/camera/{}", vehicle_id, cam.name);
        spawn_ffmpeg_camera(origin, &path, &cam.name, &mut join_set);
    }

    tracing::info!("all cameras publishing (ffmpeg). Press Ctrl+C to stop.");
    ffmpeg_camera_loop(origin, vehicle_id, reconnect, &mut join_set).await
}

fn spawn_ffmpeg_camera(
    origin: &moq_lite::OriginProducer,
    broadcast_path: &str,
    cam_name: &str,
    join_set: &mut JoinSet<String>,
) {
    let mut broadcast = moq_lite::Broadcast::produce();
    let catalog = moq_mux::CatalogProducer::new(&mut broadcast).expect("catalog creation failed");
    let fmp4 = moq_mux::import::Fmp4::new(
        broadcast.clone(), catalog,
        moq_mux::import::Fmp4Config { passthrough: false },
    );

    origin.publish_broadcast(broadcast_path, broadcast.consume());
    tracing::info!(camera = %cam_name, broadcast = %broadcast_path, "publishing camera (ffmpeg)");

    let source = moq_multicam_bridge::FfmpegSource::new(640, 480, 30);
    let name = cam_name.to_string();
    join_set.spawn(async move {
        if let Err(e) = source.run(fmp4).await {
            tracing::error!(camera = %name, "ffmpeg source failed: {e}");
        }
        name
    });
}

async fn ffmpeg_camera_loop(
    origin: &moq_lite::OriginProducer,
    vehicle_id: &str,
    reconnect: moq_native::Reconnect,
    join_set: &mut JoinSet<String>,
) -> Result<()> {
    loop {
        tokio::select! {
            res = reconnect.closed() => { res?; break; }
            Some(result) = join_set.join_next() => {
                let cam_name = result?;
                tracing::warn!(camera = %cam_name, "camera stopped, restarting in 2s...");
                tokio::time::sleep(Duration::from_secs(2)).await;
                let path = format!("vehicle/{}/camera/{}", vehicle_id, cam_name);
                spawn_ffmpeg_camera(origin, &path, &cam_name, join_set);
                tracing::info!(camera = %cam_name, "camera restarted");
            }
        }
    }
    join_set.abort_all();
    Ok(())
}

// ---------------------------------------------------------------------------
// GStreamer mode
// ---------------------------------------------------------------------------

#[cfg(feature = "gstreamer")]
async fn run_multicam_gstreamer(
    origin: &moq_lite::OriginProducer,
    vehicle_id: &str,
    cameras: &[CameraConfig],
    reconnect: moq_native::Reconnect,
) -> Result<()> {
    let mut join_set = JoinSet::new();

    publish_manifest(origin, vehicle_id, cameras)?;

    // BroadcastProducer must stay alive — dropping it closes the conducer channel,
    // causing subscribe_track to fail with Error::Dropped on the relay side.
    let mut broadcast_handles = Vec::new();

    for cam in cameras {
        let handles = publish_camera(origin, vehicle_id, cam, &mut join_set)?;
        broadcast_handles.push(handles);
    }

    tracing::info!("all cameras publishing. Press Ctrl+C to stop.");

    loop {
        tokio::select! {
            res = reconnect.closed() => { res?; break; }
            Some(result) = join_set.join_next() => {
                let cam_name = result?;
                tracing::warn!(camera = %cam_name, "camera stopped");
            }
        }
    }

    join_set.abort_all();
    Ok(())
}

/// Publish vehicle manifest for camera discovery.
fn publish_manifest(
    origin: &moq_lite::OriginProducer,
    vehicle_id: &str,
    cameras: &[CameraConfig],
) -> Result<()> {
    let manifest_path = format!("vehicle/{}/meta", vehicle_id);
    let mut broadcast = moq_lite::Broadcast::produce();
    let track = broadcast.create_track(moq_lite::Track {
        name: "manifest".to_string(),
        priority: 0,
    })?;

    let manifest = serde_json::json!({
        "vehicle_id": vehicle_id,
        "cameras": cameras.iter().map(|c| serde_json::json!({
            "name": c.name,
            "broadcast": format!("vehicle/{}/camera/{}", vehicle_id, c.name),
        })).collect::<Vec<_>>(),
    });

    let mut producer = hang::container::OrderedProducer::new(track);
    producer.keyframe();
    producer.write(hang::container::Frame {
        timestamp: hang::container::Timestamp::from_micros(0)?,
        payload: bytes::Bytes::from(manifest.to_string()).into(),
    })?;

    origin.publish_broadcast(&manifest_path, broadcast.consume());
    tracing::info!(broadcast = %manifest_path, "publishing vehicle manifest");
    Ok(())
}

/// Publish a single camera with all renditions. Returns handles to keep alive.
#[cfg(feature = "gstreamer")]
fn publish_camera(
    origin: &moq_lite::OriginProducer,
    vehicle_id: &str,
    cam: &CameraConfig,
    join_set: &mut JoinSet<String>,
) -> Result<(moq_lite::BroadcastProducer, moq_mux::CatalogProducer)> {
    publish_camera_with(origin, vehicle_id, cam, join_set, |r| {
        moq_multicam_bridge::GstreamerSource::new(r.width, r.height, 30, r.bitrate_kbps)
    })
}

// ---------------------------------------------------------------------------
// OpenH264 mode
// ---------------------------------------------------------------------------

#[cfg(feature = "openh264")]
async fn run_multicam_openh264(
    origin: &moq_lite::OriginProducer,
    vehicle_id: &str,
    cameras: &[CameraConfig],
    reconnect: moq_native::Reconnect,
) -> Result<()> {
    let mut join_set = JoinSet::new();

    publish_manifest(origin, vehicle_id, cameras)?;

    let mut broadcast_handles = Vec::new();

    for (i, cam) in cameras.iter().enumerate() {
        let cam_idx = i as u8;
        let handles = publish_camera_with(origin, vehicle_id, cam, &mut join_set, |r| {
            moq_multicam_bridge::OpenH264Source::new(r.width, r.height, 30, r.bitrate_kbps)
                .with_index(cam_idx)
        })?;
        broadcast_handles.push(handles);
    }

    tracing::info!("all cameras publishing (openh264). Press Ctrl+C to stop.");

    loop {
        tokio::select! {
            res = reconnect.closed() => { res?; break; }
            Some(result) = join_set.join_next() => {
                let cam_name = result?;
                tracing::warn!(camera = %cam_name, "camera stopped");
            }
        }
    }

    join_set.abort_all();
    Ok(())
}

// ---------------------------------------------------------------------------
// V4L2 mode
// ---------------------------------------------------------------------------

#[cfg(feature = "v4l")]
async fn run_multicam_v4l(
    origin: &moq_lite::OriginProducer,
    vehicle_id: &str,
    cameras: &[CameraConfig],
    reconnect: moq_native::Reconnect,
) -> Result<()> {
    let mut join_set = JoinSet::new();

    publish_manifest(origin, vehicle_id, cameras)?;

    let mut broadcast_handles = Vec::new();

    // Each camera name maps to a device path: "front" → /dev/video0, etc.
    // For now, use camera index as device index.
    for (i, cam) in cameras.iter().enumerate() {
        let device_path = format!("/dev/video{}", i);
        let dp = device_path.clone();
        let handles = publish_camera_with(origin, vehicle_id, cam, &mut join_set, move |r| {
            moq_multicam_bridge::V4lSource::new(&dp, r.width, r.height, 30, r.bitrate_kbps)
        })?;
        broadcast_handles.push(handles);
    }

    tracing::info!("all cameras publishing (v4l). Press Ctrl+C to stop.");

    loop {
        tokio::select! {
            res = reconnect.closed() => { res?; break; }
            Some(result) = join_set.join_next() => {
                let cam_name = result?;
                tracing::warn!(camera = %cam_name, "camera stopped");
            }
        }
    }

    join_set.abort_all();
    Ok(())
}

/// Generic camera publisher — works with any VideoSource.
#[cfg(any(feature = "gstreamer", feature = "openh264", feature = "v4l"))]
fn publish_camera_with<S: VideoSource>(
    origin: &moq_lite::OriginProducer,
    vehicle_id: &str,
    cam: &CameraConfig,
    join_set: &mut JoinSet<String>,
    make_source: impl Fn(&Rendition) -> S,
) -> Result<(moq_lite::BroadcastProducer, moq_mux::CatalogProducer)> {
    let broadcast_path = format!("vehicle/{}/camera/{}", vehicle_id, cam.name);
    let mut broadcast = moq_lite::Broadcast::produce();
    let mut catalog = moq_mux::CatalogProducer::new(&mut broadcast)?;

    {
        let mut cat = catalog.lock();
        for r in RENDITIONS {
            cat.video.insert(r.track_name, make_video_config(r.width, r.height, r.bitrate_kbps, 30.0))?;
        }
    }

    for r in RENDITIONS {
        let track = broadcast.create_track(moq_lite::Track {
            name: r.track_name.to_string(),
            priority: cam.priority + r.priority_offset,
        })?;
        let producer = hang::container::OrderedProducer::new(track);

        tracing::info!(camera = %cam.name, track = %r.track_name, w = %r.width, h = %r.height, bitrate_kbps = %r.bitrate_kbps, "publishing rendition");

        let source = make_source(r);
        let name = cam.name.clone();
        let track_name = r.track_name.to_string();
        join_set.spawn(async move {
            if let Err(e) = source.run(producer).await {
                tracing::error!(camera = %name, rendition = %track_name, "source failed: {e}");
            }
            name
        });
    }

    origin.publish_broadcast(&broadcast_path, broadcast.consume());
    tracing::info!(camera = %cam.name, broadcast = %broadcast_path, "publishing camera broadcast");

    Ok((broadcast, catalog))
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn make_video_config(width: u32, height: u32, bitrate_kbps: u32, fps: f64) -> hang::catalog::VideoConfig {
    hang::catalog::VideoConfig {
        codec: hang::catalog::H264 {
            profile: 0x42,
            constraints: 0xC0,
            level: 0x1E,
            inline: true,
        }
        .into(),
        description: None,
        coded_width: Some(width),
        coded_height: Some(height),
        display_ratio_width: None,
        display_ratio_height: None,
        bitrate: Some(bitrate_kbps as u64 * 1000),
        framerate: Some(fps),
        optimize_for_latency: None,
        container: hang::catalog::Container::Legacy,
        jitter: None,
    }
}


/// Subscribe to operator commands and log them.
async fn subscribe_commands(origin: moq_lite::OriginProducer, vehicle_id: &str) {
    let control_path = format!("vehicle/{}/control", vehicle_id);
    let path: moq_lite::Path<'_> = control_path.as_str().into();

    tracing::info!(path = %control_path, "waiting for operator commands...");

    let mut consumer = match origin.consume_only(&[path]) {
        Some(c) => c,
        None => { tracing::warn!("failed to consume control path"); return; }
    };

    tracing::info!("waiting for announced broadcasts...");

    while let Some((announced_path, maybe_broadcast)) = consumer.announced().await {
        tracing::info!(path = %announced_path, has_broadcast = maybe_broadcast.is_some(), "announced");
        let broadcast = match maybe_broadcast {
            Some(b) => b,
            None => continue,
        };

        let mut track = match broadcast.subscribe_track(&moq_lite::Track {
            name: "command".to_string(),
            priority: 0,
        }) {
            Ok(t) => t,
            Err(e) => { tracing::warn!("failed to subscribe command track: {e}"); continue; }
        };

        tracing::info!("operator connected, receiving commands");

        loop {
            match track.recv_group().await {
                Ok(Some(mut group)) => {
                    while let Ok(Some(frame)) = group.read_frame().await {
                        match serde_json::from_slice::<serde_json::Value>(&frame) {
                            Ok(cmd) => tracing::info!(command = %cmd, "received operator command"),
                            Err(e) => tracing::warn!("invalid command JSON: {e}"),
                        }
                    }
                }
                Ok(None) => {
                    tracing::info!("operator disconnected");
                    break;
                }
                Err(e) => {
                    tracing::warn!("command read error: {e}");
                    break;
                }
            }
        }
    }
}
fn make_client(tls_disable_verify: bool) -> Result<moq_native::Client> {
    let mut config = moq_native::ClientConfig::default();
    if tls_disable_verify {
        config.tls.disable_verify = Some(true);
    }
    config.init().map_err(Into::into)
}

async fn read_stdin(mut fmp4: moq_mux::import::Fmp4) -> Result<()> {
    let mut stdin = tokio::io::stdin();
    let mut buffer = bytes::BytesMut::new();
    loop {
        let n = tokio::io::AsyncReadExt::read_buf(&mut stdin, &mut buffer).await?;
        if n == 0 { return Ok(()); }
        fmp4.decode(&mut buffer)?;
    }
}
