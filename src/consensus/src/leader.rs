use crate::config::Committee;
use crate::types::Round;
use crypto::PublicKey;

pub type LeaderElector = RRLeaderElector;

pub struct RRLeaderElector {
    committee: Committee,
}

impl RRLeaderElector {
    pub fn new(committee: Committee) -> Self {
        Self { committee }
    }

    /// get the round leader.
    /// This is a simple algorithm for this prototyp.
    /// A more elaborate algorithm that considers `stake` may be implemented
    pub fn get_leader(&self, round: Round) -> PublicKey {
        let mut keys: Vec<_> = self.committee.authorities.keys().cloned().collect();
        keys.sort();
        keys[round as usize % self.committee.size()]
    }
}
