use std::task::{Context, Poll};

use crate::{error::InterplexError, identification::NodeIdentifier};
use chrono::TimeDelta;
use derive_builder::Builder;
use libp2p::{
    request_response::{self, ProtocolSupport},
    swarm::{NetworkBehaviour, THandlerInEvent, ToSwarm},
    Multiaddr, PeerId, StreamProtocol,
};

use super::{
    message::{RendezvousCommand, RendezvousRequest, RendezvousResponse},
    registrations::{Registration, Registrations},
};

#[derive(Builder, Clone, Debug)]
#[builder(setter(into, strip_option))]
pub struct Config {
    database: String,

    #[builder(default = "chrono::TimeDelta::hours(12)")]
    max_lifetime: TimeDelta,
}

pub struct Behavior {
    inner: libp2p::request_response::cbor::Behaviour<RendezvousRequest, RendezvousResponse>,
    config: Config,
    registrations: Registrations,
}

#[derive(Clone, Debug)]
pub enum Event {
    CreatedRegistration(Registration),
    RemovedRegistration(NodeIdentifier),
    ExpiredRegistration(Registration),
    RegistrationFailure(NodeIdentifier, InterplexError),
    DeregistrationFailure(NodeIdentifier, InterplexError),
    ServedDiscovery {
        source: NodeIdentifier,
        namespace: String,
        group: Option<String>,
        results: u64,
    },
    FailedDiscovery {
        source: NodeIdentifier,
        namespace: String,
        group: Option<String>,
        error: InterplexError,
    },
    ServedFind {
        source: NodeIdentifier,
        result: Option<Registration>,
    },
    FailedFind {
        source: NodeIdentifier,
        error: InterplexError,
    },
    ServedGroups {
        source: NodeIdentifier,
        result: Vec<String>,
    },
    FailedGroups {
        source: NodeIdentifier,
        error: InterplexError,
    },
}

impl NetworkBehaviour for Behavior {
    type ConnectionHandler = <libp2p::request_response::cbor::Behaviour<
        RendezvousRequest,
        RendezvousResponse,
    > as NetworkBehaviour>::ConnectionHandler;

    type ToSwarm = Event;

    fn handle_established_inbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        peer: libp2p::PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        self.inner.handle_established_inbound_connection(
            _connection_id,
            peer,
            local_addr,
            remote_addr,
        )
    }

    fn handle_established_outbound_connection(
        &mut self,
        _connection_id: libp2p::swarm::ConnectionId,
        peer: libp2p::PeerId,
        addr: &Multiaddr,
        role_override: libp2p::core::Endpoint,
        port_use: libp2p::core::transport::PortUse,
    ) -> Result<libp2p::swarm::THandler<Self>, libp2p::swarm::ConnectionDenied> {
        self.inner.handle_established_outbound_connection(
            _connection_id,
            peer,
            addr,
            role_override,
            port_use,
        )
    }

    fn on_swarm_event(&mut self, event: libp2p::swarm::FromSwarm) {
        self.inner.on_swarm_event(event);
    }

    fn on_connection_handler_event(
        &mut self,
        _peer_id: libp2p::PeerId,
        _connection_id: libp2p::swarm::ConnectionId,
        _event: libp2p::swarm::THandlerOutEvent<Self>,
    ) {
        self.inner
            .on_connection_handler_event(_peer_id, _connection_id, _event);
    }

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<ToSwarm<Self::ToSwarm, THandlerInEvent<Self>>> {
        if let Ok(Some(expired)) = self.registrations.poll(self.config.max_lifetime) {
            return Poll::Ready(ToSwarm::GenerateEvent(Event::ExpiredRegistration(expired)));
        }
        loop {
            if let Poll::Ready(to_swarm) = self.inner.poll(cx) {
                match to_swarm {
                    ToSwarm::GenerateEvent(libp2p::request_response::Event::Message {
                        peer: peer_id,
                        message:
                            libp2p::request_response::Message::Request {
                                request, channel, ..
                            },
                        ..
                    }) => {
                        if let Some((event, response)) = self.handle_request(peer_id, request) {
                            if let Some(resp) = response {
                                self.inner
                                    .send_response(channel, resp)
                                    .expect("Send response");
                            }

                            return Poll::Ready(ToSwarm::GenerateEvent(event));
                        }

                        continue;
                    }
                    ToSwarm::GenerateEvent(libp2p::request_response::Event::InboundFailure {
                        peer,
                        request_id,
                        error,
                        ..
                    }) => {
                        tracing::warn!(
                            %peer,
                            request=%request_id,
                            "Inbound request with peer failed: {error}"
                        );

                        continue;
                    }
                    ToSwarm::GenerateEvent(libp2p::request_response::Event::ResponseSent {
                        ..
                    })
                    | ToSwarm::GenerateEvent(libp2p::request_response::Event::Message {
                        peer: _,
                        message: libp2p::request_response::Message::Response { .. },
                        ..
                    })
                    | ToSwarm::GenerateEvent(libp2p::request_response::Event::OutboundFailure {
                        ..
                    }) => {
                        continue;
                    }
                    other => {
                        let new_to_swarm = other
                            .map_out(|_| unreachable!("we manually map `GenerateEvent` variants"));

                        return Poll::Ready(new_to_swarm);
                    }
                };
            }

            return Poll::Pending;
        }
    }
}

impl Behavior {
    pub fn new(config: Config) -> Self {
        Self {
            inner: libp2p::request_response::cbor::Behaviour::new(
                [(
                    StreamProtocol::new("/interplex/rendezvous"),
                    ProtocolSupport::Full,
                )],
                request_response::Config::default(),
            ),
            config: config.clone(),
            registrations: Registrations::new(config.database),
        }
    }

    pub fn handle_request(
        &self,
        _: PeerId,
        request: RendezvousRequest,
    ) -> Option<(Event, Option<RendezvousResponse>)> {
        match request.command.clone() {
            RendezvousCommand::Register(addresses) => {
                match self
                    .registrations
                    .register(request.source.clone(), addresses, self.config.max_lifetime)
                {
                    Ok(reg) => Some((
                        Event::CreatedRegistration(reg.clone()),
                        Some(RendezvousResponse::Register(Ok(
                            reg.last_registration + self.config.max_lifetime
                        ))),
                    )),
                    Err(e) => Some((
                        Event::RegistrationFailure(request.source.clone(), e.clone()),
                        Some(RendezvousResponse::Register(Err(e.clone()))),
                    )),
                }
            }
            RendezvousCommand::Deregister => {
                match self.registrations.deregister(request.source.clone()) {
                    Ok(()) => Some((
                        Event::RemovedRegistration(request.source.clone()),
                        Some(RendezvousResponse::Deregister(Ok(()))),
                    )),
                    Err(e) => Some((
                        Event::DeregistrationFailure(request.source.clone(), e.clone()),
                        Some(RendezvousResponse::Deregister(Err(e.clone()))),
                    )),
                }
            }
            RendezvousCommand::Discover(group) => {
                match self
                    .registrations
                    .discover(request.source.clone(), group.clone())
                {
                    Ok(results) => Some((
                        Event::ServedDiscovery {
                            source: request.source.clone(),
                            namespace: request.source.namespace.clone(),
                            group: group.clone(),
                            results: results.len() as u64,
                        },
                        Some(RendezvousResponse::Discover(Ok(results))),
                    )),
                    Err(e) => Some((
                        Event::FailedDiscovery {
                            source: request.source.clone(),
                            namespace: request.source.namespace.clone(),
                            group: group.clone(),
                            error: e.clone(),
                        },
                        Some(RendezvousResponse::Discover(Err(e.clone()))),
                    )),
                }
            }
            RendezvousCommand::Find(key) => match self.registrations.get(key.clone()) {
                Ok(result) => Some((
                    Event::ServedFind {
                        source: request.source.clone(),
                        result: result.clone(),
                    },
                    Some(RendezvousResponse::Find(Ok(result.clone()))),
                )),
                Err(e) => Some((
                    Event::FailedFind {
                        source: request.source.clone(),
                        error: e.clone(),
                    },
                    Some(RendezvousResponse::Find(Err(e.clone()))),
                )),
            },
            RendezvousCommand::Groups => {
                match self.registrations.groups(request.source.namespace.clone()) {
                    Ok(result) => Some((
                        Event::ServedGroups {
                            source: request.source.clone(),
                            result: result.clone(),
                        },
                        Some(RendezvousResponse::Groups(Ok(result))),
                    )),
                    Err(e) => Some((
                        Event::FailedGroups {
                            source: request.source.clone(),
                            error: e.clone(),
                        },
                        Some(RendezvousResponse::Groups(Err(e.clone()))),
                    )),
                }
            }
        }
    }
}
