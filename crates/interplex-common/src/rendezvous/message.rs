use chrono::{DateTime, Utc};
use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};

use crate::{error::IResult, identification::NodeIdentifier};

use super::registrations::Registration;

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
    /// Register this peer in the rendezvous server, replacing existing addresses and updating the TTL
    Register(Vec<Multiaddr>),

    /// De-register the source peer
    Deregister,

    /// Discover all peers in the source's namespace, optionally filtering by group.
    /// Some peers may or may not be returned, based on their discoverability
    Discover(Option<String>),

    /// Attempts to retrieve a peer by locator key ("<namespace>/<group>/<id>")
    Find(String),

    /// Return a list of all groups in the source peer's namespace
    Groups
}

/// Rendezvous response types
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum RendezvousResponse {
    /// Returned on successful registration, with current expiration time
    Register(IResult<DateTime<Utc>>),

    /// Returned on successful de-registration
    Deregister(IResult<()>),

    /// Returned on successful discovery operation
    Discover(IResult<Vec<Registration>>),

    /// Returned on successful find operation (if peer is not found, returns None)
    Find(IResult<Option<Registration>>),

    /// Returned on successful group operation
    Groups(IResult<Vec<String>>)
}