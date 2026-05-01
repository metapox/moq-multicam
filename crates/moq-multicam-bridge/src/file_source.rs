//! File-based video source — decodes a video file via ffmpeg, encodes with openh264.
//!
//! Loops the video forever. Requires ffmpeg in PATH and openh264 feature.

use anyhow::Result;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use openh264::encoder::{Encoder, EncoderConfig};
use openh264::formats::YUVBuffer;

use crate::source::VideoSource;

pub struct FileSource {
    path: String,
    width: u32,
    height: u32,
    fps: u32,
    _bitrate_kbps: u32,
}

impl FileSource {
    pub fn new(path: &str, width: u32, height: u32, fps: u32, bitrate_kbps: u32) -> Self {
        Self { path: path.to_string(), width, height, fps, _bitrate_kbps: bitrate_kbps }
    }
}

impl VideoSource for FileSource {
    async fn run(self, mut producer: hang::container::OrderedProducer) -> Result<()> {
        if !std::path::Path::new(&self.path).exists() {
            anyhow::bail!("video file not found: {}", self.path);
        }

        let w = self.width as usize;
        let h = self.height as usize;
        let frame_size = w * h * 3; // RGB24
        let frame_interval = Duration::from_secs_f64(1.0 / self.fps as f64);
        let gop_size = self.fps as u64;

        let config = EncoderConfig::new(self.width, self.height);
        let mut encoder = Encoder::with_config(config)?;

        tracing::info!(path = %self.path, w, h, fps = self.fps, "file source started");

        let mut frame_num: u64 = 0;

        loop {
            let mut child = Command::new("ffmpeg")
                .args([
                    "-re",
                    "-stream_loop", "-1",
                    "-i", &self.path,
                    "-f", "rawvideo",
                    "-pix_fmt", "rgb24",
                    "-s", &format!("{}x{}", self.width, self.height),
                    "-r", &format!("{}", self.fps),
                    "-an",
                    "-",
                ])
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()?;

            let mut stdout = child.stdout.take().unwrap();
            let mut buf = vec![0u8; frame_size];

            loop {
                if let Err(_) = stdout.read_exact(&mut buf).await {
                    break;
                }

                let is_gop_start = frame_num % gop_size == 0;
                let rgb = buf.clone();

                let result = tokio::task::spawn_blocking(move || -> Result<_, openh264::Error> {
                    let yuv = YUVBuffer::with_rgb(w, h, &rgb);
                    if is_gop_start {
                        unsafe { encoder.raw_api().force_intra_frame(true); }
                    }
                    let bs = encoder.encode(&yuv)?;
                    let data = bs.to_vec();
                    let ft = bs.frame_type();
                    drop(bs);
                    Ok((data, encoder, ft))
                }).await??;

                let (annexb, enc, frame_type) = result;
                encoder = enc;

                if annexb.is_empty() {
                    frame_num += 1;
                    tokio::time::sleep(frame_interval).await;
                    continue;
                }

                if matches!(frame_type, openh264::encoder::FrameType::IDR) {
                    let _ = producer.keyframe();
                }

                let pts = frame_num * 1_000_000 / self.fps as u64;
                producer.write(hang::container::Frame {
                    timestamp: hang::container::Timestamp::from_micros(pts)?,
                    payload: bytes::Bytes::from(annexb).into(),
                })?;

                frame_num += 1;
                tokio::time::sleep(frame_interval).await;
            }

            let _ = child.kill().await;
            tracing::info!(path = %self.path, "video ended, restarting");
        }
    }
}
