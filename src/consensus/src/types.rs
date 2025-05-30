/// The default channel capacity for each channel of the consensus.
pub const CHANNEL_CAPACITY: usize = 1_000;

/// The consensus round number.
pub type Round = u64;

pub type Store = store::Store<Vec<u8>, Vec<u8>>;
