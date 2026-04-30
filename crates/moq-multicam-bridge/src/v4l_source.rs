//! V4L2 camera capture + openh264 encode.
//!
//! Captures raw frames from a Linux V4L2 device (e.g. /dev/video0),
//! converts to YUV420, encodes with openh264, and writes H.264 Annex B
//! to a hang OrderedProducer.

use std::time::Duration;

use anyhow::{Context, Result};
use openh264::encoder::{Encoder, EncoderConfig};
use openh264::formats::YUVBuffer;
use v4l::buffer::Type;
use v4l::io::traits::CaptureStream;
use v4l::prelude::*;
use v4l::FourCC;

use crate::source::VideoSource;

pub struct V4lSource {
    device_path: String,
    width: u32,
    height: u32,
    fps: u32,
    bitrate_kbps: u32,
}

impl V4lSource {
    pub fn new(device_path: &str, width: u32, height: u32, fps: u32, bitrate_kbps: u32) -> Self {
        Self {
            device_path: device_path.to_string(),
            width, height, fps, bitrate_kbps,
        }
    }
}

impl VideoSource for V4lSource {
    async fn run(self, mut producer: hang::container::OrderedProducer) -> Result<()> {
        let w = self.width as usize;
        let h = self.height as usize;

        let config = EncoderConfig::new(self.width, self.height);
        let mut encoder = Encoder::with_config(config)?;

        // V4L2 is blocking I/O, run in spawn_blocking
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut dev = Device::with_path(&self.device_path)
                .with_context(|| format!("failed to open {}", self.device_path))?;

            // Request YUYV format (widely supported by USB cameras)
            let mut fmt = dev.format().context("failed to get format")?;
            fmt.width = self.width;
            fmt.height = self.height;
            fmt.fourcc = FourCC::new(b"YUYV");
            dev.set_format(&fmt).context("failed to set format")?;

            let actual = dev.format()?;
            tracing::info!(
                device = %self.device_path,
                w = actual.width, h = actual.height,
                fourcc = %actual.fourcc,
                "v4l2 capture started"
            );

            let mut stream = MmapStream::with_buffers(&mut dev, Type::VideoCapture, 4)
                .context("failed to create mmap stream")?;

            let mut frame_num: u64 = 0;
            let gop_size = self.fps as u64;

            loop {
                let (buf, _meta) = stream.next().context("capture failed")?;

                // Convert YUYV to RGB, then to YUV420 via openh264
                let rgb = yuyv_to_rgb(buf, w, h);
                let yuv = YUVBuffer::with_rgb(w, h, &rgb);

                let bs = encoder.encode(&yuv)?;
                let annexb = bs.to_vec();
                let is_idr = matches!(bs.frame_type(), openh264::encoder::FrameType::IDR);
                drop(bs);

                if is_idr {
                    producer.keyframe();
                }

                let pts = frame_num * 1_000_000 / self.fps as u64;
                producer.write(hang::container::Frame {
                    timestamp: hang::container::Timestamp::from_micros(pts)?,
                    payload: bytes::Bytes::from(annexb).into(),
                })?;

                frame_num += 1;
            }
        }).await?
    }
}

/// Convert YUYV (YUV 4:2:2 packed) to RGB24.
fn yuyv_to_rgb(yuyv: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut rgb = vec![0u8; w * h * 3];
    for i in 0..(w * h / 2) {
        let y0 = yuyv[i * 4] as f32;
        let u = yuyv[i * 4 + 1] as f32 - 128.0;
        let y1 = yuyv[i * 4 + 2] as f32;
        let v = yuyv[i * 4 + 3] as f32 - 128.0;

        let r0 = (y0 + 1.402 * v).clamp(0.0, 255.0) as u8;
        let g0 = (y0 - 0.344 * u - 0.714 * v).clamp(0.0, 255.0) as u8;
        let b0 = (y0 + 1.772 * u).clamp(0.0, 255.0) as u8;

        let r1 = (y1 + 1.402 * v).clamp(0.0, 255.0) as u8;
        let g1 = (y1 - 0.344 * u - 0.714 * v).clamp(0.0, 255.0) as u8;
        let b1 = (y1 + 1.772 * u).clamp(0.0, 255.0) as u8;

        let idx = i * 6;
        rgb[idx] = r0; rgb[idx + 1] = g0; rgb[idx + 2] = b0;
        rgb[idx + 3] = r1; rgb[idx + 4] = g1; rgb[idx + 5] = b1;
    }
    rgb
}
