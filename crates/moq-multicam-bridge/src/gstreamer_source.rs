//! GStreamer-based video source.
//!
//! GStreamer handles video capture and H.264 encoding. The encoded stream
//! is piped through ffmpeg for CMAF fMP4 muxing (GStreamer's built-in
//! mp4mux doesn't produce the format moq-mux expects).

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

    pub async fn run(self, mut fmp4: moq_mux::import::Fmp4) -> Result<()> {
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

        tracing::info!("GStreamer pipeline started");

        // ffmpeg remuxes raw H.264 → CMAF fMP4
        let mut ffmpeg = std::process::Command::new("ffmpeg")
            .args([
                "-hide_banner", "-v", "quiet",
                "-f", "h264", "-i", "pipe:0",
                "-c:v", "copy",
                "-f", "mp4",
                "-movflags", "cmaf+separate_moof+delay_moov+skip_trailer+frag_every_frame",
                "-flush_packets", "1",
                "pipe:1",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .context("failed to spawn ffmpeg for remux")?;

        let mut ffmpeg_stdin = ffmpeg.stdin.take().context("no ffmpeg stdin")?;
        let ffmpeg_stdout = ffmpeg.stdout.take().context("no ffmpeg stdout")?;

        // Thread 1: GStreamer appsink → ffmpeg stdin
        let writer = std::thread::spawn(move || -> Result<()> {
            use std::io::Write;
            loop {
                let sample = match sink.pull_sample() {
                    Ok(s) => s,
                    Err(_) => break, // EOS
                };
                let buf = sample.buffer().context("no buffer")?;
                let map = buf.map_readable().context("map failed")?;
                ffmpeg_stdin.write_all(&map)?;
            }
            Ok(())
        });

        // Thread 2: ffmpeg stdout → fmp4 decoder
        let reader = tokio::task::spawn_blocking(move || -> Result<()> {
            use std::io::Read;
            let mut reader = std::io::BufReader::new(ffmpeg_stdout);
            let mut tmp = [0u8; 8192];
            let mut buffer = bytes::BytesMut::new();
            loop {
                let n = reader.read(&mut tmp)?;
                if n == 0 { break; }
                buffer.extend_from_slice(&tmp[..n]);
                fmp4.decode(&mut buffer)?;
            }
            Ok(())
        });

        let result = reader.await?;

        pipeline.set_state(gstreamer::State::Null).ok();
        writer.join().map_err(|_| anyhow::anyhow!("writer thread panicked"))??;
        ffmpeg.wait()?;

        result
    }
}
