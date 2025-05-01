use bincode::{Decode, Encode, decode_from_slice, encode_to_vec};
use core::fmt::Debug;
use redb::{Database, Key, TableDefinition, TypeName, Value};
use std::any::type_name;
use std::cmp::Ordering;
use tokio::sync::mpsc::{Sender, channel};
use tokio::sync::oneshot;

type StoreError = redb::Error;

type StoreResult<T> = Result<T, StoreError>;

pub enum StoreCommand<K, V>
where
    K: Encode + PartialEq + PartialOrd,
    V: Encode + PartialEq,
{
    Write(K, V),
    Read(K, oneshot::Sender<Option<V>>),
}

pub struct DB {
    pub db: Database,
}

pub trait Persist<K, V> {
    fn put(&self, k: K, v: V) -> StoreResult<()>;
    fn get(&self, k: &K) -> StoreResult<Option<V>>;
}
impl DB {
    pub fn new(p: &std::path::Path) -> StoreResult<Self> {
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

impl<K, V> Persist<K, V> for DB
where
    K: Encode + PartialEq + PartialOrd,
    V: Encode + PartialEq,
{
    fn put(&self, k: K, v: V) -> StoreResult<()> {
        todo!()
    }

    fn get(&self, k: &K) -> StoreResult<Option<V>> {
        todo!()
    }
}

#[derive(Clone)]
pub struct Store<K, V>
where
    K: Encode + PartialEq + PartialOrd,
    V: Encode + PartialEq,
{
    channel: Sender<StoreCommand<K, V>>,
}
impl<K, V> Store<K, V>
where
    K: Debug + Encode + PartialEq + PartialOrd + Ord + Encode + Send + Decode<()> + 'static,
    V: Debug + Encode + PartialEq + Send + Encode + Decode<()> + 'static,
{
    pub fn new(path: &std::path::Path) -> StoreResult<Self> {
        let db = DB::new(path).unwrap();

        let table_definition: TableDefinition<Bincode<K>, Bincode<V>> =
            TableDefinition::new("store-table");

        let (tx, mut rx) = channel::<StoreCommand<K, V>>(100);
        tokio::spawn(async move {
            while let Some(store_command) = rx.recv().await {
                match store_command {
                    StoreCommand::Write(k, v) => {
                        let db_txn = db.db.begin_write().unwrap();
                        let mut table = db_txn.open_table(table_definition).unwrap();
                        table.insert(&k, &v).unwrap();
                    }

                    StoreCommand::Read(k, sender) => {
                        let db_txn = db.db.begin_read().unwrap();
                        let table = db_txn.open_table(table_definition).unwrap();
                        let response: Option<V> = table.get(k).unwrap().map(|x| x.value());
                        // .value();
                        let _ = sender.send(response);
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

    pub async fn read(&mut self, k: K) -> Option<V> {
        let (sender, receiver) = oneshot::channel();
        if let Err(e) = self.channel.send(StoreCommand::Read(k, sender)).await {
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
