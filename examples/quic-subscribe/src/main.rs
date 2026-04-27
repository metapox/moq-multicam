//! Subscribe to test cameras from a moq-relay over QUIC.
//!
//! Usage (3 terminals):
//!   1. moq-relay --server-bind "[::]:4443" --tls-generate localhost --tls-disable-verify --auth-public ""
//!   2. cargo run -p quic-publish
//!   3. cargo run -p quic-subscribe

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

    // Connect to relay as consumer
    let mut config = moq_native::ClientConfig::default();
    config.tls.disable_verify = Some(true);
    let client = config.init()?;

    let url = url::Url::parse(RELAY_URL)?;
    tracing::info!(%url, "connecting to relay...");

    let session = client
        .with_consume(origin.clone())
        .connect(url)
        .await?;

    tracing::info!("connected to relay");

    // Wait for the broadcast to appear
    let broadcast_path = TrackPath::camera(VEHICLE_ID, "front", Quality::High).broadcast_path();
    let path: Path<'_> = broadcast_path.as_str().into();
    let mut consumer = origin
        .consume_only(&[path])
        .ok_or_else(|| anyhow::anyhow!("failed to consume origin"))?;

    tracing::info!("waiting for broadcast: {broadcast_path}");

    while let Some((path, maybe_broadcast)) = consumer.announced().await {
        let broadcast = match maybe_broadcast {
            Some(b) => b,
            None => {
                tracing::warn!(%path, "broadcast offline");
                continue;
            }
        };

        tracing::info!(%path, "broadcast online, subscribing to cameras");

        let mut handles = Vec::new();
        for cam in &cameras {
            let track_path = TrackPath::camera(VEHICLE_ID, &cam.name, Quality::High);
            let mut track = broadcast.subscribe_track(&Track {
                name: track_path.track_name(),
                priority: cam.priority,
            })?;

            let cam_name = cam.name.clone();
            handles.push(tokio::spawn(async move {
                let mut group_count = 0u64;
                loop {
                    match track.recv_group().await {
                        Ok(Some(mut group)) => {
                            let mut frames = 0usize;
                            let mut bytes = 0usize;
                            while let Some(f) = group.read_frame().await.unwrap_or(None) {
                                bytes += f.len();
                                frames += 1;
                            }
                            tracing::info!(
                                camera = %cam_name,
                                group = group_count,
                                frames,
                                bytes,
                                "received group"
                            );
                            group_count += 1;
                        }
                        _ => {
                            tracing::info!(camera = %cam_name, "track ended after {group_count} groups");
                            return;
                        }
                    }
                }
            }));
        }

        // Run until session closes or all tracks end
        for h in handles {
            let _ = h.await;
        }
        return Ok(());
    }

    Ok(())
}
