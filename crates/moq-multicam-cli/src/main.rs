use anyhow::Result;
use clap::Parser;
use url::Url;

mod publish;
mod subscribe;

#[derive(Parser)]
#[command(name = "moq-multicam", version, about = "Multi-camera streaming over MoQ")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Publish test cameras to a relay
    Publish {
        /// Relay URL
        #[arg(long, default_value = "https://localhost:4443")]
        relay: Url,

        /// Vehicle ID
        #[arg(long, default_value = "truck-01")]
        vehicle: String,

        /// Camera names (comma-separated)
        #[arg(long, default_value = "front,rear")]
        cameras: String,

        /// Disable TLS verification (for local dev)
        #[arg(long)]
        tls_disable_verify: bool,
    },
    /// Subscribe to cameras from a relay
    Subscribe {
        /// Relay URL
        #[arg(long, default_value = "https://localhost:4443")]
        relay: Url,

        /// Vehicle ID
        #[arg(long, default_value = "truck-01")]
        vehicle: String,

        /// Camera names (comma-separated)
        #[arg(long, default_value = "front,rear")]
        cameras: String,

        /// Disable TLS verification (for local dev)
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
        Command::Publish {
            relay,
            vehicle,
            cameras,
            tls_disable_verify,
        } => {
            let cameras = parse_cameras(&cameras);
            publish::run(relay, &vehicle, &cameras, tls_disable_verify).await
        }
        Command::Subscribe {
            relay,
            vehicle,
            cameras,
            tls_disable_verify,
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
