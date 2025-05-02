type Store = super::Store<Vec<u8>, Vec<u8>>;

#[tokio::test]
async fn create_store() {
    // Create new store.
    let path = tempfile::NamedTempFile::new().unwrap().into_temp_path();

    let mut store = match Store::new(&path) {
        Ok(store) => {
            assert!(true, "failed to create database");
            store
        }
        Err(e) => {
            assert!(false);
            panic!("failed to create test-db with error: {}", e);
        }
    };

    // Write value to the store.
    let key = vec![0u8, 1u8, 3u8, 3u8];
    let value = vec![4u8, 5u8, 6u8, 7u8];
    store.write(key.clone(), value.clone()).await;

    // Read value.
    let result = store.read(key).await;
    assert!(result.is_ok());
    let read_value = result.unwrap();
    assert!(read_value.is_some());
    assert_eq!(read_value.unwrap(), value);
    // Create new store.

    // Try to read unknown key.
    let key = vec![0u8, 1u8, 2u8, 3u8];
    let result = store.read(key).await;
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());

    // Try to read a kew that does not yet exist. Then write a value
    // for that key and check that notify read returns the result.
    let key = vec![0u8, 1u8, 2u8, 3u8];
    let value = vec![4u8, 5u8, 6u8, 7u8];

    // Try to read a missing value.
    let mut store_copy = store.clone();
    let key_copy = key.clone();
    let value_copy = value.clone();
    let handle = tokio::spawn(async move {
        match store_copy.notify_read(key_copy).await {
            Ok(v) => assert_eq!(v, value_copy),
            _ => assert!(false),
        }
    });

    // Write the missing value and ensure the handle terminates correctly.
    store.write(key, value).await;
    assert!(handle.await.is_ok());
}
