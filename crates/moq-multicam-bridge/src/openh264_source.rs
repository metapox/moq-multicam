//! OpenH264-based video source — lightweight H.264 encoding without GStreamer.
//!
//! Generates a color-cycling test pattern, encodes with openh264, and writes
//! H.264 Annex B NAL units to a hang OrderedProducer.

use std::time::Duration;

use anyhow::Result;
use openh264::encoder::{Encoder, EncoderConfig};
use openh264::formats::YUVBuffer;

use crate::source::VideoSource;

pub struct OpenH264Source {
    width: u32,
    height: u32,
    fps: u32,
    bitrate_kbps: u32,
}

impl OpenH264Source {
    pub fn new(width: u32, height: u32, fps: u32, bitrate_kbps: u32) -> Self {
        Self { width, height, fps, bitrate_kbps }
    }
}

impl VideoSource for OpenH264Source {
    async fn run(self, mut producer: hang::container::OrderedProducer) -> Result<()> {
        let w = self.width as usize;
        let h = self.height as usize;
        let frame_interval = Duration::from_secs_f64(1.0 / self.fps as f64);

        let config = EncoderConfig::new(self.width, self.height);
        let mut encoder = Encoder::with_config(config)?;

        let mut frame_num: u64 = 0;

        tracing::info!(w = self.width, h = self.height, fps = self.fps, "openh264 source started");

        loop {
            let rgb = generate_test_rgb(w, h, frame_num);
            let yuv = YUVBuffer::with_rgb(w, h, &rgb);

            let bitstream = tokio::task::spawn_blocking(move || -> Result<_, openh264::Error> {
                let bs = encoder.encode(&yuv)?;
                let annexb = bs.to_vec();
                let ft = bs.frame_type();
                drop(bs);
                Ok((annexb, encoder, ft))
            }).await??;

            let (annexb, enc, frame_type) = bitstream;
            encoder = enc;

            // openh264 decides keyframes internally
            let is_idr = matches!(frame_type, openh264::encoder::FrameType::IDR);
            if is_idr {
                producer.keyframe();
            }

            let pts = frame_num * 1_000_000 / self.fps as u64;
            producer.write(hang::container::Frame {
                timestamp: hang::container::Timestamp::from_micros(pts)?,
                payload: bytes::Bytes::from(annexb).into(),
            })?;

            frame_num += 1;
            tokio::time::sleep(frame_interval).await;
        }
    }
}

/// Generate an RGB test pattern — static color bars with a moving indicator line.
fn generate_test_rgb(w: usize, h: usize, frame: u64) -> Vec<u8> {
    let mut rgb = vec![0u8; w * h * 3];

    // Static 8 vertical color bars
    for row in 0..h {
        for col in 0..w {
            let bar = col * 8 / w;
            let (r, g, b) = COLOR_BARS_RGB[bar];
            let idx = (row * w + col) * 3;
            rgb[idx] = r;
            rgb[idx + 1] = g;
            rgb[idx + 2] = b;
        }
    }

    // Thin horizontal white line that moves down slowly (1 row per frame)
    let line_row = (frame as usize) % h;
    for col in 0..w {
        let idx = (line_row * w + col) * 3;
        rgb[idx] = 255;
        rgb[idx + 1] = 255;
        rgb[idx + 2] = 255;
    }

    rgb
}

// Standard 8-bar color pattern (RGB)
const COLOR_BARS_RGB: [(u8, u8, u8); 8] = [
    (255, 255, 255), // white
    (255, 255, 0),   // yellow
    (0, 255, 255),   // cyan
    (0, 255, 0),     // green
    (255, 0, 255),   // magenta
    (255, 0, 0),     // red
    (0, 0, 255),     // blue
    (0, 0, 0),       // black
];
