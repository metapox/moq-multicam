mod source;
mod test_source;
mod ffmpeg_source;
#[cfg(feature = "openh264")]
mod file_source;
#[cfg(feature = "openh264")]
mod openh264_source;
#[cfg(feature = "v4l")]
mod v4l_source;

pub use source::{VideoSource, SourceConfig};
pub use test_source::TestSource;
pub use ffmpeg_source::FfmpegSource;
#[cfg(feature = "openh264")]
pub use file_source::FileSource;
#[cfg(feature = "openh264")]
pub use openh264_source::OpenH264Source;
#[cfg(feature = "v4l")]
pub use v4l_source::V4lSource;
