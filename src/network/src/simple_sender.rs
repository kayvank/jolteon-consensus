use crate::error::NetworkError;
use bytes::Bytes;
use futures::sink::SinkExt as _;
use futures::stream::StreamExt as _;
use log::{info, warn};
use rand::SeedableRng as _;
use rand::prelude::SliceRandom as _;
use rand::rngs::SmallRng;
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{Receiver, Sender, channel};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

/// We keep alive one TCP connection per peer, each connection is handled by a separate task (called `Connection`).
/// We communicate with our 'connections' through a dedicated channel kept by the HashMap called `connections`.
pub struct SimpleSender {
    /// channels to our connections
    connections: HashMap<SocketAddr, Sender<Bytes>>,
    /// not crypto related, used to shuffle connections
    rng: SmallRng,
}
impl std::default::Default for SimpleSender {
    fn default() -> Self {
        Self::new()
    }
}

impl SimpleSender {
    pub fn new() -> Self {
        SimpleSender {
            connections: HashMap::new(),
            rng: SmallRng::from_os_rng(),
        }
    }

    /// Try (best-effort) to send a message to a specific address.
    /// This is useful to answer sync requests.
    pub async fn send(&mut self, address: SocketAddr, data: Bytes) {
        // re-use exsiting connection if there is one
        if let Some(tx) = self.connections.get(&address) {
            if tx.send(data.clone()).await.is_ok() {
                return;
            }
        }
        // otherwise make a new connection
        let tx = Self::spawn_connection(address);
        if tx.send(data.clone()).await.is_ok() {
            self.connections.insert(address, tx);
        }
    }

    /// Try (best-effort) to broadcast the messages to all specified addresses
    pub async fn broadcast(&mut self, addresses: Vec<SocketAddr>, data: Bytes) {
        for address in addresses {
            self.send(address, data.clone()).await
        }
    }

    /// Pick a few addresses at random (specified by `nodes`) and try (best-effort) to send the
    /// message only to them. This is useful to pick nodes with whom to sync.
    pub async fn lucky_broadcast(
        &mut self,
        mut addresses: Vec<SocketAddr>,
        data: Bytes,
        nodes: usize,
    ) {
        addresses.shuffle(&mut self.rng);
        addresses.truncate(nodes);
        self.broadcast(addresses, data).await
    }

    fn spawn_connection(address: SocketAddr) -> Sender<Bytes> {
        let (tx, rx) = channel(1_000);
        Connection::spawn(address, rx);
        tx
    }
}

struct Connection {
    address: SocketAddr,
    receiver: Receiver<Bytes>,
}

impl Connection {
    fn spawn(address: SocketAddr, receiver: Receiver<Bytes>) {
        tokio::spawn(async move {
            Self { address, receiver }.run().await;
        });
    }

    /// main loop trying to connect to peer and transmit messages
    async fn run(&mut self) {
        // Try to connect to the peer.
        let (mut writer, mut reader) = match TcpStream::connect(self.address).await {
            Ok(stream) => Framed::new(stream, LengthDelimitedCodec::new()).split(),
            Err(e) => {
                warn!(
                    "{}",
                    NetworkError::FailedToConnect(self.address, /* retry */ 0, e)
                );
                return;
            }
        };
        info!("Outgoing connection established with {}", self.address);

        // Transmit messages once we have established a connection.
        loop {
            // Check if there are any new messages to send or if we get an ACK for messages we already sent.
            tokio::select! {
                Some(data) = self.receiver.recv() => {
                    if let Err(e) = writer.send(data).await {
                        warn!("{}", NetworkError::FailedToSendMessage(self.address, e));
                        return;
                    }
                },
                response = reader.next() => {
                    match response {
                        Some(Ok(_)) => {
                            info!("Response received");
                            // Sink the reply.
                        },
                        _ => {
                            // Something has gone wrong (either the channel dropped or we failed to read from it).
                            warn!("{}", NetworkError::FailedToReceiveAck(self.address));
                            return;
                        }
                    }
                },
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/simple_sender_tests.rs"]
pub mod simple_sender_tests;
