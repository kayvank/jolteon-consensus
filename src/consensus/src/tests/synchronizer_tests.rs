use super::*;
use crate::common::{block, chain, committee, committee_with_base_port, keys, listener};

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
    let (_b2, _): (Block, usize) =
        bincode::serde::decode_from_slice(&value, bincode::config::standard()).unwrap();
    assert_eq!(_b2, b2.clone(), "encode decode of value failed");

    let _ = store.write(key.clone(), value.clone()).await;
    let _v = store.read(key.clone()).await;
    let (v, _): (Block, usize) =
        bincode::serde::decode_from_slice(&_v.unwrap().unwrap(), bincode::config::standard())
            .unwrap();
    assert_eq!(v, b2, "decoding of v and value has failed");

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

#[tokio::test]
async fn get_genesis_parent_block() {
    // Make a new synchronizer.
    let path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
    let store = Store::new(path).unwrap();
    let (name, _) = keys().pop().unwrap();
    let (tx_loopback, _) = channel(1);
    let mut synchronizer = Synchronizer::new(
        name,
        committee(),
        store,
        tx_loopback,
        /* sync_retry_delay */ 10_000,
    );

    // Ask the predecessor of 'block' to the synchronizer.
    match synchronizer.get_parent_block(&block()).await {
        Ok(Some(b)) => assert_eq!(b, Block::genesis()),
        _ => assert!(false),
    }
}

#[tokio::test]
async fn get_missing_parent_block() {
    let _ = env_logger::builder()
        .target(env_logger::Target::Stdout)
        .filter_level(log::LevelFilter::Trace)
        .is_test(true)
        .try_init();

    let committee = committee_with_base_port(12_030);
    let mut chain = chain(keys());
    let block = chain.pop().unwrap();
    let parent_block = chain.pop().unwrap();

    // Make a new synchronizer.
    let path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
    let mut store = Store::new(path).unwrap();
    let (name, _) = keys().pop().unwrap();
    let (tx_loopback, mut rx_loopback) = channel(1);
    let mut synchronizer = Synchronizer::new(
        name,
        committee.clone(),
        store.clone(),
        tx_loopback,
        /* sync_retry_delay */ 10_000,
    );

    // redb does creates tables `lazyly`.
    // to create the table, we insert a fake block
    let key = vec![0u8, 0u8, 0u8, 0u8];
    let value = vec![0u8, 0u8, 0u8, 0u8];
    store.write(key.clone(), value.clone()).await;

    // Spawn a listener to receive our sync request.
    let address = committee.address(&block.author).unwrap();
    let message = ConsensusMessage::SyncRequest(parent_block.digest(), name);
    let expected =
        Bytes::from(bincode::serde::encode_to_vec(&message, bincode::config::standard()).unwrap());
    let listener_handle = listener(address, Some(expected.clone()));

    // Ask for the parent of a block to the synchronizer. The store does not have the parent yet.
    let copy = block.clone();
    let handle = tokio::spawn(async move {
        let ret = synchronizer.get_parent_block(&copy).await;
        assert!(ret.is_ok());
        assert!(ret.unwrap().is_none());
    });

    // Ensure the other listeners correctly received the sync request.
    assert!(listener_handle.await.is_ok());

    // Ensure the synchronizer returns None, thus suspending the processing of the block.
    assert!(handle.await.is_ok());
    // Add the parent to the store.
    let key = parent_block.digest().to_vec();
    let value = bincode::serde::encode_to_vec(&parent_block, bincode::config::standard()).unwrap();
    let _ = store.write(key, value).await;

    /*
     * for this test to pass, network receiver must be up.
     * In other words the committee member need to be up
    // Now that we have the parent, ensure the synchronizer loops back the block to the core
    // to resume processing.
    println!("calling the delivered .... ");
    let delivered = rx_loopback.recv().await.unwrap();
    println!("delivered was delivered: {:?}", delivered);
    assert_eq!(delivered, block.clone());

     */
}
