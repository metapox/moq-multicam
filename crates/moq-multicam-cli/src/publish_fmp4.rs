//! Publish fMP4 to a relay — single camera (stdin) or multi-camera (ffmpeg subprocesses).
//!
//! Single camera (stdin pipe):
//!   ffmpeg ... | moq-multicam publish-fmp4 --broadcast vehicle/truck-01/camera/front
//!
//! Multi-camera (built-in ffmpeg):
//!   moq-multicam publish-fmp4 --camera front --camera rear --vehicle truck-01

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

    let session = connect_relay(relay, &origin, tls_disable_verify).await?;

    let stdin_handle = tokio::spawn(read_stdin(fmp4));
    tokio::select! {
        res = session.closed() => res?,
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

    // Connect to relay FIRST, then spawn ffmpeg processes.
    // Origin dynamically announces broadcasts as they are added.
    let session = connect_relay(relay, &origin, tls_disable_verify).await?;

    let mut join_set = JoinSet::new();

    for cam in cameras {
        let broadcast_path = TrackPath::camera(vehicle_id, &cam.name, Quality::High).broadcast_path()
            + "/" + &TrackPath::camera(vehicle_id, &cam.name, Quality::High).track_name().split('/').next().unwrap_or(&cam.name);

        // Each camera gets its own Broadcast + Fmp4 decoder
        let mut broadcast = moq_lite::Broadcast::produce();
        let catalog = moq_mux::CatalogProducer::new(&mut broadcast)?;
        let fmp4 = moq_mux::import::Fmp4::new(
            broadcast.clone(), catalog,
            moq_mux::import::Fmp4Config { passthrough: false },
        );

        let cam_broadcast_path = format!("vehicle/{}/camera/{}", vehicle_id, cam.name);
        origin.publish_broadcast(&cam_broadcast_path, broadcast.consume());
        tracing::info!(camera = %cam.name, broadcast = %cam_broadcast_path, "publishing camera");

        let source = FfmpegSource::new(640, 480, 30);
        let cam_name = cam.name.clone();
        join_set.spawn(async move {
            if let Err(e) = source.run(fmp4).await {
                tracing::error!(camera = %cam_name, "ffmpeg source failed: {e}");
            }
            cam_name
        });
    }

    tracing::info!("all cameras publishing. Press Ctrl+C to stop.");

    // Wait for session close or any ffmpeg to exit
    tokio::select! {
        res = session.closed() => res?,
        Some(result) = join_set.join_next() => {
            let cam_name = result?;
            tracing::warn!(camera = %cam_name, "camera stopped, continuing with remaining cameras");
        }
    }

    // Abort remaining ffmpeg processes on exit
    join_set.abort_all();
    Ok(())
}

async fn connect_relay(
    relay: Url,
    origin: &moq_lite::OriginProducer,
    tls_disable_verify: bool,
) -> Result<moq_lite::Session> {
    let mut config = moq_native::ClientConfig::default();
    if tls_disable_verify {
        config.tls.disable_verify = Some(true);
    }
    let client = config.init()?;
    tracing::info!(%relay, "connecting to relay...");
    let session = client.with_publish(origin.consume()).connect(relay).await?;
    tracing::info!("connected to relay");
    Ok(session)
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
