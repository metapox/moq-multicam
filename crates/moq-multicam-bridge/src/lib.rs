mod ffmpeg_source;
#[cfg(feature = "openh264")]
mod file_source;
#[cfg(feature = "openh264")]
mod openh264_source;
mod source;
mod test_source;
#[cfg(feature = "v4l")]
mod v4l_source;

pub use ffmpeg_source::FfmpegSource;
#[cfg(feature = "openh264")]
pub use file_source::FileSource;
#[cfg(feature = "openh264")]
pub use openh264_source::OpenH264Source;
pub use source::{SourceConfig, VideoSource};
pub use test_source::TestSource;
#[cfg(feature = "v4l")]
pub use v4l_source::V4lSource;
