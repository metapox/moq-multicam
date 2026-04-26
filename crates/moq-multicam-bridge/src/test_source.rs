//! Dummy video source for development and testing.
//!
//! Generates fake keyframe (I) and delta (P) frames at a configurable
//! frame rate. No real encoding — just tagged byte buffers.

use std::time::Duration;

use bytes::Bytes;
use moq_multicam_core::TrackProducer;

/// Simulates a camera by writing dummy frames to a moq-lite Track.
pub struct TestSource {
    frame_size: usize,
    fps: u32,
    gop_size: u32,
}

impl TestSource {
    pub fn new(fps: u32, frame_size: usize, gop_size: u32) -> Self {
        Self { frame_size, fps, gop_size }
    }

    /// Run the test source, writing Groups to the given Track until cancelled.
    pub async fn run(self, mut track: TrackProducer) -> anyhow::Result<()> {
        let frame_interval = Duration::from_secs_f64(1.0 / self.fps as f64);
        let mut group_seq = 0u64;

        loop {
            let mut group = track.append_group()?;

            for i in 0..self.gop_size {
                // 'I' for keyframe (first frame), 'P' for delta frames
                let tag = if i == 0 { b'I' } else { b'P' };
                let payload = Bytes::from(vec![tag; self.frame_size]);
                group.write_frame(payload)?;
                tokio::time::sleep(frame_interval).await;
            }

            group.finish()?;
            tracing::debug!(group = group_seq, "test source: published group");
            group_seq += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moq_multicam_core::{CameraConfig, create_camera_broadcast};

    #[tokio::test]
    async fn test_source_produces_frames() {
        let cameras = vec![CameraConfig { name: "front".into(), priority: 0 }];
        let (_broadcast, mut tracks) = create_camera_broadcast("test-vehicle", &cameras).unwrap();
        let track_producer = tracks.remove(0);

        // Subscribe to the track before publishing
        let consumer = _broadcast.consume();
        let mut track_consumer = consumer.subscribe_track(&moq_lite::Track {
            name: "camera/front/video".into(),
            priority: 0,
        }).unwrap();

        let source = TestSource::new(30, 128, 3);

        // Run source for a short time, then check output
        let handle = tokio::spawn(source.run(track_producer));

        let mut group = track_consumer.recv_group().await.unwrap().unwrap();
        let mut frame_count = 0;
        while let Some(frame) = group.read_frame().await.unwrap() {
            if frame_count == 0 {
                assert_eq!(frame[0], b'I');
            } else {
                assert_eq!(frame[0], b'P');
            }
            assert_eq!(frame.len(), 128);
            frame_count += 1;
        }
        assert_eq!(frame_count, 3);

        handle.abort();
    }
}
