mod camera;
mod moq;
mod track;

pub use camera::CameraConfig;
pub use moq::create_camera_broadcast;
pub use track::{Quality, TrackKind, TrackPath};

// Re-export moq-lite types through core for API stability.
pub use moq::{
    Broadcast, BroadcastProducer, GroupProducer, Origin, OriginConsumer, OriginProducer, Path,
    Subscription, Track, TrackConsumer, TrackProducer,
};
