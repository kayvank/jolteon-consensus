use crate::error::NetworkError;
use bytes::Bytes;
use futures::sink::SinkExt as _;
use futures::stream::StreamExt as _;
use log::{info, warn};
use rand::SeedableRng as _;
use rand::prelude::SliceRandom as _;
use rand::rngs::SmallRng;
use std::cmp::min;
use std::collections::{HashMap, VecDeque};
use std::fmt::Debug;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio::sync::mpsc::{Receiver, Sender, channel};
use tokio::sync::oneshot;
use tokio::time::{Duration, sleep};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

/// Convenient alias for cancel handlers returned to the caller task.
pub type CancelHandler = oneshot::Receiver<Bytes>;

/// We keep alive one TCP connection per peer, each connection is handled by a separate task (called `Connection`).
/// We communicate with our 'connections' through a dedicated channel kept by the HashMap called `connections`.
/// This sender is 'reliable' in the sense that it keeps trying to re-transmit messages for which it didn't
/// receive an ACK back (until they succeed or are canceled).
pub struct ReliableSender {
    /// A map holding the channels to our connections.
    connections: HashMap<SocketAddr, Sender<InnerMessage>>,
    /// Small RNG just used to shuffle nodes and randomize connections (not crypto related).
    rng: SmallRng,
}

impl std::default::Default for ReliableSender {
    fn default() -> Self {
        Self::new()
    }
}

impl ReliableSender {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
            rng: SmallRng::from_os_rng(),
        }
    }

    fn spawn_connection(address: SocketAddr) -> Sender<InnerMessage> {
        let (tx, rx) = channel::<InnerMessage>(1_000);
        Connection::spawn(address, rx);
        tx
    }

    pub async fn send(&mut self, address: SocketAddr, data: Bytes) -> CancelHandler {
        let (sender, receiver) = oneshot::channel::<Bytes>();
        self.connections
            .entry(address)
            .or_insert_with(|| Self::spawn_connection(address))
            .send(InnerMessage {
                data,
                cancel_handler: sender,
            })
            .await
            .expect("Failed to send internal message");

        receiver
    }

    pub async fn broadcast(
        &mut self,
        addresses: Vec<SocketAddr>,
        data: Bytes,
    ) -> Vec<CancelHandler> {
        let mut handlers = Vec::new();
        for address in addresses {
            let handler = self.send(address, data.clone()).await;
            handlers.push(handler);
        }
        handlers
    }

    pub async fn lucky_broadcast(
        &mut self,
        mut addresses: Vec<SocketAddr>,
        data: Bytes,
        nodes: usize,
    ) -> Vec<CancelHandler> {
        addresses.shuffle(&mut self.rng);
        addresses.truncate(nodes);
        self.broadcast(addresses, data).await
    }
}

/// Simple message used by `ReliableSender` to communicate with its connections.
#[derive(Debug)]
struct InnerMessage {
    /// The data to transmit.
    data: Bytes,
    /// The cancel handler allowing the caller task to cancel the transmission of this message
    /// and to be notified of its successfully transmission.
    cancel_handler: oneshot::Sender<Bytes>,
}

/// A connection is responsible to reliably establish (and keep alive) a connection with a single peer.
struct Connection {
    /// The destination address.
    address: SocketAddr,
    /// Channel from which the connection receives its commands.
    receiver: Receiver<InnerMessage>,
    /// The initial delay to wait before re-attempting a connection (in ms).
    retry_delay: u64,
    /// Buffer keeping all messages that need to be re-transmitted.
    buffer: VecDeque<(Bytes, oneshot::Sender<Bytes>)>,
}
impl Connection {
    fn spawn(address: SocketAddr, receiver: Receiver<InnerMessage>) {
        tokio::spawn(async move {
            Self {
                address,
                receiver,
                retry_delay: 200,
                buffer: VecDeque::new(),
            }
            .run()
            .await;
        });
    }

    async fn run(&mut self) {
        let mut delay = self.retry_delay;
        let mut retry = 0;
        loop {
            match TcpStream::connect(self.address).await {
                Ok(stream) => {
                    info!("Outgoing connection established with {}", self.address);
                    delay = self.retry_delay;
                    retry = 0;

                    let error = self.keep_alive(stream).await;
                    warn!("{}", error);
                }
                Err(e) => {
                    warn!("{}", NetworkError::FailedToConnect(self.address, retry, e));
                    let timer = sleep(Duration::from_millis(delay));
                    tokio::pin!(timer);

                    'waiter: loop {
                        tokio::select! {
                            () = &mut timer => {
                                delay = min(2*delay, 60_000);
                                retry += 1;
                                break 'waiter;
                            },
                            Some(InnerMessage{data, cancel_handler}) = self.receiver.recv() =>  {
                                self.buffer.push_back((data,cancel_handler));
                                self.buffer.retain(|(_, handler)| !handler.is_closed());
                            },
                        }
                    }
                }
            }
        }
    }
    async fn keep_alive(&mut self, stream: TcpStream) -> NetworkError {
        // This buffer keeps all messages and handlers that we have successfully transmitted but for
        // which we are still waiting to receive an ACK.
        let mut pending_replies = VecDeque::new();
        let (mut writer, mut reader) = Framed::new(stream, LengthDelimitedCodec::new()).split();
        let error = 'connection: loop {
            while let Some((data, handler)) = self.buffer.pop_front() {
                if handler.is_closed() {
                    continue;
                }
                match writer.send(data.clone()).await {
                    Ok(()) =>
                    // message was sent, add to `pending-replies` to wait for ACK
                    {
                        pending_replies.push_back((data, handler))
                    }
                    Err(e) => {
                        // put it back in buffer to retry
                        self.buffer.push_front((data, handler));
                        break 'connection NetworkError::FailedToSendMessage(self.address, e);
                    }
                }
            }
            tokio::select! {
                Some(InnerMessage {data, cancel_handler}) = self.receiver.recv() => {
                   // add the message to buffer of messages to send
                   self.buffer.push_back((data, cancel_handler));
                },
                response = reader.next() => {
                    let (data, handler) =
                        match pending_replies.pop_front(){
                            Some(message) => message,
                            None => break 'connection NetworkError::UnexpectedAck(self.address),
                        };
                    match response {
                        Some(Ok(bytes)) => {
                            // notify handler message successfully has been received
                            let _ = handler.send(bytes.freeze());
                        },
                        _ => {
                            // Something has gone wrong (either the channel dropped or we failed to read from it).
                            // Put the message back in the buffer, we will try to send it again.
                            pending_replies.push_front((data, handler));
                            break 'connection NetworkError::FailedToReceiveAck(self.address);
                        }
                    }
                },
            }
        };
        // If we reach this code, it means something went wrong. Put the messages for which we didn't receive an ACK
        // back into the sending buffer, we will try to send them again once we manage to establish a new connection.
        while let Some(message) = pending_replies.pop_back() {
            self.buffer.push_front(message);
        }
        error
    }
}
