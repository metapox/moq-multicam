//! Publish 2 test cameras to a moq-relay over QUIC.
//!
//! Usage:
//!   1. Start relay:  moq-relay --server-bind "[::]:4443" --tls-generate localhost --tls-disable-verify --auth-public ""
//!   2. Run this:     cargo run -p quic-publish

use moq_multicam_bridge::TestSource;
use moq_multicam_core::*;

const VEHICLE_ID: &str = "truck-01";
const RELAY_URL: &str = "https://localhost:4443";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let cameras = vec![
        CameraConfig { name: "front".into(), priority: 0 },
        CameraConfig { name: "rear".into(), priority: 1 },
    ];

    let origin = Origin::produce();
    let (broadcast, tracks) = create_camera_broadcast(VEHICLE_ID, &cameras)?;

    let broadcast_path = TrackPath::camera(VEHICLE_ID, "front", Quality::High).broadcast_path();
    origin.publish_broadcast(&broadcast_path, broadcast.consume());
    tracing::info!("publishing broadcast: {broadcast_path}");

    // Spawn test sources (slow rate for demo)
    for (cam, track) in cameras.iter().zip(tracks) {
        let cam_name = cam.name.clone();
        let source = TestSource::new(2, 512, 3);
        tokio::spawn(async move {
            if let Err(e) = source.run(track).await {
                tracing::warn!(camera = %cam_name, "test source stopped: {e}");
            }
        });
        tracing::info!(camera = %cam.name, "started test source");
    }

    // Connect to relay over QUIC
    let mut config = moq_native::ClientConfig::default();
    config.tls.disable_verify = Some(true);
    let client = config.init()?;

    let url = url::Url::parse(RELAY_URL)?;
    tracing::info!(%url, "connecting to relay...");

    let session = client
        .with_publish(origin.consume())
        .connect(url)
        .await?;

    tracing::info!("connected! publishing to relay. Press Ctrl+C to stop.");

    session.closed().await?;
    Ok(())
}
