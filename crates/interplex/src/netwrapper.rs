use std::sync::Arc;

use async_channel::{Receiver, Sender};
use interplex_common::identification::NodeIdentifier;
use libp2p::{identity::Keypair, Multiaddr};
use tokio::{sync::Mutex, task::JoinHandle};

use crate::{
    error::CResult,
    ipc::{CommandWrapper, Event},
    network::NetworkHandler,
    Error,
};

pub(crate) enum NetworkState {
    Running(JoinHandle<CResult<NetworkHandler>>),
    Ready(NetworkHandler),
    Failed(Error),
}

#[derive(Clone)]
pub(crate) struct Network {
    state: Arc<Mutex<NetworkState>>,
    commands: Sender<CommandWrapper>,
    events: Receiver<Event>,
}

impl Network {
    pub fn create(
        (command_tx, command_rx): (Sender<CommandWrapper>, Receiver<CommandWrapper>),
        (event_tx, event_rx): (Sender<Event>, Receiver<Event>),
        identifier: NodeIdentifier,
        rendezvous_nodes: Vec<Multiaddr>,
        keypair: Keypair,
    ) -> CResult<Self> {
        Ok(Self {
            state: Arc::new(Mutex::new(NetworkState::Ready(
                NetworkHandler::new(command_rx, event_tx, identifier, rendezvous_nodes, keypair)
                    .or_else(|e| Err(Error::Internal(e)))?,
            ))),
            commands: command_tx,
            events: event_rx,
        })
    }

    pub fn running(&self) -> bool {
        if let NetworkState::Running(handle) = self.state.blocking_lock() {
            !handle.is_finished()
        } else {
            false
        }
    }

    pub fn start(&mut self) -> CResult<()> {
        Ok(())
    }
}
