use std::collections::HashMap;

use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};

use crate::{error::InterplexError, identification::NodeIdentifier};

/// Request wrapper for rendezvous requests
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RendezvousRequest {
    /// Source of the request
    pub source: NodeIdentifier,

    /// Command to execute
    pub command: RendezvousCommand
}

/// Rendezvous command types
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum RendezvousCommand {
    /// Register this peer in the rendezvous server. This has one of the following effects:
    /// - If the peer is already registered: The peer's TTL will refresh, and recorded addresses will be replaced with the new set if provided.
    /// - If the peer is not registered: The peer will be registered, with either the addresses provided or the source address.
    Register(Option<Vec<Multiaddr>>),

    /// De-register the source peer
    Deregister,

    /// Discover all peers in the source's namespace, optionally filtering by group.
    /// Some peers may or may not be returned, based on their discoverability
    Discover(Option<String>),

    /// Attempts to retrieve a peer by ID in the source peer's group
    Find(String)
}

/// Rendezvous response types
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum RendezvousResponse {
    /// Returned if an error occurs
    Error(InterplexError),

    /// Returned on successful registration, with the time remaining until the next required registration/check-in
    Register(chrono::TimeDelta),

    /// Returned on successful de-registration
    Deregister,

    /// Returned on successful discovery operation
    Discover(HashMap<String, (NodeIdentifier, Vec<Multiaddr>)>),

    /// Returned on successful find operation (if peer is not found, returns None)
    Find(Option<(NodeIdentifier, Vec<Multiaddr>)>)
}