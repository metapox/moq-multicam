//! DEPRECATED: Uses old single-broadcast design. Use publish-fmp4 with --source gstreamer instead.

use anyhow::Result;
use url::Url;

use moq_multicam_bridge::TestSource;
use moq_multicam_core::*;

pub async fn run(
    relay: Url,
    vehicle_id: &str,
    cameras: &[CameraConfig],
    tls_disable_verify: bool,
) -> Result<()> {
    let origin = Origin::produce();
    let (broadcast, tracks) = create_camera_broadcast(vehicle_id, cameras)?;

    let broadcast_path = TrackPath::camera(vehicle_id, &cameras[0].name, Quality::High).broadcast_path();
    origin.publish_broadcast(&broadcast_path, broadcast.consume());

    for (cam, track) in cameras.iter().zip(tracks) {
        let cam_name = cam.name.clone();
        let source = TestSource::new(2, 512, 3);
        tokio::spawn(async move {
            if let Err(e) = source.run(track).await {
                tracing::warn!(camera = %cam_name, "test source stopped: {e}");
            }
        });
        tracing::info!(camera = %cam.name, priority = cam.priority, "started test source");
    }

    let mut config = moq_native::ClientConfig::default();
    if tls_disable_verify {
        config.tls.disable_verify = Some(true);
    }
    let client = config.init()?;

    tracing::info!(%relay, "connecting to relay...");
    let session = client.with_publish(origin.consume()).connect(relay).await?;
    tracing::info!("publishing. Press Ctrl+C to stop.");

    session.closed().await?;
    Ok(())
}
