//! Multi-camera test source example.
//!
//! Publishes dummy video from 2 cameras (front + rear) using core types,
//! and a subscriber reads both tracks. No network or relay required.

use moq_multicam_bridge::TestSource;
use moq_multicam_core::*;

const VEHICLE_ID: &str = "truck-01";

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
    let consumer = origin.consume();

    let (broadcast, tracks) = create_camera_broadcast(VEHICLE_ID, &cameras)?;

    let broadcast_path = TrackPath::camera(VEHICLE_ID, "front", Quality::High).broadcast_path();
    origin.publish_broadcast(&broadcast_path, broadcast.consume());
    tracing::info!("published broadcast: {broadcast_path}");

    // Spawn a TestSource per camera
    for (cam, track) in cameras.iter().zip(tracks) {
        let cam_name = cam.name.clone();
        let source = TestSource::new(30, 512, 5);
        tokio::spawn(async move {
            if let Err(e) = source.run(track).await {
                tracing::warn!(camera = %cam_name, "test source stopped: {e}");
            }
        });
        tracing::info!(camera = %cam.name, priority = cam.priority, "started test source");
    }

    subscribe(consumer, VEHICLE_ID, &cameras).await
}

async fn subscribe(
    consumer: OriginConsumer,
    vehicle_id: &str,
    cameras: &[CameraConfig],
) -> anyhow::Result<()> {
    let broadcast_path = TrackPath::camera(vehicle_id, "front", Quality::High).broadcast_path();
    let path: moq_multicam_core::Path<'_> = broadcast_path.as_str().into();
    let mut origin = consumer
        .consume_only(&[path])
        .ok_or_else(|| anyhow::anyhow!("failed to consume origin"))?;

    tracing::info!("waiting for broadcast...");

    while let Some((path, maybe_broadcast)) = origin.announced().await {
        let broadcast = match maybe_broadcast {
            Some(b) => b,
            None => continue,
        };

        tracing::info!(%path, "broadcast online");

        let mut handles = Vec::new();
        for cam in cameras {
            let track_path = TrackPath::camera(vehicle_id, &cam.name, Quality::High);
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
                            if group_count >= 3 {
                                tracing::info!(camera = %cam_name, "received enough, stopping");
                                return;
                            }
                        }
                        _ => return,
                    }
                }
            }));
        }

        for h in handles {
            h.await?;
        }
        return Ok(());
    }

    Ok(())
}
