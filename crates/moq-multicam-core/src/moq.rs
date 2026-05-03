//! Thin wrapper over moq-lite.
//!
//! All crates access moq-lite through here so upstream breaking changes
//! are absorbed in one place.

pub use moq_lite::{
    Broadcast, BroadcastProducer, GroupProducer, Origin, OriginConsumer, OriginProducer, Path,
    Subscription, Track, TrackConsumer, TrackProducer,
};

use crate::{CameraConfig, Quality};

/// Create a broadcast per camera with a video track.
/// Returns the producer and tracks for each camera.
pub fn create_camera_broadcast(
    _vehicle_id: &str,
    cameras: &[CameraConfig],
) -> anyhow::Result<(BroadcastProducer, Vec<TrackProducer>)> {
    let mut broadcast = Broadcast::default().produce();
    let mut tracks = Vec::with_capacity(cameras.len());

    for _cam in cameras {
        let track = broadcast.create_track(Track {
            name: Quality::High.track_name().to_string(),
        })?;
        tracks.push(track);
    }

    Ok((broadcast, tracks))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CameraConfig;

    #[test]
    fn create_broadcast_with_cameras() {
        let cameras = vec![CameraConfig {
            name: "front".into(),
            priority: 0,
        }];

        let (broadcast, tracks) = create_camera_broadcast("truck-01", &cameras).unwrap();
        assert_eq!(tracks.len(), 1);
        let _consumer = broadcast.consume();
    }
}
