mod connection;

use bytes::Bytes;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, mpsc};
use tracing::{debug, error, info, warn};

use connection::Connection;

type ClientId = usize;

/// Per-client connection statistics, readable while the server runs
#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub id: usize,
    pub addr: SocketAddr,
    pub bytes_sent: u64,
    pub messages_sent: u64,
}

/// Shared registry of connected clients and their send counters
#[derive(Clone, Default)]
pub struct ClientRegistry {
    inner: Arc<StdMutex<HashMap<ClientId, ClientInfo>>>,
}

impl ClientRegistry {
    fn register(&self, id: ClientId, addr: SocketAddr) {
        self.inner.lock().unwrap().insert(
            id,
            ClientInfo {
                id,
                addr,
                bytes_sent: 0,
                messages_sent: 0,
            },
        );
    }

    fn unregister(&self, id: ClientId) {
        self.inner.lock().unwrap().remove(&id);
    }

    pub(crate) fn record_send(&self, id: ClientId, bytes: usize) {
        if let Some(info) = self.inner.lock().unwrap().get_mut(&id) {
            info.bytes_sent += bytes as u64;
            info.messages_sent += 1;
        }
    }

    /// Current clients, sorted by connection order
    pub fn snapshot(&self) -> Vec<ClientInfo> {
        let mut clients: Vec<_> = self.inner.lock().unwrap().values().cloned().collect();
        clients.sort_by_key(|c| c.id);
        clients
    }
}

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
    UnregisterClient(ClientId),
}

pub struct TcpTransport {
    listener: TcpListener,
    broadcast_tx: mpsc::Sender<BroadcastMessage>,
    broadcast_rx: mpsc::Receiver<BroadcastMessage>,
    max_clients: usize,
    chunk_size: Option<usize>,
    /// Message sent to each client immediately on connect (real decoders
    /// send a VERSION message here)
    greeting: Option<Bytes>,
    registry: ClientRegistry,
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
    /// * `greeting` - Optional message sent to each client on connect (VERSION)
    ///
    /// # Returns
    /// (TcpTransport, TransportHandle, ClientRegistry): the handle sends
    /// messages, the registry exposes connected-client stats
    pub async fn new(
        port: u16,
        max_clients: usize,
        chunk_size: Option<usize>,
        greeting: Option<Bytes>,
    ) -> Result<(Self, TransportHandle, ClientRegistry), std::io::Error> {
        let listener = TcpListener::bind(("0.0.0.0", port)).await?;
        let addr = listener.local_addr()?;

        info!("TCP server listening on {}", addr);
        if let Some(size) = chunk_size {
            info!("Chunked sending enabled: {} bytes per chunk", size);
        }

        // Channel for broadcasting messages to all clients
        // Buffer size of 32 allows simulator to queue messages without blocking
        let (broadcast_tx, broadcast_rx) = mpsc::channel(32);
        let registry = ClientRegistry::default();

        let transport = Self {
            listener,
            broadcast_tx: broadcast_tx.clone(),
            broadcast_rx,
            max_clients,
            chunk_size,
            greeting,
            registry: registry.clone(),
            next_client_id: 0,
            clients: HashMap::new(),
        };

        let handle = TransportHandle { tx: broadcast_tx };

        Ok((transport, handle, registry))
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

                            // Greet the new client (fresh channel, cannot be full)
                            if let Some(greeting) = &self.greeting {
                                let _ = client_tx.try_send(greeting.clone());
                            }

                            // Register the client
                            self.clients.insert(client_id, client_tx);
                            self.registry.register(client_id, addr);
                            info!("Client {} registered ({}), total clients: {}", client_id, addr, self.clients.len());

                            // Spawn connection handler
                            let chunk_size = self.chunk_size;
                            let broadcast_tx = self.broadcast_tx.clone();
                            let registry = self.registry.clone();

                            tokio::spawn(async move {
                                let connection = Connection::new(stream, client_rx, addr, client_id, registry.clone());
                                if let Err(e) = connection.run(chunk_size).await {
                                    error!("Connection error for {}: {}", addr, e);
                                }

                                // Unregister client when connection closes
                                registry.unregister(client_id);
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
                                self.registry.unregister(client_id);
                                warn!("Removed disconnected client {}", client_id);
                            }

                            debug!("Broadcasted {} bytes to {} clients", message.len(), self.clients.len());
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
