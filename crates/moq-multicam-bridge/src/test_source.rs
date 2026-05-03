//! Dummy video source for development and testing.
//!
//! Generates fake keyframe (I) and delta (P) frames at a configurable
//! frame rate. No real encoding — just tagged byte buffers.

use std::time::Duration;

use anyhow::Result;
use bytes::Bytes;
use hang::container::OrderedProducer;

use crate::source::VideoSource;

/// Simulates a camera by writing dummy frames to a hang OrderedProducer.
pub struct TestSource {
    frame_size: usize,
    fps: u32,
    gop_size: u32,
}

impl TestSource {
    pub fn new(fps: u32, frame_size: usize, gop_size: u32) -> Self {
        Self {
            frame_size,
            fps,
            gop_size,
        }
    }
}

impl VideoSource for TestSource {
    async fn run(self, mut producer: OrderedProducer) -> Result<()> {
        let frame_interval = Duration::from_secs_f64(1.0 / self.fps as f64);
        let mut pts: u64 = 0;

        loop {
            let _ = producer.keyframe();

            for i in 0..self.gop_size {
                let tag = if i == 0 { b'I' } else { b'P' };
                let payload = Bytes::from(vec![tag; self.frame_size]);
                let _ = producer.write(hang::container::Frame {
                    timestamp: hang::container::Timestamp::from_micros(pts)?,
                    payload: payload.into(),
                });
                pts += 1_000_000 / self.fps as u64;
                tokio::time::sleep(frame_interval).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_source_produces_frames() {
        let mut broadcast = moq_lite::Broadcast::new().produce();
        let track = broadcast
            .create_track(moq_lite::Track {
                name: "video".into(),
            })
            .unwrap();

        let producer = OrderedProducer::new(track);
        let source = TestSource::new(30, 128, 3);

        let consumer = broadcast.consume();
        let mut track_consumer = consumer
            .subscribe_track(
                &moq_lite::Track {
                    name: "video".into(),
                },
                moq_lite::Subscription::default(),
            )
            .unwrap();

        let handle = tokio::spawn(source.run(producer));

        let mut group = track_consumer.recv_group().await.unwrap().unwrap();
        let mut frame_count = 0;
        while let Some(_frame) = group.read_frame().await.unwrap() {
            frame_count += 1;
        }
        assert_eq!(frame_count, 3);

        handle.abort();
    }
}
