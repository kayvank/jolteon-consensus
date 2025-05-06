use bincode::{Decode, Encode, decode_from_slice, encode_to_vec};
use core::fmt::Debug;
use redb::{Database, Key, TableDefinition, TypeName, Value};
use std::any::type_name;
use std::cmp::Ordering;
use std::collections::{HashMap, VecDeque};
use tokio::sync::mpsc::{Sender, channel};
use tokio::sync::oneshot;

pub type StoreError = redb::Error;

pub type StoreResult<T> = Result<T, StoreError>;

pub enum StoreCommand<K, V>
where
    K: Encode + PartialEq + PartialOrd + Send,
    V: Encode + PartialEq,
{
    Write(K, V),
    Read(K, oneshot::Sender<Option<V>>),
    NotifyRead(K, oneshot::Sender<StoreResult<V>>),
}

pub struct DB {
    pub db: Database,
}

pub trait Persist<K, V> {
    fn put(&self, k: K, v: V) -> StoreResult<()>;
    fn get(&self, k: &K) -> StoreResult<Option<V>>;
}
impl DB {
    pub fn new(p: impl AsRef<std::path::Path>) -> StoreResult<Self> {
        Database::create(p)
            .map(|db| Self { db })
            .map_err(redb::Error::from)
    }
}

impl DB {
    pub fn get_db(&self) -> &Database {
        &self.db
    }
}

#[derive(Clone)]
pub struct Store<K, V>
where
    K: Encode + PartialEq + PartialOrd + Send,
    V: Encode + PartialEq,
{
    channel: Sender<StoreCommand<K, V>>,
}
impl<K, V> Store<K, V>
where
    K: Clone
        + Debug
        + Encode
        + PartialEq
        + PartialOrd
        + Ord
        + Encode
        + Send
        + Decode<()>
        + std::hash::Hash
        + 'static
        + std::marker::Sync,
    V: Debug + Encode + PartialEq + Send + Encode + Decode<()> + 'static,
{
    pub fn new(path: impl AsRef<std::path::Path>) -> StoreResult<Self> {
        let db = DB::new(path)?;
        let mut obligations = HashMap::<_, VecDeque<oneshot::Sender<_>>>::new();
        let (tx, mut rx) = channel::<StoreCommand<K, V>>(100);

        let table_definition: TableDefinition<Bincode<K>, Bincode<V>> =
            TableDefinition::new("q2consensus");

        tokio::spawn(async move {
            while let Some(store_command) = rx.recv().await {
                match store_command {
                    StoreCommand::Write(k, v) => {
                        let db_txn = db
                            .db
                            .begin_write()
                            .expect("failed to start a database write-transaction");
                        {
                            let mut table = db_txn
                                .open_table(table_definition)
                                .expect("failed to open table for write-transaction");
                            table
                                .insert(&k, &v)
                                .expect("failed to insert into database");
                        }
                        db_txn
                            .commit()
                            .expect("failed to commit writes to database");
                    }
                    StoreCommand::Read(k, sender) => {
                        let db_txn = db
                            .db
                            .begin_read()
                            .expect("failed to start database read operation");
                        let table = db_txn
                            .open_table(table_definition)
                            .expect("failed to open table for read operation");
                        let response: Option<V> = table.get(&k).unwrap().map(|x| x.value());

                        // .value();
                        let _ = sender.send(response);
                    }
                    StoreCommand::NotifyRead(k, sender) => {
                        let _k = k.clone();
                        let db_txn = db.db.begin_read().unwrap();
                        let table = db_txn.open_table(table_definition).unwrap();
                        let response: Option<V> = table.get(k).unwrap().map(|x| x.value());
                        match response {
                            None => obligations
                                .entry(_k)
                                .or_insert_with(VecDeque::new)
                                .push_back(sender),
                            Some(_v) => {
                                let _ = sender.send(Ok(_v));
                            }
                        }
                    }
                }
            }
        });
        Ok(Self { channel: tx })
    }

    pub async fn write(&mut self, k: K, v: V) {
        if let Err(e) = self.channel.send(StoreCommand::Write(k, v)).await {
            panic!("Failed to send Write command to store: {}", e);
        }
    }

    pub async fn read(&mut self, k: K) -> StoreResult<Option<V>> {
        let (sender, receiver) = oneshot::channel();
        if let Err(e) = self.channel.send(StoreCommand::Read(k, sender)).await {
            panic!("Failed to send Read command to store: {}", e);
        }
        Ok(receiver
            .await
            .expect("Failed to receive reply to Read Command from store"))
    }

    pub async fn notify_read(&mut self, k: K) -> StoreResult<V> {
        let (sender, receiver) = oneshot::channel();
        if let Err(e) = self.channel.send(StoreCommand::NotifyRead(k, sender)).await {
            panic!("Failed to send Read command to store: {}", e);
        }
        receiver
            .await
            .expect("Failed to receive reply to Read Command from store")
    }
}

/// Wrapper type to handle keys and values using bincode serialization
#[derive(Debug)]
pub struct Bincode<T>(pub T);

impl<T> Value for Bincode<T>
where
    T: Debug + Encode + Decode<()>,
{
    type SelfType<'a>
        = T
    where
        Self: 'a;

    type AsBytes<'a>
        = Vec<u8>
    where
        Self: 'a;

    fn fixed_width() -> Option<usize> {
        None
    }

    fn from_bytes<'a>(data: &'a [u8]) -> Self::SelfType<'a>
    where
        Self: 'a,
    {
        decode_from_slice(data, bincode::config::standard())
            .unwrap()
            .0
    }

    fn as_bytes<'a, 'b: 'a>(value: &'a Self::SelfType<'b>) -> Self::AsBytes<'a>
    where
        Self: 'a,
        Self: 'b,
    {
        encode_to_vec(value, bincode::config::standard()).unwrap()
    }

    fn type_name() -> TypeName {
        TypeName::new(&format!("Bincode<{}>", type_name::<T>()))
    }
}

impl<T> Key for Bincode<T>
where
    T: Debug + Decode<()> + Encode + Ord,
{
    fn compare(data1: &[u8], data2: &[u8]) -> Ordering {
        Self::from_bytes(data1).cmp(&Self::from_bytes(data2))
    }
}
#[cfg(test)]
#[path = "tests/store_test.rs"]
pub mod store_tests;
