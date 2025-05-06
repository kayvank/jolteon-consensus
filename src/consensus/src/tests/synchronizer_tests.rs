use super::*;
use crate::common::{block, chain, committee, committee_with_base_port, keys, listener};
use std::fs;

#[tokio::test]
async fn get_existing_parent_block() {
    let mut chain = chain(keys());
    let block = chain.pop().unwrap();
    let b2 = chain.pop().unwrap();

    // Add the block b2 to the store.
    let path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
    let mut store = Store::new(path).unwrap();
    let key: Vec<u8> = b2.digest().to_vec();
    let value: Vec<u8> = bincode::serde::encode_to_vec(&b2, bincode::config::standard()).unwrap();
    let _ = store.write(key, value).await;

    // Make a new synchronizer.
    let (name, _) = keys().pop().unwrap();
    let (tx_loopback, _) = channel(10);
    let mut synchronizer = Synchronizer::new(
        name,
        committee(),
        store,
        tx_loopback,
        /* sync_retry_delay */ 10_000,
    );

    // Ask the predecessor of 'block' to the synchronizer.
    match synchronizer.get_parent_block(&block).await {
        Ok(Some(b)) => assert_eq!(b, b2),
        Ok(_) => assert!(false, "returned some invalid payload"),
        Err(e) => assert!(false, "returned error e: {}", e),
    }
}
