use anyhow::Result;
use url::Url;

use moq_multicam_core::*;

pub async fn run(
    relay: Url,
    vehicle_id: &str,
    cameras: &[CameraConfig],
    tls_disable_verify: bool,
) -> Result<()> {
    let origin = Origin::produce();

    let mut config = moq_native::ClientConfig::default();
    if tls_disable_verify {
        config.tls.disable_verify = Some(true);
    }
    let client = config.init()?;

    tracing::info!(%relay, "connecting to relay...");
    let _session = client.with_consume(origin.clone()).connect(relay).await?;
    tracing::info!("connected");

    let broadcast_path = TrackPath::camera(vehicle_id, &cameras[0].name, Quality::High).broadcast_path();
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
                                camera = %cam_name, group = group_count,
                                frames, bytes, "received group"
                            );
                            group_count += 1;
                        }
                        _ => {
                            tracing::info!(camera = %cam_name, "track ended ({group_count} groups)");
                            return;
                        }
                    }
                }
            }));
        }

        for h in handles {
            let _ = h.await;
        }
        return Ok(());
    }

    Ok(())
}
