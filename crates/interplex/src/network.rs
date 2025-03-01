use std::sync::Arc;

use async_channel::{Receiver, Sender};
use interplex_common::identification::NodeIdentifier;
use libp2p::{autonat, floodsub, identify, ping, swarm::NetworkBehaviour, upnp, Swarm};
use tokio::sync::Mutex;

use crate::ipc::{CommandWrapper, Event};

#[derive(NetworkBehaviour)]
pub(crate) struct NodeBehaviour {
    rendezvous: interplex_common::rendezvous::client::Behaviour,
    floodsub: floodsub::Floodsub,
    autonat: autonat::Behaviour,
    identify: identify::Behaviour,
    stream: libp2p_stream::Behaviour,
    upnp: upnp::tokio::Behaviour,
    ping: ping::Behaviour
}

pub(crate) struct NetworkHandler {
    commands: Receiver<CommandWrapper>,
    events: Sender<Event>,
    swarm: Arc<Mutex<Swarm<NodeBehaviour>>>,
    identifier: NodeIdentifier,
}