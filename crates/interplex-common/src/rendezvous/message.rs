use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};

use crate::{error::InterplexError, identification::NodeIdentifier};

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
    Discover(Vec<Registration>),

    /// Returned on successful find operation (if peer is not found, returns None)
    Find(Option<Registration>)
}