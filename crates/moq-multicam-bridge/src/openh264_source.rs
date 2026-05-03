//! OpenH264-based video source — lightweight H.264 encoding.
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
    // TODO: openh264 0.4 does not expose bitrate config. Upgrade to 0.8+ to use
    // EncoderConfig::bitrate(BitRate::from_bps(bitrate_kbps * 1000)).
    _bitrate_kbps: u32,
    camera_index: u8,
}

impl OpenH264Source {
    pub fn new(width: u32, height: u32, fps: u32, bitrate_kbps: u32) -> Self {
        Self {
            width,
            height,
            fps,
            _bitrate_kbps: bitrate_kbps,
            camera_index: 0,
        }
    }

    pub fn with_index(mut self, index: u8) -> Self {
        self.camera_index = index;
        self
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
        let gop_size = self.fps as u64; // keyframe every second

        tracing::info!(
            w = self.width,
            h = self.height,
            fps = self.fps,
            "openh264 source started"
        );

        loop {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let rgb = generate_test_rgb(w, h, frame_num, self.camera_index, now_ms);
            let yuv = YUVBuffer::with_rgb(w, h, &rgb);

            let is_gop_start = frame_num % gop_size == 0;

            let bitstream = tokio::task::spawn_blocking(move || -> Result<_, openh264::Error> {
                // Force IDR at GOP boundary
                if is_gop_start {
                    // SAFETY: force_intra_frame is a simple boolean setter on the initialized encoder.
                    // The encoder is exclusively owned within this spawn_blocking closure.
                    unsafe {
                        encoder.raw_api().force_intra_frame(true);
                    }
                }
                let bs = encoder.encode(&yuv)?;
                let annexb = bs.to_vec();
                let ft = bs.frame_type();
                Ok((annexb, encoder, ft))
            })
            .await??;

            let (annexb, enc, frame_type) = bitstream;
            encoder = enc;

            // Start new Group on actual IDR frames
            if matches!(frame_type, openh264::encoder::FrameType::IDR)
                && producer.keyframe().is_err()
            {
                tracing::warn!("producer closed, stopping openh264 source");
                break Ok(());
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

/// Generate an RGB test pattern — unique color per camera with scan line and timestamp.
fn generate_test_rgb(
    w: usize,
    h: usize,
    frame: u64,
    camera_index: u8,
    timestamp_ms: u64,
) -> Vec<u8> {
    let mut rgb = vec![0u8; w * h * 3];
    let (base_r, base_g, base_b) = CAMERA_COLORS[camera_index as usize % CAMERA_COLORS.len()];

    for row in 0..h {
        for col in 0..w {
            let idx = (row * w + col) * 3;

            // Gradient: brighter at top, darker at bottom
            let brightness = 255 - (row * 180 / h) as u8;
            rgb[idx] = (base_r as u16 * brightness as u16 / 255) as u8;
            rgb[idx + 1] = (base_g as u16 * brightness as u16 / 255) as u8;
            rgb[idx + 2] = (base_b as u16 * brightness as u16 / 255) as u8;

            // Vertical grid lines every 1/4 width
            if col % (w / 4) < 2 {
                rgb[idx] = 255;
                rgb[idx + 1] = 255;
                rgb[idx + 2] = 255;
            }
        }
    }

    // Embed wall-clock timestamp (ms) as binary pixels in top-left 48 pixels.
    // Each pixel: white (255) = bit 1, black (0) = bit 0.
    // 48 bits covers ~8900 years of milliseconds.
    for bit in 0..48.min(w) {
        let val = if (timestamp_ms >> (47 - bit)) & 1 == 1 {
            255u8
        } else {
            0u8
        };
        let idx = bit * 3;
        rgb[idx] = val;
        rgb[idx + 1] = val;
        rgb[idx + 2] = val;
    }

    // Horizontal scan line
    let line_row = (frame as usize) % h;
    for col in 0..w {
        let idx = (line_row * w + col) * 3;
        rgb[idx] = 255;
        rgb[idx + 1] = 255;
        rgb[idx + 2] = 255;
    }

    rgb
}

// Distinct colors for up to 8 cameras
const CAMERA_COLORS: [(u8, u8, u8); 8] = [
    (66, 133, 244),  // blue
    (234, 67, 53),   // red
    (52, 168, 83),   // green
    (251, 188, 4),   // yellow
    (171, 71, 188),  // purple
    (0, 172, 193),   // teal
    (255, 112, 67),  // orange
    (158, 158, 158), // gray
];
