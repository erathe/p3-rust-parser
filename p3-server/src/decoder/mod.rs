pub mod stream;

use p3_parser::Message;
use stream::MessageFramer;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Manages a TCP connection to a MyLaps P3 decoder (or test server).
/// Reads raw bytes, frames them into complete messages, and sends
/// parsed messages on a channel.
pub struct DecoderConnection {
    host: String,
    port: u16,
}

impl DecoderConnection {
    pub fn new(host: String, port: u16) -> Self {
        Self { host, port }
    }

    /// Connect to the decoder and start reading messages.
    /// Parsed messages are sent on `tx`. Reconnects on disconnect.
    pub async fn run(self, tx: mpsc::Sender<Message>) {
        loop {
            info!(host = %self.host, port = %self.port, "Connecting to decoder...");

            match TcpStream::connect((self.host.as_str(), self.port)).await {
                Ok(stream) => {
                    info!("Connected to decoder");
                    if let Err(e) = self.read_loop(stream, &tx).await {
                        warn!(error = %e, "Decoder connection lost");
                    }
                }
                Err(e) => {
                    error!(error = %e, "Failed to connect to decoder");
                }
            }

            info!("Reconnecting in 3 seconds...");
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        }
    }

    async fn read_loop(
        &self,
        mut stream: TcpStream,
        tx: &mpsc::Sender<Message>,
    ) -> anyhow::Result<()> {
        let mut framer = MessageFramer::new();
        let mut chunk = [0u8; 4096];

        loop {
            let n = stream.read(&mut chunk).await?;

            if n == 0 {
                return Err(anyhow::anyhow!("Connection closed by decoder"));
            }

            let results = framer.feed(&chunk[..n]);

            for result in results {
                match result {
                    Ok(message) => {
                        if tx.send(message).await.is_err() {
                            return Err(anyhow::anyhow!("Message channel closed"));
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to parse message, skipping");
                    }
                }
            }
        }
    }
}
