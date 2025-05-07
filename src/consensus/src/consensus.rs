use crate::config::{Committee, Parameters};
use crate::core::Core;
use crate::error::ConsensusError;
use crate::helper::Helper;
use crate::leader::LeaderElector;
use crate::mempool::MempoolDriver;
use crate::messages::{Block, ConsensusMessage, TC, Timeout, Vote};
use crate::proposer::Proposer;
use crate::synchronizer::Synchronizer;
use crate::types::{CHANNEL_CAPACITY, Store};
use async_trait::async_trait;
use bytes::Bytes;
use crypto::{Digest, PublicKey, SignatureService};
use futures::SinkExt as _;
use log::info;
use mempool::ConsensusMempoolMessage;
use network::{MessageHandler, Receiver as NetworkReceiver, Writer};
use std::error::Error;
use tokio::sync::mpsc::{Receiver, Sender, channel};

pub struct Consensus;

impl Consensus {
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        name: PublicKey,
        committee: Committee,
        parameters: Parameters,
        signature_service: SignatureService,
        store: Store,
        rx_mempool: Receiver<Digest>,
        tx_mempool: Sender<ConsensusMempoolMessage>,
        tx_commit: Sender<Block>,
    ) {
        // NOTE: This log entry is used to compute performance.
        parameters.log();

        let (tx_consensus, rx_consensus) = channel(CHANNEL_CAPACITY);
        let (tx_loopback, rx_loopback) = channel(CHANNEL_CAPACITY);
        let (tx_proposer, rx_proposer) = channel(CHANNEL_CAPACITY);
        let (tx_helper, rx_helper) = channel(CHANNEL_CAPACITY);

        // Spawn the network receiver.
        let mut address = committee
            .address(&name)
            .expect("Our public key is not in the committee");
        address.set_ip("0.0.0.0".parse().unwrap());
        NetworkReceiver::spawn(
            address,
            /* handler */
            ConsensusReceiverHandler {
                tx_consensus,
                tx_helper,
            },
        );
        info!(
            "Node {} listening to consensus messages on {}",
            name, address
        );

        // Make the leader election module.
        let leader_elector = LeaderElector::new(committee.clone());

        // Make the mempool driver.
        let mempool_driver = MempoolDriver::new(store.clone(), tx_mempool, tx_loopback.clone());

        // Make the synchronizer.
        let synchronizer = Synchronizer::new(
            name,
            committee.clone(),
            store.clone(),
            tx_loopback.clone(),
            parameters.sync_retry_delay,
        );

        // Spawn the consensus core.
        Core::spawn(
            name,
            committee.clone(),
            signature_service.clone(),
            store.clone(),
            leader_elector,
            mempool_driver,
            synchronizer,
            parameters.timeout_delay,
            /* rx_message */ rx_consensus,
            rx_loopback,
            tx_proposer,
            tx_commit,
        );

        // Spawn the block proposer.
        Proposer::spawn(
            name,
            committee.clone(),
            signature_service,
            rx_mempool,
            /* rx_message */ rx_proposer,
            tx_loopback,
        );

        // Spawn the helper module.
        Helper::spawn(committee, store, /* rx_requests */ rx_helper);
    }
}

/// Defines how the network receiver handles incoming primary messages.
#[derive(Clone)]
struct ConsensusReceiverHandler {
    tx_consensus: Sender<ConsensusMessage>,
    tx_helper: Sender<(Digest, PublicKey)>,
}

#[async_trait]
impl MessageHandler for ConsensusReceiverHandler {
    async fn dispatch(&self, writer: &mut Writer, serialized: Bytes) -> Result<(), Box<dyn Error>> {
        // Deserialize and parse the message.
        match bincode::serde::decode_from_slice(&serialized, bincode::config::standard())
            .map_err(|e| ConsensusError::DecodeError(Box::new(e)))?
        {
            (ConsensusMessage::SyncRequest(missing, origin), _) => self
                .tx_helper
                .send((missing, origin))
                .await
                .expect("Failed to send consensus message"),
            (message @ ConsensusMessage::Propose(..), _) => {
                // Reply with an ACK.
                let _ = writer.send(Bytes::from("Ack")).await;

                // Pass the message to the consensus core.
                self.tx_consensus
                    .send(message)
                    .await
                    .expect("Failed to consensus message")
            }
            (message, _) => self
                .tx_consensus
                .send(message)
                .await
                .expect("Failed to consensus message"),
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/consensus_tests.rs"]
pub mod consensus_tests;
