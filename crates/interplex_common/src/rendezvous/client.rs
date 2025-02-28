use std::{
    collections::HashMap,
    task::{Context, Poll}, time::Duration,
};

use chrono::{DateTime, TimeDelta, Utc};
use libp2p::{
    core::{transport::PortUse, Endpoint},
    futures::{self, future::BoxFuture, stream::FuturesUnordered, FutureExt, StreamExt as _},
    request_response::{Config, OutboundRequestId, ProtocolSupport},
    swarm::{
        ConnectionDenied, ConnectionId, ExternalAddresses, FromSwarm, NetworkBehaviour, THandler,
        THandlerInEvent, THandlerOutEvent, ToSwarm,
    },
    Multiaddr, PeerId, StreamProtocol,
};
use uuid::Uuid;

use crate::{
    error::{IResult, InterplexError},
    identification::NodeIdentifier,
};

use super::{
    message::{RendezvousCommand, RendezvousRequest, RendezvousResponse},
    registrations::Registration,
};

pub struct Behaviour {
    inner: libp2p::request_response::cbor::Behaviour<RendezvousRequest, RendezvousResponse>,
    identity: NodeIdentifier,
    processing_requests: HashMap<OutboundRequestId, (PeerId, RendezvousCommand)>,
    peers: HashMap<PeerId, (PeerId, Registration, Uuid)>, // {peer_id: (rdv_id, peer)}
    expiring_peers: FuturesUnordered<BoxFuture<'static, (PeerId, Uuid)>>,
    expiring_registrations: FuturesUnordered<BoxFuture<'static, (PeerId, Uuid)>>,
    addresses: ExternalAddresses,
    rendezvous_points: HashMap<PeerId, (DateTime<Utc>, Uuid)>,
}

const REGISTRATION_BUFFER: TimeDelta = TimeDelta::minutes(1);

#[derive(Clone, Debug)]
pub enum Event {
    Registered {
        request: OutboundRequestId,
        rendezvous_node: PeerId,
        lifetime: DateTime<Utc>,
    },
    RegisterFailed {
        request: OutboundRequestId,
        rendezvous_node: PeerId,
        error: InterplexError,
    },
    Deregistered {
        request: OutboundRequestId,
        rendezvous_node: PeerId,
    },
    DeregisterFailed {
        request: OutboundRequestId,
        rendezvous_node: PeerId,
        error: InterplexError,
    },
    Discovered {
        request: OutboundRequestId,
        rendezvous_node: PeerId,
        peers: Vec<Registration>,
    },
    DiscoverFailed {
        request: OutboundRequestId,
        rendezvous_node: PeerId,
        error: InterplexError,
    },
    Found {
        request: OutboundRequestId,
        rendezvous_node: PeerId,
        key: String,
        peer: Registration,
    },
    NotFound {
        request: OutboundRequestId,
        rendezvous_node: PeerId,
        key: String,
    },
    FindFailed {
        request: OutboundRequestId,
        rendezvous_node: PeerId,
        error: InterplexError,
    },
    Groups {
        request: OutboundRequestId,
        rendezvous_node: PeerId,
        groups: Vec<String>,
    },
    GroupsFailed {
        request: OutboundRequestId,
        rendezvous_node: PeerId,
        error: InterplexError,
    },
    PeerExpired {
        rendezvous_node: PeerId,
        registration: Registration,
    },
    RegistrationExpired {
        rendezvous_node: PeerId,
    },
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = <libp2p::request_response::cbor::Behaviour<
        RendezvousRequest,
        RendezvousResponse,
    > as NetworkBehaviour>::ConnectionHandler;
    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner.handle_established_inbound_connection(
            connection_id,
            peer,
            local_addr,
            remote_addr,
        )
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
        port_use: PortUse,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner.handle_established_outbound_connection(
            connection_id,
            peer,
            addr,
            role_override,
            port_use,
        )
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        self.inner
            .on_connection_handler_event(peer_id, connection_id, event);
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        let changed = self.addresses.on_swarm_event(&event);

        self.inner.on_swarm_event(event);

        if changed && self.addresses.iter().count() > 0 {
            let registered = self.rendezvous_points.clone();
            for (node, _) in registered {
                let _ = self.register(&node);
            }
        }
    }

    fn handle_pending_outbound_connection(
        &mut self,
        _connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        _addresses: &[Multiaddr],
        _effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        let peer = match maybe_peer {
            None => return Ok(vec![]),
            Some(peer) => peer,
        };

        let addresses = self
            .peers
            .iter()
            .filter_map(|(candidate, (_, Registration { addresses, .. }, _))| {
                (candidate == &peer).then_some(addresses)
            })
            .flatten()
            .cloned()
            .collect();

        Ok(addresses)
    }

    #[tracing::instrument(level = "trace", name = "NetworkBehaviour::poll", skip(self, cx))]
    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        use libp2p::request_response as req_res;

        loop {
            match self.inner.poll(cx) {
                Poll::Ready(ToSwarm::GenerateEvent(req_res::Event::Message {
                    message:
                        req_res::Message::Response {
                            request_id,
                            response,
                        },
                    ..
                })) => {
                    if let Some(event) = self.handle_response(&request_id, response) {
                        return Poll::Ready(ToSwarm::GenerateEvent(event));
                    }

                    continue; // not a request we care about
                }
                Poll::Ready(ToSwarm::GenerateEvent(req_res::Event::OutboundFailure {
                    request_id,
                    ..
                })) => {
                    if let Some(event) = self.event_for_outbound_failure(&request_id) {
                        return Poll::Ready(ToSwarm::GenerateEvent(event));
                    }

                    continue; // not a request we care about
                }
                Poll::Ready(ToSwarm::GenerateEvent(
                    req_res::Event::InboundFailure { .. }
                    | req_res::Event::ResponseSent { .. }
                    | req_res::Event::Message {
                        message: req_res::Message::Request { .. },
                        ..
                    },
                )) => {
                    unreachable!("rendezvous clients never receive requests")
                }
                Poll::Ready(other) => {
                    let new_to_swarm =
                        other.map_out(|_| unreachable!("we manually map `GenerateEvent` variants"));

                    return Poll::Ready(new_to_swarm);
                }
                Poll::Pending => {}
            }

            if let Poll::Ready(Some((expired_peer, key))) =
                self.expiring_peers.poll_next_unpin(cx)
            {
                if let Some((peer, registration, current_key)) = self.peers.clone().get(&expired_peer) {
                    if current_key == &key {
                        self.peers.remove(&expired_peer);
                            return Poll::Ready(ToSwarm::GenerateEvent(Event::PeerExpired {
                            rendezvous_node: peer.clone(),
                            registration: registration.clone(),
                        }));
                    }
                }
            }

            if let Poll::Ready(Some((expired_registration, key))) =
                self.expiring_registrations.poll_next_unpin(cx)
            {
                if let Some((_, current_key)) = self.rendezvous_points.clone().get(&expired_registration) {
                    if current_key == &key {
                        self.rendezvous_points.remove(&expired_registration);
                        return Poll::Ready(ToSwarm::GenerateEvent(Event::RegistrationExpired { rendezvous_node: expired_registration.clone()}));
                    }
                }
            }

            return Poll::Pending;
        }
    }
}

impl Behaviour {
    pub fn new(identifier: NodeIdentifier) -> Self {
        Self {
            inner: libp2p::request_response::cbor::Behaviour::<RendezvousRequest, RendezvousResponse>::new([(
                StreamProtocol::new("/interplex/rendezvous"),
                ProtocolSupport::Full,
            )], Config::default()),
            identity: identifier,
            processing_requests: Default::default(),
            peers: Default::default(),
            expiring_peers: FuturesUnordered::from_iter(vec![
                futures::future::pending().boxed()
            ]),
            expiring_registrations: FuturesUnordered::from_iter(vec![
                futures::future::pending().boxed()
            ]),
            addresses: Default::default(),
            rendezvous_points: Default::default()
        }
    }

    fn send_request(&mut self, target: &PeerId, command: RendezvousCommand) -> OutboundRequestId {
        let req_id = self.inner.send_request(
            target,
            RendezvousRequest {
                source: self.identity.clone(),
                command: command.clone(),
            },
        );
        self.processing_requests
            .insert(req_id.clone(), (target.clone(), command.clone()));
        req_id
    }

    pub fn register(&mut self, target: &PeerId) -> IResult<OutboundRequestId> {
        if self.addresses.as_slice().len() > 0 {
            Ok(self.send_request(
                target,
                RendezvousCommand::Register(self.addresses.as_slice().to_vec()),
            ))
        } else {
            Err(InterplexError::NodeInaccessible)
        }
    }

    pub fn deregister(&mut self, target: &PeerId) -> OutboundRequestId {
        let rid = self.send_request(target, RendezvousCommand::Deregister);
        self.rendezvous_points.remove(target);
        rid
    }

    pub fn discover(
        &mut self,
        target: &PeerId,
        group: Option<String>,
    ) -> OutboundRequestId {
        self.send_request(
            target,
            RendezvousCommand::Discover(group.and_then(|g| Some(g))),
        )
    }

    pub fn find(
        &mut self,
        target: &PeerId,
        namespace: impl AsRef<str>,
        group: impl AsRef<str>,
        peer_id: impl Into<PeerId>,
    ) -> OutboundRequestId {
        let resolved = format!(
            "{}/{}/{}",
            namespace.as_ref(),
            group.as_ref(),
            Into::<PeerId>::into(peer_id).to_string()
        );
        self.send_request(target, RendezvousCommand::Find(resolved))
    }

    pub fn groups(&mut self, target: &PeerId) -> OutboundRequestId {
        self.send_request(target, RendezvousCommand::Groups)
    }

    fn event_for_outbound_failure(&mut self, req_id: &OutboundRequestId) -> Option<Event> {
        if let Some((target, command)) = self.processing_requests.remove(req_id) {
            let err = InterplexError::RequestDispatch {
                peer: target.clone(),
                namespace: String::from("rendezvous"),
                command: format!("{:?}", command.clone()),
            };
            Some(match command {
                RendezvousCommand::Register(_) => Event::RegisterFailed {
                    request: *req_id,
                    rendezvous_node: target,
                    error: err,
                },
                RendezvousCommand::Deregister => Event::DeregisterFailed {
                    request: *req_id,
                    rendezvous_node: target,
                    error: err,
                },
                RendezvousCommand::Discover(_) => Event::DiscoverFailed {
                    request: *req_id,
                    rendezvous_node: target,
                    error: err,
                },
                RendezvousCommand::Find(_) => Event::FindFailed {
                    request: *req_id,
                    rendezvous_node: target,
                    error: err,
                },
                RendezvousCommand::Groups => Event::GroupsFailed {
                    request: *req_id,
                    rendezvous_node: target,
                    error: err,
                },
            })
        } else {
            None
        }
    }

    fn handle_response(&mut self, req_id: &OutboundRequestId, response: RendezvousResponse) -> Option<Event> {
        if let Some((target, command)) = self.processing_requests.remove(req_id) {
            match response {
                RendezvousResponse::Register(Ok(next_registration)) => {
                    let key = Uuid::new_v4();
                    self.rendezvous_points.insert(target.clone(), (next_registration.clone(), key.clone()));
                    let async_target = target.clone();
                    let async_expire = next_registration.clone();
                    self.expiring_registrations.push(async move {
                        futures_timer::Delay::new((async_expire - Utc::now() - REGISTRATION_BUFFER).to_std().expect("Refresh time out of int range").max(Duration::from_secs(0))).await;
                        (async_target, key)
                    }.boxed());
                    Some(Event::Registered { request: *req_id, rendezvous_node: target, lifetime: next_registration })
                },
                RendezvousResponse::Register(Err(e)) => Some(Event::RegisterFailed { request: *req_id, rendezvous_node: target, error: e }),
                RendezvousResponse::Deregister(Ok(_)) => {
                    self.rendezvous_points.remove(&target);
                    Some(Event::Deregistered { request: *req_id, rendezvous_node: target })
                },
                RendezvousResponse::Deregister(Err(e)) => Some(Event::DeregisterFailed { request: *req_id, rendezvous_node: target, error: e }),
                RendezvousResponse::Discover(Ok(registrations)) => {
                    for registration in registrations.clone() {
                        let key = Uuid::new_v4();
                        self.peers.insert(registration.identity.peer_id.clone(), (target.clone(), registration.clone(), key.clone()));
                        let async_target = registration.identity.peer_id.clone();
                        let async_expire = registration.expiration();
                        self.expiring_peers.push(async move {
                            futures_timer::Delay::new((async_expire - Utc::now()).to_std().expect("Refresh time out of int range").max(Duration::from_secs(0))).await;
                            (async_target, key)
                        }.boxed());
                    }

                    Some(Event::Discovered { request: *req_id, rendezvous_node: target, peers: registrations })
                },
                RendezvousResponse::Discover(Err(e)) => Some(Event::DiscoverFailed { request: *req_id, rendezvous_node: target, error: e }),
                RendezvousResponse::Find(Ok(Some(registration))) => {
                    let key = Uuid::new_v4();
                    self.peers.insert(registration.identity.peer_id.clone(), (target.clone(), registration.clone(), key.clone()));
                    let async_target = registration.identity.peer_id.clone();
                    let async_expire = registration.expiration();
                    self.expiring_peers.push(async move {
                        futures_timer::Delay::new((async_expire - Utc::now()).to_std().expect("Refresh time out of int range").max(Duration::from_secs(0))).await;
                        (async_target, key)
                    }.boxed());
                    
                    Some(Event::Found {request: *req_id, rendezvous_node: target, key: registration.clone().identity.key(), peer: registration})
                },
                RendezvousResponse::Find(Ok(None)) => Some(Event::NotFound { request: *req_id, rendezvous_node: target, key: if let RendezvousCommand::Find(key) = command {key} else {String::new()} }),
                RendezvousResponse::Find(Err(e)) => Some(Event::FindFailed { request: *req_id, rendezvous_node: target, error: e }),
                RendezvousResponse::Groups(Ok(groups)) => Some(Event::Groups { request: *req_id, rendezvous_node: target, groups }),
                RendezvousResponse::Groups(Err(e)) => Some(Event::GroupsFailed { request: *req_id, rendezvous_node: target, error: e })
            }
        } else {
            None
        }
    }
}
