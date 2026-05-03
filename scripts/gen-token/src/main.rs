use clap::{Parser, Subcommand};
use moq_token::{Algorithm, Claims, Key};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[derive(Parser)]
#[command(about = "Generate auth keys and JWT tokens for moq-relay")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Generate an ES256 key pair (server.jwk + server.pub.jwk)
    Keygen {
        #[arg(short, long, default_value = "auth")]
        output_dir: PathBuf,
    },
    /// Generate a JWT token for publishing
    Token {
        #[arg(short, long, default_value = "auth/server.jwk")]
        key: PathBuf,
        /// Publish path prefixes (e.g. "vehicle/")
        #[arg(short, long, default_value = "")]
        publish: Vec<String>,
        /// Token lifetime in hours
        #[arg(long, default_value = "8760")]
        hours: u64,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Keygen { output_dir } => {
            std::fs::create_dir_all(&output_dir)?;

            let key = Key::generate(Algorithm::ES256, None)?;
            let priv_path = output_dir.join("server.jwk");
            key.to_file(&priv_path)?;
            #[cfg(unix)]
            std::fs::set_permissions(&priv_path, std::fs::Permissions::from_mode(0o600))?;
            eprintln!("wrote private key: {}", priv_path.display());

            let pub_key = key.to_public()?;
            let pub_path = output_dir.join("server.pub.jwk");
            pub_key.to_file(&pub_path)?;
            eprintln!("wrote public key:  {}", pub_path.display());
        }
        Cmd::Token {
            key,
            publish,
            hours,
        } => {
            let key = Key::from_file(&key)?;
            let claims = Claims {
                publish,
                subscribe: vec!["".to_string()],
                expires: Some(SystemTime::now() + Duration::from_secs(hours * 3600)),
                issued: Some(SystemTime::now()),
                ..Default::default()
            };
            let token = key.encode(&claims)?;
            println!("{token}");
        }
    }
    Ok(())
}
