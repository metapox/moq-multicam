//! Unified video source trait for capture + encode backends.

use anyhow::Result;
use hang::container::OrderedProducer;

/// Configuration for a video source rendition.
#[derive(Debug, Clone)]
pub struct SourceConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_kbps: u32,
}

/// Unified interface for video capture + encode backends.
///
/// Implementors produce H.264 frames and write them to a hang OrderedProducer.
/// Call `producer.keyframe()` before each IDR frame.
/// Returns when the source stops or an error occurs.
pub trait VideoSource: Send + 'static {
    fn run(self, producer: OrderedProducer)
        -> impl std::future::Future<Output = Result<()>> + Send;
}
