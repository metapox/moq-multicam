//! Publish fMP4 from stdin to a relay.
//!
//! Usage:
//!   ffmpeg -f lavfi -i testsrc=size=1280x720:rate=30 -c:v libx264 \
//!     -f mp4 -movflags cmaf+separate_moof+delay_moov+skip_trailer+frag_every_frame - \
//!     | moq-multicam publish-fmp4 --broadcast vehicle/truck-01/camera/front --tls-disable-verify

use anyhow::Result;
use url::Url;

pub async fn run(relay: Url, broadcast_path: &str, tls_disable_verify: bool) -> Result<()> {
    let mut broadcast = moq_lite::Broadcast::produce();
    let catalog = moq_mux::CatalogProducer::new(&mut broadcast)?;

    let fmp4 = moq_mux::import::Fmp4::new(
        broadcast.clone(),
        catalog,
        moq_mux::import::Fmp4Config { passthrough: false },
    );

    // Publish broadcast to origin
    let origin = moq_lite::Origin::produce();
    origin.publish_broadcast(broadcast_path, broadcast.consume());
    tracing::info!(broadcast = broadcast_path, "publishing fMP4 broadcast");

    // Connect to relay
    let mut config = moq_native::ClientConfig::default();
    if tls_disable_verify {
        config.tls.disable_verify = Some(true);
    }
    let client = config.init()?;

    tracing::info!(%relay, "connecting to relay...");
    let session = client.with_publish(origin.consume()).connect(relay).await?;
    tracing::info!("connected, reading fMP4 from stdin...");

    // Read stdin and decode fMP4
    let stdin_handle = tokio::spawn(read_stdin(fmp4));

    tokio::select! {
        res = session.closed() => res?,
        res = stdin_handle => res??,
    }

    Ok(())
}

async fn read_stdin(mut fmp4: moq_mux::import::Fmp4) -> Result<()> {
    let mut stdin = tokio::io::stdin();
    let mut buffer = bytes::BytesMut::new();

    loop {
        let n = tokio::io::AsyncReadExt::read_buf(&mut stdin, &mut buffer).await?;
        if n == 0 {
            tracing::info!("stdin closed");
            return Ok(());
        }
        fmp4.decode(&mut buffer)?;
    }
}
