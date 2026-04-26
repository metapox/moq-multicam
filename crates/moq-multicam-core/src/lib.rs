mod track;
mod camera;
mod moq;

pub use track::{TrackPath, TrackKind, Quality};
pub use camera::CameraConfig;
pub use moq::create_camera_broadcast;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
