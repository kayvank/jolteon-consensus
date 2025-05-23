use super::*;
use futures::sink::SinkExt as _;
use tokio::sync::mpsc::Sender;
use tokio::sync::mpsc::channel;
use tokio::time::{Duration, sleep};

#[derive(Clone)]
struct TestHandler {
    deliver: Sender<String>,
}

#[async_trait]
impl MessageHandler for TestHandler {
    async fn dispatch(&self, writer: &mut Writer, message: Bytes) -> Result<(), Box<dyn Error>> {
        // Reply with an ACK.
        let _ = writer.send(Bytes::from("Ack")).await;

        // Deserialize the message.
        let (message, _) =
            bincode::serde::decode_from_slice(&message, bincode::config::standard()).unwrap();

        // Deliver the message to the application.
        self.deliver.send(message).await.unwrap();
        Ok(())
    }
}

#[tokio::test]
async fn receive() {
    // Make the network receiver.
    let address = "127.0.0.1:4000".parse::<SocketAddr>().unwrap();
    let (tx, mut rx) = channel(1);
    Receiver::spawn(address, TestHandler { deliver: tx });
    sleep(Duration::from_millis(50)).await;

    // Send a message.
    let sent: &str = "Hello, world!";
    let bytes =
        Bytes::from(bincode::serde::encode_to_vec(sent, bincode::config::standard()).unwrap());
    let stream = TcpStream::connect(address).await.unwrap();
    let mut transport = Framed::new(stream, LengthDelimitedCodec::new());
    transport.send(bytes.clone()).await.unwrap();

    // Ensure the message gets passed to the channel.
    let message = rx.recv().await;
    assert!(message.is_some());
    let received: String = message.unwrap();
    assert_eq!(received.to_owned(), sent);
}
