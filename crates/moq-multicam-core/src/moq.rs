//! Thin wrapper over moq-lite.
//!
//! All crates access moq-lite through here so upstream breaking changes
//! are absorbed in one place.

pub use moq_lite::{
    Broadcast, BroadcastProducer,
    GroupProducer,
    Origin, OriginConsumer, OriginProducer,
    Path, Track, TrackConsumer, TrackProducer,
};

use crate::{CameraConfig, Quality, TrackPath};

/// Create a broadcast with one Track per camera (high quality).
/// Returns the producer and the broadcast path for publishing.
pub fn create_camera_broadcast(
    vehicle_id: &str,
    cameras: &[CameraConfig],
) -> anyhow::Result<(BroadcastProducer, Vec<TrackProducer>)> {
    let mut broadcast = Broadcast::produce();
    let mut tracks = Vec::with_capacity(cameras.len());

    for cam in cameras {
        let path = TrackPath::camera(vehicle_id, &cam.name, Quality::High);
        let track = broadcast.create_track(Track {
            name: path.track_name(),
            priority: cam.priority,
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
        let cameras = vec![
            CameraConfig { name: "front".into(), priority: 0 },
            CameraConfig { name: "rear".into(), priority: 1 },
        ];

        let (broadcast, tracks) = create_camera_broadcast("truck-01", &cameras).unwrap();
        assert_eq!(tracks.len(), 2);

        // Broadcast should be consumable
        let _consumer = broadcast.consume();
    }
}
