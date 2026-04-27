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

use moq_multicam_core::*;

/// Video source backend.
#[derive(Clone, Copy, Debug)]
pub enum SourceKind {
    Ffmpeg,
    #[cfg(feature = "gstreamer")]
    Gstreamer,
}

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

    let client = make_client(tls_disable_verify)?;
    tracing::info!(%relay, "connecting to relay (with auto-reconnect)...");
    let reconnect = client.with_publish(origin.consume()).reconnect(relay);

    match source_kind {
        SourceKind::Ffmpeg => run_multicam_ffmpeg(&origin, vehicle_id, cameras, reconnect).await,
        #[cfg(feature = "gstreamer")]
        SourceKind::Gstreamer => run_multicam_gstreamer(&origin, vehicle_id, cameras, reconnect).await,
    }
}

/// Ffmpeg mode: separate Broadcast per camera (import::Fmp4 limitation).
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

    tracing::info!("all cameras publishing (ffmpeg, separate broadcasts). Press Ctrl+C to stop.");
    camera_loop(origin, vehicle_id, SourceKind::Ffmpeg, reconnect, &mut join_set).await
}

/// GStreamer mode: separate Broadcast per camera (with renditions) + manifest.
#[cfg(feature = "gstreamer")]
async fn run_multicam_gstreamer(
    origin: &moq_lite::OriginProducer,
    vehicle_id: &str,
    cameras: &[CameraConfig],
    reconnect: moq_native::Reconnect,
) -> Result<()> {
    let mut join_set = JoinSet::new();

    let renditions: &[(&str, u32, u32, u32, u8)] = &[
        ("video", 640, 480, 2000, 0),
        ("video-low", 320, 240, 500, 2),
    ];

    // Publish manifest (camera discovery)
    let manifest_path = format!("vehicle/{}/meta", vehicle_id);
    let mut manifest_broadcast = moq_lite::Broadcast::produce();
    let manifest_track = manifest_broadcast.create_track(moq_lite::Track {
        name: "manifest".to_string(),
        priority: 0,
    })?;

    let manifest_json = serde_json::json!({
        "vehicle_id": vehicle_id,
        "cameras": cameras.iter().map(|c| serde_json::json!({
            "name": c.name,
            "broadcast": format!("vehicle/{}/camera/{}", vehicle_id, c.name),
        })).collect::<Vec<_>>(),
    });

    // Write manifest as a single group/frame
    let mut manifest_producer = hang::container::OrderedProducer::new(manifest_track);
    manifest_producer.keyframe();
    manifest_producer.write(hang::container::Frame {
        timestamp: hang::container::Timestamp::from_micros(0)?,
        payload: bytes::Bytes::from(manifest_json.to_string()).into(),
    })?;

    origin.publish_broadcast(&manifest_path, manifest_broadcast.consume());
    tracing::info!(broadcast = %manifest_path, "publishing vehicle manifest");

    // Publish each camera as a separate Broadcast with renditions
    // BroadcastProducer must stay alive — dropping it closes the conducer channel,
    // causing subscribe_track to fail with Error::Dropped on the relay side.
    let mut _broadcasts = Vec::new();

    for cam in cameras {
        let cam_broadcast_path = format!("vehicle/{}/camera/{}", vehicle_id, cam.name);
        let mut broadcast = moq_lite::Broadcast::produce();
        let mut catalog = moq_mux::CatalogProducer::new(&mut broadcast)?;

        // Create video tracks and populate catalog
        let mut producers = Vec::new();
        {
            let mut cat = catalog.lock();
            for &(suffix, w, h, bitrate_kbps, _) in renditions {
                cat.video.insert(suffix, make_video_config(w, h, bitrate_kbps, 30.0))?;
            }
        }
        for &(suffix, w, h, bitrate_kbps, prio_offset) in renditions {
            let track = broadcast.create_track(moq_lite::Track {
                name: suffix.to_string(),
                priority: cam.priority + prio_offset,
            })?;
            producers.push((suffix, w, h, bitrate_kbps, hang::container::OrderedProducer::new(track)));
        }

        origin.publish_broadcast(&cam_broadcast_path, broadcast.consume());
        tracing::info!(camera = %cam.name, broadcast = %cam_broadcast_path, "publishing camera broadcast");

        // Keep BroadcastProducer and CatalogProducer alive
        _broadcasts.push((broadcast, catalog));

        // Spawn GStreamer sources
        for (suffix, w, h, bitrate_kbps, producer) in producers {
            tracing::info!(camera = %cam.name, track = %suffix, %w, %h, %bitrate_kbps, "publishing rendition");
            let source = moq_multicam_bridge::GstreamerSource::new(w, h, 30, bitrate_kbps);
            let name = cam.name.clone();
            let suf = suffix.to_string();
            join_set.spawn(async move {
                if let Err(e) = source.run(producer).await {
                    tracing::error!(camera = %name, rendition = %suf, "gstreamer source failed: {e}");
                }
                name
            });
        }
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
    tracing::info!(camera = %cam_name, broadcast = %broadcast_path, source = ?SourceKind::Ffmpeg, "publishing camera");

    let source = moq_multicam_bridge::FfmpegSource::new(640, 480, 30);
    let name = cam_name.to_string();
    join_set.spawn(async move {
        if let Err(e) = source.run(fmp4).await {
            tracing::error!(camera = %name, "ffmpeg source failed: {e}");
        }
        name
    });
}

async fn camera_loop(
    origin: &moq_lite::OriginProducer,
    vehicle_id: &str,
    source_kind: SourceKind,
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
                match source_kind {
                    SourceKind::Ffmpeg => spawn_ffmpeg_camera(origin, &path, &cam_name, join_set),
                    #[cfg(feature = "gstreamer")]
                    _ => {}
                }
                tracing::info!(camera = %cam_name, "camera restarted");
            }
        }
    }
    join_set.abort_all();
    Ok(())
}

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
