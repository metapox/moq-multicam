//! Publish fMP4 to a relay — single camera (stdin) or multi-camera (ffmpeg subprocesses).
//!
//! Single camera (stdin pipe):
//!   ffmpeg ... | moq-multicam publish-fmp4 --broadcast vehicle/truck-01/camera/front
//!
//! Multi-camera (built-in ffmpeg):
//!   moq-multicam publish-fmp4 --camera front --camera rear --vehicle truck-01

use std::time::Duration;

use anyhow::Result;
use tokio::task::JoinSet;
use url::Url;

use moq_multicam_bridge::FfmpegSource;
use moq_multicam_core::*;

/// Single camera: read fMP4 from stdin (backward compatible with Phase 0a).
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

/// Multi-camera: spawn ffmpeg per camera, all in one process + one QUIC connection.
pub async fn run_multicam(
    relay: Url,
    vehicle_id: &str,
    cameras: &[CameraConfig],
    tls_disable_verify: bool,
) -> Result<()> {
    let origin = moq_lite::Origin::produce();

    // Connect with auto-reconnect before spawning ffmpeg.
    let client = make_client(tls_disable_verify)?;
    tracing::info!(%relay, "connecting to relay (with auto-reconnect)...");
    let reconnect = client.with_publish(origin.consume()).reconnect(relay);

    let mut join_set = JoinSet::new();

    for cam in cameras {
        let cam_broadcast_path = format!("vehicle/{}/camera/{}", vehicle_id, cam.name);
        spawn_camera(&origin, &cam_broadcast_path, &cam.name, &mut join_set);
    }

    tracing::info!("all cameras publishing. Press Ctrl+C to stop.");

    loop {
        tokio::select! {
            res = reconnect.closed() => {
                res?;
                break;
            }
            Some(result) = join_set.join_next() => {
                let cam_name = result?;
                tracing::warn!(camera = %cam_name, "camera stopped, restarting in 2s...");
                tokio::time::sleep(Duration::from_secs(2)).await;

                // Restart the camera
                let cam_broadcast_path = format!("vehicle/{}/camera/{}", vehicle_id, cam_name);
                spawn_camera(&origin, &cam_broadcast_path, &cam_name, &mut join_set);
                tracing::info!(camera = %cam_name, "camera restarted");
            }
        }
    }

    join_set.abort_all();
    Ok(())
}

fn spawn_camera(
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
    tracing::info!(camera = %cam_name, broadcast = %broadcast_path, "publishing camera");

    let source = FfmpegSource::new(640, 480, 30);
    let name = cam_name.to_string();
    join_set.spawn(async move {
        if let Err(e) = source.run(fmp4).await {
            tracing::error!(camera = %name, "ffmpeg source failed: {e}");
        }
        name
    });
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
