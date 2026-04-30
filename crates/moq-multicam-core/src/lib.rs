mod track;
mod camera;
mod moq;

pub use track::{TrackPath, TrackKind, Quality};
pub use camera::CameraConfig;
pub use moq::create_camera_broadcast;

// Re-export moq-lite types through core for API stability.
pub use moq::{
    Broadcast, BroadcastProducer,
    GroupProducer,
    Origin, OriginConsumer, OriginProducer,
    Path, Subscription, Track, TrackConsumer, TrackProducer,
};
