use anyhow::Result;
use clap::Parser;
use url::Url;

mod publish_fmp4;
mod subscribe;

#[derive(Parser)]
#[command(name = "moq-multicam", version, about = "Multi-camera streaming over MoQ")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Publish fMP4 to a relay. Use --broadcast for stdin pipe, or --camera for built-in source.
    PublishFmp4 {
        #[arg(long, default_value = "https://localhost:4443")]
        relay: Url,

        /// Single camera: broadcast path for stdin pipe
        #[arg(long, conflicts_with_all = ["camera", "vehicle", "source"])]
        broadcast: Option<String>,

        /// Multi-camera: camera names (can be repeated)
        #[arg(long)]
        camera: Vec<String>,

        /// Vehicle ID (used with --camera)
        #[arg(long, default_value = "truck-01")]
        vehicle: String,

        /// Video source backend: ffmpeg or gstreamer
        #[arg(long, default_value = "ffmpeg")]
        source: String,

        #[arg(long)]
        tls_disable_verify: bool,
    },
    /// Subscribe to cameras from a relay
    Subscribe {
        #[arg(long, default_value = "https://localhost:4443")]
        relay: Url,
        #[arg(long, default_value = "truck-01")]
        vehicle: String,
        #[arg(long, default_value = "front,rear")]
        cameras: String,
        #[arg(long)]
        tls_disable_verify: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Command::PublishFmp4 {
            relay, broadcast, camera, vehicle, source, tls_disable_verify,
        } => {
            if let Some(broadcast_path) = broadcast {
                publish_fmp4::run_stdin(relay, &broadcast_path, tls_disable_verify).await
            } else if !camera.is_empty() {
                let cameras: Vec<_> = camera.iter().enumerate().map(|(i, name)| {
                    moq_multicam_core::CameraConfig {
                        name: name.clone(),
                        priority: i as u8,
                    }
                }).collect();
                let source_kind = parse_source(&source)?;
                publish_fmp4::run_multicam(relay, &vehicle, &cameras, source_kind, tls_disable_verify).await
            } else {
                anyhow::bail!("specify --broadcast for stdin pipe or --camera for built-in source")
            }
        }
        Command::Subscribe {
            relay, vehicle, cameras, tls_disable_verify,
        } => {
            let cameras = parse_cameras(&cameras);
            subscribe::run(relay, &vehicle, &cameras, tls_disable_verify).await
        }
    }
}

fn parse_cameras(s: &str) -> Vec<moq_multicam_core::CameraConfig> {
    s.split(',')
        .enumerate()
        .map(|(i, name)| moq_multicam_core::CameraConfig {
            name: name.trim().to_string(),
            priority: i as u8,
        })
        .collect()
}

fn parse_source(s: &str) -> Result<publish_fmp4::SourceKind> {
    match s {
        "ffmpeg" => Ok(publish_fmp4::SourceKind::Ffmpeg),
        #[cfg(feature = "gstreamer")]
        "gstreamer" | "gst" => Ok(publish_fmp4::SourceKind::Gstreamer),
        #[cfg(not(feature = "gstreamer"))]
        "gstreamer" | "gst" => anyhow::bail!("gstreamer support not compiled in (enable 'gstreamer' feature)"),
        #[cfg(feature = "openh264")]
        "openh264" => Ok(publish_fmp4::SourceKind::OpenH264),
        #[cfg(not(feature = "openh264"))]
        "openh264" => anyhow::bail!("openh264 support not compiled in (enable 'openh264' feature)"),
        #[cfg(feature = "v4l")]
        "v4l" | "v4l2" => Ok(publish_fmp4::SourceKind::V4l),
        #[cfg(not(feature = "v4l"))]
        "v4l" | "v4l2" => anyhow::bail!("v4l support not compiled in (enable 'v4l' feature, Linux only)"),
        other => anyhow::bail!("unknown source: {other} (expected: ffmpeg, gstreamer, openh264, v4l)"),
    }
}
