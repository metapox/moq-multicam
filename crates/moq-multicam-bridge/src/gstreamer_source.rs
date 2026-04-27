//! GStreamer-based video source — writes H.264 directly to hang OrderedProducer.
//!
//! No ffmpeg, no import::Fmp4. GStreamer encodes H.264 (Annex B), and we
//! write it straight into a hang track using Container::Legacy + avc3 (inline SPS/PPS).

use anyhow::{Context, Result};
use gstreamer::prelude::*;
use gstreamer_app::AppSink;

pub struct GstreamerSource {
    width: u32,
    height: u32,
    fps: u32,
}

impl GstreamerSource {
    pub fn new(width: u32, height: u32, fps: u32) -> Self {
        Self { width, height, fps }
    }

    /// Run the GStreamer pipeline and write H.264 frames to the given OrderedProducer.
    pub async fn run(self, mut producer: hang::container::OrderedProducer) -> Result<()> {
        gstreamer::init().context("failed to init GStreamer")?;

        let pipeline_str = format!(
            "videotestsrc is-live=true ! \
             video/x-raw,width={w},height={h},framerate={fps}/1 ! \
             x264enc tune=zerolatency speed-preset=ultrafast key-int-max={fps} ! \
             h264parse config-interval=1 ! \
             video/x-h264,stream-format=byte-stream,alignment=au ! \
             appsink name=sink sync=false emit-signals=false",
            w = self.width, h = self.height, fps = self.fps,
        );

        let pipeline = gstreamer::parse::launch(&pipeline_str)
            .context("failed to parse GStreamer pipeline")?
            .downcast::<gstreamer::Pipeline>()
            .map_err(|_| anyhow::anyhow!("not a pipeline"))?;

        let sink = pipeline
            .by_name("sink")
            .context("appsink not found")?
            .downcast::<AppSink>()
            .map_err(|_| anyhow::anyhow!("not an appsink"))?;

        pipeline.set_state(gstreamer::State::Playing)
            .context("failed to start pipeline")?;

        tracing::info!("GStreamer pipeline started (direct hang write)");

        let result = tokio::task::spawn_blocking(move || -> Result<()> {
            loop {
                let sample = match sink.pull_sample() {
                    Ok(s) => s,
                    Err(_) => {
                        tracing::info!("GStreamer EOS");
                        break;
                    }
                };

                let buf = sample.buffer().context("no buffer")?;
                let pts = buf.pts().map(|p| p.useconds()).unwrap_or(0);
                let map = buf.map_readable().context("map failed")?;
                let data = map.as_slice();

                if is_keyframe(data) {
                    producer.keyframe();
                }

                let frame = hang::container::Frame {
                    timestamp: hang::container::Timestamp::from_micros(pts)
                        .context("invalid timestamp")?,
                    payload: bytes::Bytes::copy_from_slice(data).into(),
                };
                producer.write(frame)?;
            }
            Ok(())
        })
        .await?;

        pipeline.set_state(gstreamer::State::Null).ok();
        result
    }
}

/// Check if an Annex B H.264 access unit contains an IDR slice (NAL type 5).
fn is_keyframe(data: &[u8]) -> bool {
    let mut i = 0;
    while i < data.len().saturating_sub(4) {
        // Look for start codes: 00 00 00 01 or 00 00 01
        if data[i] == 0 && data[i + 1] == 0 {
            let (nal_start, sc_len) = if data[i + 2] == 1 {
                (i + 3, 3)
            } else if data[i + 2] == 0 && i + 3 < data.len() && data[i + 3] == 1 {
                (i + 4, 4)
            } else {
                i += 1;
                continue;
            };
            if nal_start < data.len() {
                let nal_type = data[nal_start] & 0x1F;
                if nal_type == 5 {
                    return true; // IDR slice
                }
            }
            i += sc_len;
        } else {
            i += 1;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyframe_detection() {
        // IDR frame (NAL type 5) with 4-byte start code
        let idr = [0x00, 0x00, 0x00, 0x01, 0x65, 0xAA, 0xBB];
        assert!(is_keyframe(&idr));

        // Non-IDR frame (NAL type 1) with 4-byte start code
        let non_idr = [0x00, 0x00, 0x00, 0x01, 0x41, 0xAA, 0xBB];
        assert!(!is_keyframe(&non_idr));

        // SPS (type 7) + PPS (type 8) + IDR (type 5)
        let sps_pps_idr = [
            0x00, 0x00, 0x00, 0x01, 0x67, 0x42, 0x00, 0x28, // SPS
            0x00, 0x00, 0x00, 0x01, 0x68, 0xCE, 0x38, 0x80, // PPS
            0x00, 0x00, 0x00, 0x01, 0x65, 0x88, 0x80, 0x40, // IDR
        ];
        assert!(is_keyframe(&sps_pps_idr));
    }
}
