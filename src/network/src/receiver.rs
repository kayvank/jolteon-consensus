use crate::error::NetworkError;
use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::SplitSink;
use futures::stream::StreamExt as _;
use log::{debug, info, warn};
use std::error::Error;
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

/// Convenient alias for the writer end of the TCP channel.
pub type Writer = SplitSink<Framed<TcpStream, LengthDelimitedCodec>, Bytes>;

#[async_trait]
pub trait MessageHandler: Clone + Send + Sync + 'static {
    /// Defines how to handle an incoming message.
    async fn dispatch(&self, writer: &mut Writer, messge: Bytes) -> Result<(), Box<dyn Error>>;
}
/// For each incoming request, we spawn a new runner responsible to receive messages and forward them
/// through the provided deliver channel.
pub struct Receiver<Handler: MessageHandler> {
    /// Address to listen to.
    address: SocketAddr,
    /// Struct responsible to define how to handle received messages.
    handler: Handler,
}

impl<Handler: MessageHandler> Receiver<Handler> {
    /// Spawn a new network receiver handling connections from any incoming peer.
    pub fn spawn(address: SocketAddr, handler: Handler) {
        tokio::spawn(async move { Self { address, handler }.run().await });
    }
    /// Main loop responsible to accept incoming connections and spawn a new runner to handle it.
    async fn run(&self) {
        let listener = TcpListener::bind(&self.address)
            .await
            .expect("Failed to bind TCP port");
        debug!("Listening on {}", self.address);
        loop {
            let (socket, peer) = match listener.accept().await {
                Ok(v) => v,
                Err(e) => {
                    warn!("{}", NetworkError::FailedToListen(e));
                    continue;
                }
            };
            info!("Incomming connection establised with {}", peer);
            Self::spawn_runer(socket, peer, self.handler.clone()).await
        }
    }

    /// Spawn a new runner to handle a specific TCP connection, receive and process messages
    async fn spawn_runer(socket: TcpStream, peer: SocketAddr, handler: Handler) {
        tokio::spawn(async move {
            let transport = Framed::new(socket, LengthDelimitedCodec::new());
            let (mut writer, mut reader) = transport.split();
            while let Some(frame) = reader.next().await {
                match frame.map_err(|e| NetworkError::FailedToReceiveMessage(peer, e)) {
                    Ok(message) => {
                        if let Err(e) = handler.dispatch(&mut writer, message.freeze()).await {
                            warn!("{}", e);
                            return;
                        }
                    }
                    Err(e) => {
                        warn!("{}", e);
                        return;
                    }
                }
            }
            warn!("Connection closed by peer {}", peer);
        });
    }
}
#[cfg(test)]
#[path = "tests/receiver_tests.rs"]
pub mod receiver_tests;
