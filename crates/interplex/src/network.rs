use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use async_channel::{Receiver, Sender};
use interplex_common::{
    error::{IResult, InterplexError},
    identification::NodeIdentifier,
    rendezvous,
};
use libp2p::{
    autonat,
    floodsub::{self, FloodsubEvent, Topic},
    futures::{AsyncReadExt, AsyncWriteExt as _, StreamExt},
    identify,
    identity::Keypair,
    multiaddr::Protocol,
    noise, ping, relay,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, upnp, yamux, Multiaddr, PeerId, Stream, StreamProtocol, Swarm, SwarmBuilder,
};
use tokio::{
    select,
    sync::Mutex,
    task::{JoinHandle, JoinSet},
};
use uuid::Uuid;

use crate::{
    error::{CResult, Error},
    ipc::{Command, CommandResponse, CommandWrapper, Event, StreamRole},
};

#[derive(NetworkBehaviour)]
pub(crate) struct NodeBehaviour {
    rendezvous: interplex_common::rendezvous::client::Behaviour,
    floodsub: floodsub::Floodsub,
    autonat: autonat::Behaviour,
    identify: identify::Behaviour,
    stream: libp2p_stream::Behaviour,
    upnp: upnp::tokio::Behaviour,
    ping: ping::Behaviour,
    relay: relay::client::Behaviour,
}

#[derive(Clone)]
pub(crate) struct NetworkHandler {
    commands: Receiver<CommandWrapper>,
    events: Sender<Event>,
    swarm: Arc<Mutex<Swarm<NodeBehaviour>>>,
    identifier: NodeIdentifier,
    topics: Arc<Mutex<Vec<String>>>,
    streams: Arc<Mutex<HashMap<Uuid, (PeerId, StreamRole, Arc<Mutex<Stream>>)>>>,
    rendezvous_points: Arc<Mutex<HashMap<PeerId, Multiaddr>>>,
    peers: Arc<Mutex<HashMap<PeerId, (HashSet<PeerId>, NodeIdentifier)>>>,
}

enum EventType {
    Swarm(SwarmEvent<NodeBehaviourEvent>),
    Command(CommandWrapper),
    Stream(PeerId, Stream),
}

impl NetworkHandler {
    fn make_swarm(
        identification: NodeIdentifier,
        keypair: Keypair,
    ) -> Result<Swarm<NodeBehaviour>, Box<dyn std::error::Error>> {
        Ok(SwarmBuilder::with_existing_identity(keypair)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_dns()?
            .with_relay_client(noise::Config::new, yamux::Config::default)?
            .with_behaviour(|key, relay_client| NodeBehaviour {
                rendezvous: interplex_common::rendezvous::client::Behaviour::new(
                    identification.clone(),
                ),
                floodsub: floodsub::Floodsub::new(key.public().to_peer_id()),
                autonat: autonat::Behaviour::new(
                    key.public().to_peer_id(),
                    autonat::Config::default(),
                ),
                identify: identify::Behaviour::new(identify::Config::new(
                    String::from("/interplex"),
                    key.public(),
                )),
                stream: libp2p_stream::Behaviour::default(),
                upnp: upnp::tokio::Behaviour::default(),
                ping: ping::Behaviour::default(),
                relay: relay_client,
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
        let mut rendezvous_points: HashMap<PeerId, Multiaddr> = HashMap::new();
        for rdv in rendezvous_nodes.clone() {
            if let Some(peer) = rdv
                .iter()
                .filter_map(|p| {
                    if let Protocol::P2p(peer) = p {
                        Some(peer.clone())
                    } else {
                        None
                    }
                })
                .last()
            {
                rendezvous_points.insert(peer, rdv);
            } else {
                return Err(InterplexError::Address {
                    addr: rdv.to_string(),
                    reason: String::from(
                        "Rendezvous node address must contain a /p2p/<peer_id> block.",
                    ),
                });
            }
        }

        let mut swarm = Self::make_swarm(identification.clone(), keypair)
            .or_else(|e| Err(InterplexError::wrap(e)))?;

        for rdv in rendezvous_nodes.clone() {
            swarm.dial(rdv).or_else(|e| Err(InterplexError::wrap(e)))?;
        }

        Ok(Self {
            commands: command_rcv,
            events: event_send,
            identifier: identification.clone(),
            swarm: Arc::new(Mutex::new(swarm)),
            topics: Arc::new(Mutex::new(Vec::new())),
            streams: Arc::new(Mutex::new(HashMap::new())),
            rendezvous_points: Arc::new(Mutex::new(rendezvous_points)),
            peers: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    async fn handle_swarm_event(&self, event: SwarmEvent<NodeBehaviourEvent>) -> () {
        let mut swarm = self.swarm.lock().await;
        let event: Option<Event> = match event {
            SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                if let Some((peer, _)) = self.rendezvous_points.lock().await.get_key_value(&peer_id)
                {
                    let _ = swarm.behaviour_mut().rendezvous.register(peer);
                }

                None
            }
            SwarmEvent::Behaviour(NodeBehaviourEvent::Rendezvous(rdv_event)) => match rdv_event {
                rendezvous::client::Event::Discovered {
                    peers,
                    rendezvous_node,
                    ..
                } => {
                    let mut new_peers: HashMap<PeerId, NodeIdentifier> = HashMap::new();
                    for peer in peers {
                        if let Some((ref mut rendezvous_nodes, _)) =
                            self.peers.lock().await.get_mut(&peer.identity.peer_id)
                        {
                            rendezvous_nodes.insert(peer.identity.peer_id.clone());
                        } else {
                            let mut nodes: HashSet<PeerId> = HashSet::new();
                            nodes.insert(rendezvous_node);
                            self.peers.lock().await.insert(
                                peer.identity.peer_id.clone(),
                                (nodes, peer.identity.clone()),
                            );
                            new_peers.insert(peer.identity.peer_id.clone(), peer.identity.clone());
                        }
                    }

                    Some(Event::DiscoveredPeers(new_peers))
                }
                rendezvous::client::Event::PeerExpired {
                    rendezvous_node,
                    registration,
                } => {
                    let mut locked = self.peers.lock().await;
                    if let Some((peers, _)) = locked.get(&registration.identity.peer_id).clone() {
                        if peers.len() == 1 && peers.contains(&rendezvous_node) {
                            locked.remove(&registration.identity.peer_id);
                            Some(Event::LostPeer(registration.identity.clone()))
                        } else {
                            if let Some((ref mut peers, _)) =
                                locked.get_mut(&registration.identity.peer_id)
                            {
                                peers.remove(&rendezvous_node);
                            }
                            None
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            },
            SwarmEvent::Behaviour(NodeBehaviourEvent::Floodsub(FloodsubEvent::Message(
                message,
            ))) => Some(Event::SubscribedMessage {
                source: message.source.clone(),
                data: message.data.clone(),
                topics: message.topics.iter().map(|t| t.id().to_string()).collect(),
            }),
            _ => None,
        };

        if let Some(evt) = event {
            let _ = self.events.send(evt).await;
        }
    }

    async fn handle_command(&self, command: CommandWrapper) -> () {
        let mut swarm = self.swarm.lock().await;
        let mut streams = self.streams.lock().await;

        let result = match command.command.clone() {
            Command::OpenStream(peer) => {
                match swarm
                    .behaviour()
                    .stream
                    .new_control()
                    .open_stream(peer, StreamProtocol::new("/interplex/streaming"))
                    .await
                {
                    Ok(stream) => {
                        let key = Uuid::new_v4();
                        streams.insert(
                            key.clone(),
                            (
                                peer.clone(),
                                StreamRole::Source,
                                Arc::new(Mutex::new(stream)),
                            ),
                        );
                        let _ = self
                            .events
                            .send(Event::StreamOpened {
                                stream_id: key.clone(),
                                remote: peer.clone(),
                                role: StreamRole::Source,
                            })
                            .await;
                        Ok(CommandResponse::OpenStream(key))
                    }
                    Err(e) => Err(Error::open_stream(peer, e)),
                }
            }
            Command::CloseStream(stream_id) => {
                if let Some((peer, role, stream)) = streams.remove(&stream_id) {
                    match stream.lock().await.close().await {
                        Ok(_) => {
                            let _ = self
                                .events
                                .send(Event::StreamClosed {
                                    stream_id,
                                    remote: peer,
                                    role,
                                })
                                .await;
                            Ok(CommandResponse::CloseStream)
                        }
                        Err(error) => Err(Error::close_stream(stream_id, peer, error)),
                    }
                } else {
                    Err(Error::unknown_stream(stream_id))
                }
            }
            Command::ReadStream {
                stream_id,
                buf_size,
            } => {
                if let Some((peer, _, stream)) = streams.get(&stream_id) {
                    let mut locked = stream.lock().await;
                    let mut buffer = vec![0u8; buf_size];

                    match locked.read(&mut buffer).await {
                        Ok(bytes_read) => Ok(CommandResponse::ReadStream {
                            data: buffer,
                            bytes_read,
                        }),
                        Err(error) => {
                            Err(Error::read_stream(stream_id, peer.clone(), buf_size, error))
                        }
                    }
                } else {
                    Err(Error::unknown_stream(stream_id))
                }
            }
            Command::WriteStream { stream_id, data } => {
                if let Some((peer, _, stream)) = streams.get(&stream_id) {
                    let mut locked = stream.lock().await;

                    match locked.write_all(&data).await {
                        Ok(_) => Ok(CommandResponse::WriteStream(data.len())),
                        Err(error) => Err(Error::write_stream(stream_id, peer.clone(), error)),
                    }
                } else {
                    Err(Error::unknown_stream(stream_id))
                }
            }
            Command::Subscribe(topics) => {
                let mut subs = self.topics.lock().await;
                for topic in topics {
                    if swarm.behaviour_mut().floodsub.subscribe(Topic::new(&topic)) {
                        subs.push(topic);
                    }
                }

                Ok(CommandResponse::Subscribe)
            }
            Command::Unsubscribe(topics) => {
                let mut subs = self.topics.lock().await;
                for topic in topics.clone() {
                    if swarm
                        .behaviour_mut()
                        .floodsub
                        .unsubscribe(Topic::new(&topic))
                    {
                        *subs = subs
                            .iter()
                            .filter_map(|s| {
                                if topics.contains(s) {
                                    Some(s.clone())
                                } else {
                                    None
                                }
                            })
                            .collect();
                    }
                }

                Ok(CommandResponse::Unsubscribe)
            }
            Command::ExitLoop => Ok(CommandResponse::ExitLoop),
            Command::AddRendezvous(address) => {
                let mut rendezvous_points = self.rendezvous_points.lock().await;
                if let Some(peer) = address
                    .iter()
                    .filter_map(|p| {
                        if let Protocol::P2p(peer) = p {
                            Some(peer)
                        } else {
                            None
                        }
                    })
                    .last()
                {
                    match swarm.dial(address.clone()) {
                        Ok(_) => {
                            rendezvous_points.insert(peer.clone(), address.clone());
                            Ok(CommandResponse::AddRendezvous(peer.clone()))
                        }
                        Err(error) => Err(Error::connection(
                            "connecting to a new rendezvous node",
                            error,
                        )),
                    }
                } else {
                    Err(InterplexError::Address {
                        addr: address.to_string(),
                        reason: String::from(
                            "Invalid rendezvous address: must include a P2P protocol specifier.",
                        ),
                    }
                    .into())
                }
            }
            Command::RemoveRendezvous(peer_id) => {
                let mut rendezvous_points = self.rendezvous_points.lock().await;
                swarm.behaviour_mut().rendezvous.deregister(&peer_id);
                rendezvous_points.remove(&peer_id);
                Ok(CommandResponse::RemoveRendezvous)
            }
            Command::UpdateRemotes(group) => {
                let rendezvous_points = self.rendezvous_points.lock().await;
                for peer in rendezvous_points.keys() {
                    swarm
                        .behaviour_mut()
                        .rendezvous
                        .discover(peer, group.clone());
                }

                Ok(CommandResponse::UpdateRemotes)
            },
            Command::ListPeers => Ok(CommandResponse::ListPeers(self.peers.lock().await.iter().map(|(k, (_, v))| (k.clone(), v.clone())).collect())),
            Command::GetPeer(id) => Ok(CommandResponse::GetPeer(self.peers.lock().await.get(&id).and_then(|(_, node)| Some(node.clone()))))
        };

        let _ = command.response_channel.send(result).await;
    }

    async fn event_loop(self) -> CResult<Self> {
        let mut processing_handlers = JoinSet::<()>::new();

        loop {
            let mut swarm = self.swarm.lock().await;
            let mut control = swarm.behaviour().stream.new_control();
            let mut streams = control
                .accept(StreamProtocol::new("/interplex/streaming"))
                .or_else(|e| Err(InterplexError::wrap(e)))?;
            let next_event: Option<EventType> = select! {
                event = swarm.select_next_some() => Some(EventType::Swarm(event)),
                event = self.commands.recv() => if let Ok(ev) = event {Some(EventType::Command(ev))} else {None},
                event = streams.next() => if let Some((peer, stream)) = event {Some(EventType::Stream(peer, stream))} else {None}
            };

            if let Some(event) = next_event {
                match event {
                    EventType::Command(CommandWrapper {
                        response_channel,
                        command: Command::ExitLoop,
                    }) => {
                        let _ = response_channel.send(Ok(CommandResponse::ExitLoop)).await;
                        response_channel.close();
                        break;
                    }
                    EventType::Command(command) => {
                        let cself = self.clone();
                        processing_handlers
                            .spawn(async move { cself.handle_command(command).await });
                    }
                    EventType::Swarm(event) => {
                        let cself = self.clone();
                        processing_handlers
                            .spawn(async move { cself.handle_swarm_event(event).await });
                    }
                    EventType::Stream(peer, stream) => {
                        let cself = self.clone();
                        processing_handlers.spawn(async move {
                            let mut own_streams = cself.streams.lock().await;
                            let key = Uuid::new_v4();
                            own_streams.insert(
                                key.clone(),
                                (peer.clone(), StreamRole::Sink, Arc::new(Mutex::new(stream))),
                            );
                            let _ = cself
                                .events
                                .send(Event::StreamOpened {
                                    stream_id: key,
                                    remote: peer,
                                    role: StreamRole::Sink,
                                })
                                .await;
                        });
                    }
                }
            }
        }

        Ok(self)
    }

    pub fn start_event_loop(self) -> JoinHandle<CResult<Self>> {
        tokio::spawn(async move { self.event_loop().await })
    }
}
