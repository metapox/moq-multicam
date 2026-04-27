mod source;
mod test_source;
mod ffmpeg_source;
#[cfg(feature = "gstreamer")]
mod gstreamer_source;

pub use source::{VideoSource, SourceConfig};
pub use test_source::TestSource;
pub use ffmpeg_source::FfmpegSource;
#[cfg(feature = "gstreamer")]
pub use gstreamer_source::GstreamerSource;
