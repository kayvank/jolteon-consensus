use super::*;
use crate::common::{batch_digest, committee_with_base_port, keys, listener};
use tokio::sync::mpsc::channel;

#[tokio::test]
async fn synchronize() {
    let (tx_message, rx_message) = channel(1);

    let mut keys = keys();
    let (name, _) = keys.pop().unwrap();
    let committee = committee_with_base_port(9_000);

    // Create a new test store.
    let path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
    let store = Store::new(&path).unwrap();

    // Spawn a `Synchronizer` instance.
    Synchronizer::spawn(
        name,
        committee.clone(),
        store.clone(),
        /* gc_depth */ 50, // Not used in this test.
        /* sync_retry_delay */ 1_000_000, // Ensure it is not triggered.
        /* sync_retry_nodes */ 3, // Not used in this test.
        rx_message,
    );

    // Spawn a listener to receive our batch requests.
    let (target, _) = keys.pop().unwrap();
    let address = committee.mempool_address(&target).unwrap();
    let missing = vec![batch_digest()];
    let message = MempoolMessage::BatchRequest(missing.clone(), name);
    let serialized = bincode::serde::encode_to_vec(&message, bincode::config::standard()).unwrap();
    let handle = listener(address, Some(Bytes::from(serialized)));

    // Send a sync request.
    let message = ConsensusMempoolMessage::Synchronize(missing, target);
    tx_message.send(message).await.unwrap();

    // Ensure the target receives the sync request.
    assert!(handle.await.is_ok());
}
