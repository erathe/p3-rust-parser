use bytes::Bytes;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

pub struct Connection {
    stream: TcpStream,
    rx: mpsc::Receiver<Bytes>,
    peer_addr: std::net::SocketAddr,
}

impl Connection {
    pub fn new(
        stream: TcpStream,
        rx: mpsc::Receiver<Bytes>,
        peer_addr: std::net::SocketAddr,
    ) -> Self {
        Self {
            stream,
            rx,
            peer_addr,
        }
    }

    /// Run the connection handler loop
    ///
    /// Receives messages from channel and writes them to the TCP stream.
    /// Monitors the TCP connection for disconnects.
    /// Supports chunked sending for fragmentation testing.
    pub async fn run(mut self, chunk_size: Option<usize>) -> Result<(), std::io::Error> {
        info!("Client connected: {}", self.peer_addr);

        let mut buf = [0u8; 1];

        loop {
            tokio::select! {
                read_result = self.stream.read(&mut buf) => {
                    match read_result {
                        Ok(0) => {
                            // Connection closed cleanly
                            info!("Client disconnected: {}", self.peer_addr);
                            break;
                        }
                        Ok(n) => {
                            // Unexpected data received (P3 clients shouldn't send data except RESEND)
                            debug!("Received {} unexpected bytes from {}", n, self.peer_addr);
                            // Continue running - this might be a RESEND request in the future
                        }
                        Err(e) => {
                            error!("Read error from {}: {}", self.peer_addr, e);
                            break;
                        }
                    }
                }

                // Receive messages to send to client
                message = self.rx.recv() => {
                    match message {
                        Some(msg) => {
                            if let Err(e) = self.send_message(&msg, chunk_size).await {
                                error!("Failed to send to {}: {}", self.peer_addr, e);
                                break;
                            }
                        }
                        None => {
                            // Channel closed, server shutting down
                            info!("Channel closed for {}", self.peer_addr);
                            break;
                        }
                    }
                }
            }
        }

        info!("Connection handler exiting for {}", self.peer_addr);
        Ok(())
    }

    async fn send_message(
        &mut self,
        message: &[u8],
        chunk_size: Option<usize>,
    ) -> Result<(), std::io::Error> {
        match chunk_size {
            Some(size) if size > 0 => {
                for chunk in message.chunks(size) {
                    self.stream.write_all(chunk).await?;
                    debug!("Sent {} byte chunk to {}", chunk.len(), self.peer_addr);
                    // Small delay between chunks to simulate real network conditions
                    tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
                }
            }
            _ => {
                // Send complete message
                self.stream.write_all(message).await?;
                debug!("Sent {} byte message to {}", message.len(), self.peer_addr);
            }
        }

        self.stream.flush().await?;
        Ok(())
    }
}
