mod connection;

use bytes::Bytes;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, mpsc};
use tracing::{debug, error, info, warn};

use connection::Connection;

type ClientId = usize;

#[derive(Clone)]
pub struct TransportHandle {
    tx: mpsc::Sender<BroadcastMessage>,
}

impl TransportHandle {
    /// Send a message to all connected clients
    pub async fn send(&self, message: impl Into<Bytes>) -> Result<(), SendError> {
        self.tx
            .send(BroadcastMessage::Data(message.into()))
            .await
            .map_err(|_| SendError::Disconnected)
    }
}

/// Internal message types for the broadcast channel
enum BroadcastMessage {
    Data(Bytes),
    RegisterClient(ClientId, mpsc::Sender<Bytes>),
    UnregisterClient(ClientId),
}

pub struct TcpTransport {
    listener: TcpListener,
    broadcast_tx: mpsc::Sender<BroadcastMessage>,
    broadcast_rx: mpsc::Receiver<BroadcastMessage>,
    max_clients: usize,
    chunk_size: Option<usize>,
    next_client_id: ClientId,
    clients: HashMap<ClientId, mpsc::Sender<Bytes>>,
}

impl TcpTransport {
    /// Create a new TCP transport server
    ///
    /// # Arguments
    /// * `port` - Port to listen on (typically 5403 for P3 protocol)
    /// * `max_clients` - Maximum number of simultaneous client connections
    /// * `chunk_size` - Optional chunk size for fragmentation testing (None = send complete messages)
    ///
    /// # Returns
    /// A tuple of (TcpTransport, TransportHandle) where the handle can be used to send messages
    pub async fn new(
        port: u16,
        max_clients: usize,
        chunk_size: Option<usize>,
    ) -> Result<(Self, TransportHandle), std::io::Error> {
        let listener = TcpListener::bind(("0.0.0.0", port)).await?;
        let addr = listener.local_addr()?;

        info!("TCP server listening on {}", addr);
        if let Some(size) = chunk_size {
            info!("Chunked sending enabled: {} bytes per chunk", size);
        }

        // Channel for broadcasting messages to all clients
        // Buffer size of 32 allows simulator to queue messages without blocking
        let (broadcast_tx, broadcast_rx) = mpsc::channel(32);

        let transport = Self {
            listener,
            broadcast_tx: broadcast_tx.clone(),
            broadcast_rx,
            max_clients,
            chunk_size,
            next_client_id: 0,
            clients: HashMap::new(),
        };

        let handle = TransportHandle { tx: broadcast_tx };

        Ok((transport, handle))
    }

    pub async fn run(mut self) -> Result<(), std::io::Error> {
        // Semaphore to limit concurrent connections
        let connection_semaphore = Arc::new(Semaphore::new(self.max_clients));

        info!("Server ready, accepting up to {} clients", self.max_clients);

        loop {
            tokio::select! {
                // Accept new client connections
                accept_result = self.listener.accept() => {
                    match accept_result {
                        Ok((stream, addr)) => {
                            let permit = match connection_semaphore.clone().try_acquire_owned() {
                                Ok(permit) => permit,
                                Err(_) => {
                                    warn!("Connection limit reached, rejecting client: {}", addr);
                                    continue;
                                }
                            };

                            debug!("Accepted connection from {}", addr);

                            // Assign client ID
                            let client_id = self.next_client_id;
                            self.next_client_id += 1;

                            // Create a channel for this client
                            let (client_tx, client_rx) = mpsc::channel(32);

                            // Register the client
                            self.clients.insert(client_id, client_tx);
                            info!("Client {} registered ({}), total clients: {}", client_id, addr, self.clients.len());

                            // Spawn connection handler
                            let chunk_size = self.chunk_size;
                            let broadcast_tx = self.broadcast_tx.clone();

                            tokio::spawn(async move {
                                let connection = Connection::new(stream, client_rx, addr);
                                if let Err(e) = connection.run(chunk_size).await {
                                    error!("Connection error for {}: {}", addr, e);
                                }

                                // Unregister client when connection closes
                                let _ = broadcast_tx.send(BroadcastMessage::UnregisterClient(client_id)).await;
                                drop(permit); // Release connection slot
                            });
                        }
                        Err(e) => {
                            error!("Failed to accept connection: {}", e);
                        }
                    }
                }

                // Handle broadcast messages
                Some(msg) = self.broadcast_rx.recv() => {
                    match msg {
                        BroadcastMessage::Data(message) => {
                            // Broadcast to all connected clients
                            let mut failed_clients = Vec::new();

                            for (client_id, client_tx) in &self.clients {
                                if client_tx.send(message.clone()).await.is_err() {
                                    failed_clients.push(*client_id);
                                }
                            }

                            // Remove failed clients
                            for client_id in failed_clients {
                                self.clients.remove(&client_id);
                                warn!("Removed disconnected client {}", client_id);
                            }

                            debug!("Broadcasted {} bytes to {} clients", message.len(), self.clients.len());
                        }
                        BroadcastMessage::RegisterClient(client_id, client_tx) => {
                            self.clients.insert(client_id, client_tx);
                            info!("Client {} registered, total clients: {}", client_id, self.clients.len());
                        }
                        BroadcastMessage::UnregisterClient(client_id) => {
                            self.clients.remove(&client_id);
                            info!("Client {} unregistered, total clients: {}", client_id, self.clients.len());
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("Transport disconnected")]
    Disconnected,
}
