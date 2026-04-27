//! Spawn ffmpeg as a child process and pipe fMP4 output into moq-mux.
//!
//! Each FfmpegSource manages one ffmpeg process producing fMP4 on stdout,
//! decoded by moq-mux's import::Fmp4 into a Broadcast.

use anyhow::Result;
use tokio::process::Command;

/// Spawns ffmpeg to generate a test pattern as fMP4 on stdout.
pub struct FfmpegSource {
    width: u32,
    height: u32,
    fps: u32,
}

impl FfmpegSource {
    pub fn new(width: u32, height: u32, fps: u32) -> Self {
        Self { width, height, fps }
    }

    /// Spawn ffmpeg and pipe its fMP4 output into the given Fmp4 decoder.
    /// Runs until ffmpeg exits or the task is cancelled.
    pub async fn run(self, mut fmp4: moq_mux::import::Fmp4) -> Result<()> {
        let size = format!("{}x{}", self.width, self.height);

        let mut child = Command::new("ffmpeg")
            .args([
                "-hide_banner", "-v", "quiet",
                "-f", "lavfi",
                "-i", &format!("testsrc=size={size}:rate={}", self.fps),
                "-c:v", "libx264",
                "-preset", "ultrafast",
                "-tune", "zerolatency",
                "-g", &self.fps.to_string(),
                "-f", "mp4",
                "-movflags", "cmaf+separate_moof+delay_moov+skip_trailer+frag_every_frame",
                "-flush_packets", "1",
                "-",
            ])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        let stdout = child.stdout.take().ok_or_else(|| anyhow::anyhow!("no stdout"))?;
        let mut reader = tokio::io::BufReader::new(stdout);
        let mut buffer = bytes::BytesMut::new();

        loop {
            let n = tokio::io::AsyncReadExt::read_buf(&mut reader, &mut buffer).await?;
            if n == 0 {
                tracing::info!("ffmpeg exited");
                break;
            }
            fmp4.decode(&mut buffer)?;
        }

        let status = child.wait().await?;
        if !status.success() {
            anyhow::bail!("ffmpeg exited with {status}");
        }

        Ok(())
    }
}
