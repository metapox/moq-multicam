//! Minimal moq-lite pub/sub example — no network, no relay.
//!
//! A publisher writes dummy video frames to a Track, and a subscriber
//! reads them back through an in-memory Origin. This demonstrates the
//! core moq-lite data model: Origin > Broadcast > Track > Group > Frame.

use bytes::Bytes;

const NUM_GROUPS: u64 = 3;
const FRAMES_PER_GROUP: usize = 5;
const FRAME_SIZE: usize = 1024;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().with_env_filter("info").init();

    let origin = moq_lite::Origin::random().produce();
    let consumer = origin.consume();

    // Run subscriber first so it's ready when publisher starts announcing.
    // publish() finishes after all groups are sent; subscribe() finishes
    // when the track is closed.
    tokio::try_join!(publish(origin), subscribe(consumer))?;
    Ok(())
}

async fn publish(origin: moq_lite::OriginProducer) -> anyhow::Result<()> {
    let mut broadcast = moq_lite::Broadcast::new().produce();

    let mut track = broadcast.create_track(moq_lite::Track {
        name: "video".into(),
    })?;

    origin.publish_broadcast("cam/front", broadcast.consume());
    tracing::info!("published broadcast cam/front with track 'video'");

    for group_seq in 0..NUM_GROUPS {
        let mut group = track.append_group()?;

        for frame_idx in 0..FRAMES_PER_GROUP {
            // 'I' = keyframe, 'P' = delta frame
            let tag = if frame_idx == 0 { b'I' } else { b'P' };
            group.write_frame(Bytes::from(vec![tag; FRAME_SIZE]))?;
        }

        group.finish()?;
        tracing::info!(
            group = group_seq,
            "published group ({FRAMES_PER_GROUP} frames)"
        );

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    drop(track);
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    Ok(())
}

async fn subscribe(consumer: moq_lite::OriginConsumer) -> anyhow::Result<()> {
    let path: moq_lite::Path<'_> = "cam/front".into();
    let mut origin = consumer
        .consume_only(&[path])
        .ok_or_else(|| anyhow::anyhow!("failed to consume origin"))?;

    tracing::info!("waiting for broadcast...");

    while let Some((path, maybe_broadcast)) = origin.announced().await {
        let broadcast = match maybe_broadcast {
            Some(b) => b,
            None => {
                tracing::warn!(%path, "broadcast went offline");
                continue;
            }
        };

        tracing::info!(%path, "broadcast online, subscribing to 'video'");

        let mut track = broadcast.subscribe_track(
            &moq_lite::Track {
                name: "video".into(),
            },
            moq_lite::Subscription::default(),
        )?;

        let mut group_count = 0u64;
        while let Ok(Some(mut group)) = track.recv_group().await {
            let mut frame_count = 0usize;
            let mut total_bytes = 0usize;

            while let Some(frame) = group.read_frame().await? {
                total_bytes += frame.len();
                frame_count += 1;
            }

            tracing::info!(
                group = group_count,
                frames = frame_count,
                bytes = total_bytes,
                "received group"
            );
            group_count += 1;
        }

        tracing::info!("track ended, total groups: {group_count}");
        return Ok(());
    }

    Ok(())
}
