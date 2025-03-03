use std::{collections::HashMap, error::Error, sync::Arc};

use async_channel::{Receiver, Sender};
use interplex_common::{error::IResult, identification::NodeIdentifier};
use libp2p::{
    autonat, floodsub, identify, identity::Keypair, noise, ping, relay, swarm::NetworkBehaviour, tcp, upnp, yamux, Multiaddr, Stream, Swarm, SwarmBuilder
};
use tokio::{sync::Mutex, task::JoinHandle};
use uuid::Uuid;

use crate::ipc::{CommandWrapper, Event};

#[derive(NetworkBehaviour)]
pub(crate) struct NodeBehaviour {
    rendezvous: interplex_common::rendezvous::client::Behaviour,
    floodsub: floodsub::Floodsub,
    autonat: autonat::Behaviour,
    identify: identify::Behaviour,
    stream: libp2p_stream::Behaviour,
    upnp: upnp::tokio::Behaviour,
    ping: ping::Behaviour,
    relay: relay::client::Behaviour
}

pub(crate) struct NetworkHandler {
    commands: Receiver<CommandWrapper>,
    events: Sender<Event>,
    swarm: Arc<Mutex<Swarm<NodeBehaviour>>>,
    identifier: NodeIdentifier,
    topics: Arc<Mutex<Vec<String>>>,
    streams: Arc<Mutex<HashMap<Uuid, Arc<Mutex<Stream>>>>>,
    eventloop: JoinHandle<IResult<()>>,
}

impl NetworkHandler {
    pub fn new(
        command_rcv: Receiver<CommandWrapper>,
        event_send: Sender<Event>,
        identification: NodeIdentifier,
        rendezvous_nodes: Vec<Multiaddr>,
        keypair: Keypair,
    ) -> Result<Self, Box<dyn Error>> {
        let mut swarm: Swarm<NodeBehaviour> = SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_dns()?
            .with_relay_client(noise::Config::new, yamux::Config::default)?
            .with_behaviour(|key, relay_client| {
                NodeBehaviour {
                    rendezvous: interplex_common::rendezvous::client::Behaviour::new(identification.clone()),
                    floodsub: floodsub::Floodsub::new(key.public().to_peer_id()),
                    autonat: autonat::Behaviour::new(key.public().to_peer_id(), autonat::Config::default()),
                    identify: identify::Behaviour::new(identify::Config::new(String::from("/interplex"), key.public().to_peer_id())),
                    stream: libp2p_stream::Behaviour::default(),
                    upnp: upnp::tokio::Behaviour::default(),
                    ping: ping::Behaviour::default(),
                    relay: relay_client
                }
            })?
            .build();
    }
}
