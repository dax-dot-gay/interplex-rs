use std::{collections::HashMap, error::Error, sync::Arc};

use async_channel::{Receiver, Sender};
use interplex_common::{error::{IResult, InterplexError}, identification::NodeIdentifier};
use libp2p::{
    autonat, floodsub, futures::StreamExt, identify, identity::Keypair, noise, ping, relay, swarm::{NetworkBehaviour, SwarmEvent}, tcp, upnp, yamux, Multiaddr, PeerId, Stream, StreamProtocol, Swarm, SwarmBuilder
};
use tokio::{select, sync::Mutex, task::JoinHandle};
use uuid::Uuid;

use crate::ipc::{Command, CommandResponse, CommandWrapper, Event};

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

#[derive(Clone)]
pub(crate) struct NetworkHandler {
    commands: Receiver<CommandWrapper>,
    events: Sender<Event>,
    swarm: Arc<Mutex<Swarm<NodeBehaviour>>>,
    identifier: NodeIdentifier,
    topics: Arc<Mutex<Vec<String>>>,
    streams: Arc<Mutex<HashMap<Uuid, Arc<Mutex<Stream>>>>>
}

enum EventType {
    Swarm(SwarmEvent<NodeBehaviourEvent>),
    Command(CommandWrapper),
    Stream(PeerId, Stream)
}

impl NetworkHandler {
    fn make_swarm(identification: NodeIdentifier, keypair: Keypair) -> Result<Swarm<NodeBehaviour>, Box<dyn Error>> {
        Ok(SwarmBuilder::with_existing_identity(keypair)
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
                    identify: identify::Behaviour::new(identify::Config::new(String::from("/interplex"), key.public())),
                    stream: libp2p_stream::Behaviour::default(),
                    upnp: upnp::tokio::Behaviour::default(),
                    ping: ping::Behaviour::default(),
                    relay: relay_client
                }
            })?
            .build())
    }

    pub fn new(
        command_rcv: Receiver<CommandWrapper>,
        event_send: Sender<Event>,
        identification: NodeIdentifier,
        rendezvous_nodes: Vec<Multiaddr>,
        keypair: Keypair,
    ) -> IResult<Self> {
        for rdv in rendezvous_nodes.clone() {
            if !rdv.protocol_stack().any(|p| p == "p2p") {
                return Err(InterplexError::Address { addr: rdv.to_string(), reason: String::from("Rendezvous node address must contain a /p2p/<peer_id> block.") });
            }
        }

        let mut swarm = Self::make_swarm(identification.clone(), keypair).or_else(|e| Err(InterplexError::wrap(e)))?;

        for rdv in rendezvous_nodes.clone() {
            swarm.dial(rdv).or_else(|e| Err(InterplexError::wrap(e)))?;
        }

        Ok(Self {
            commands: command_rcv,
            events: event_send,
            identifier: identification.clone(),
            swarm: Arc::new(Mutex::new(swarm)),
            topics: Arc::new(Mutex::new(Vec::new())),
            streams: Arc::new(Mutex::new(HashMap::new()))
        })

    }

    async fn handle_swarm_event(&self, event: SwarmEvent<NodeBehaviourEvent>) -> IResult<()> {
        Ok(())
    }

    async fn handle_command(&self, command: CommandWrapper) -> IResult<()> {
        Ok(())
    }

    async fn event_loop(self) -> IResult<Self> {
        loop {
            let mut swarm = self.swarm.lock().await;
            let mut control = swarm.behaviour().stream.new_control();
            let mut streams = control.accept(StreamProtocol::new("/interplex/streaming")).or_else(|e| Err(InterplexError::wrap(e)))?;
            let next_event: Option<EventType> = select! {
                event = swarm.select_next_some() => Some(EventType::Swarm(event)),
                event = self.commands.recv() => if let Ok(ev) = event {Some(EventType::Command(ev))} else {None},
                event = streams.next() => if let Some((peer, stream)) = event {Some(EventType::Stream(peer, stream))} else {None}
            };

            if let Some(event) = next_event {
                match event {
                    EventType::Command(CommandWrapper {response_channel, command: Command::ExitLoop}) => {
                        let _ = response_channel.send(Ok(CommandResponse::ExitLoop)).await;
                        response_channel.close();
                        break;
                    },
                    _ => ()
                }
            }
        }

        Ok(self)
    }

    pub fn start_event_loop(self) -> JoinHandle<IResult<Self>> {
        tokio::spawn(async move {
            self.event_loop().await
        })
    }
}
