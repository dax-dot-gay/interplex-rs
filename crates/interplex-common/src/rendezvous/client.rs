use std::collections::HashMap;

use chrono::{DateTime, Utc};
use libp2p::{futures::{future::BoxFuture, stream::FuturesUnordered}, request_response::OutboundRequestId, swarm::ExternalAddresses, Multiaddr, PeerId};

use crate::{error::InterplexError, identification::NodeIdentifier};

use super::{message::{RendezvousCommand, RendezvousRequest, RendezvousResponse}, registrations::Registration};

pub struct Behaviour {
    inner: libp2p::request_response::cbor::Behaviour<RendezvousRequest, RendezvousResponse>,
    identity: NodeIdentifier,
    processing_requests: HashMap<OutboundRequestId, RendezvousCommand>,
    peers: HashMap<PeerId, (PeerId, Registration)>, // {peer_id: (rdv_id, peer)}
    expiring_peers: FuturesUnordered<BoxFuture<'static, PeerId>>,
    expiring_registrations: FuturesUnordered<BoxFuture<'static, PeerId>>,
    addresses: ExternalAddresses,
    rendezvous_points: HashMap<PeerId, Vec<String>>
}

#[derive(Clone, Debug)]
pub enum Event {
    Registered {
        rendezvous_node: PeerId,
        addresses: Vec<Multiaddr>,
        lifetime: DateTime<Utc>
    },
    RegisterFailed {
        rendezvous_node: PeerId,
        error: InterplexError
    },
    Deregistered {
        rendezvous_node: PeerId
    },
    DeregisterFailed {
        rendezvous_node: PeerId,
        error: InterplexError
    },
    Discovered {
        rendezvous_node: PeerId,
        peers: Vec<Registration>
    },
    DiscoverFailed {
        rendezvous_node: PeerId,
        error: InterplexError
    },
    Found {
        rendezvous_node: PeerId,
        key: String,
        peer: Registration
    },
    NotFound {
        rendezvous_node: PeerId,
        key: String
    },
    FindFailed {
        rendezvous_node: PeerId,
        error: InterplexError
    },
    Groups {
        rendezvous_node: PeerId,
        groups: Vec<String>
    },
    GroupsFailed {
        rendezvous_node: PeerId,
        error: InterplexError
    }

}