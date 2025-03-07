use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_channel::{Receiver, Sender};
use interplex_common::identification::{Discoverability, NodeIdentifier};
use libp2p::{
    identity::{Keypair, PublicKey},
    multiaddr::Protocol,
    Multiaddr, PeerId,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_cbor::{value::to_value, Value};

use crate::{
    error::{CResult, Error},
    ipc::{CommandWrapper, Event}
};

#[derive(Clone)]
pub struct InterplexNode {
    commands: (Sender<CommandWrapper>, Receiver<CommandWrapper>),
    events: (Sender<Event>, Receiver<Event>),
    identifier: Arc<Mutex<NodeIdentifier>>,
    running_network: Arc<Mutex<Network>>,
    event_hooks: Arc<Mutex<HashMap<String, fn(Event) -> ()>>>,
    keypair: Keypair,
    rendezvous_nodes: Arc<Mutex<HashMap<PeerId, Multiaddr>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedKey(Vec<u8>);

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct NodeBuilder {
    namespace: Option<String>,
    alias: Option<String>,
    group: Option<String>,
    metadata: HashMap<String, Value>,
    keypair: Option<SavedKey>,
    discoverability: Discoverability,
    rendezvous_nodes: HashMap<PeerId, Multiaddr>,
}

impl NodeBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn namespace(&mut self, namespace: impl Into<String>) -> &mut Self {
        self.namespace = Some(namespace.into());
        self
    }

    pub fn alias(&mut self, alias: impl Into<String>) -> &mut Self {
        self.alias = Some(alias.into());
        self
    }

    pub fn group(&mut self, group: impl Into<String>) -> &mut Self {
        self.group = Some(group.into());
        self
    }

    pub fn with_keypair(&mut self, keypair: libp2p::identity::ed25519::Keypair) -> &mut Self {
        self.keypair = Some(SavedKey::from(keypair));
        self
    }

    pub fn with_meta(
        &mut self,
        key: impl Into<String>,
        value: impl Serialize + DeserializeOwned,
    ) -> CResult<&mut Self> {
        let serialized = to_value(value).or_else(|e| Err(Error::encoding(e)))?;
        self.metadata.insert(key.into(), serialized);
        Ok(self)
    }

    pub fn discoverability(&mut self, mode: Discoverability) -> &mut Self {
        self.discoverability = mode;
        self
    }

    pub fn rendezvous(&mut self, address: Multiaddr) -> CResult<&mut Self> {
        if let Some(peer) = address.iter().find_map(|p| {
            if let Protocol::P2p(peer_id) = p {
                Some(peer_id)
            } else {
                None
            }
        }) {
            self.rendezvous_nodes.insert(peer, address);
            Ok(self)
        } else {
            Err(Error::incorrect_address(
                address,
                "Address must contain a P2P protocol block (ie .../p2p/<peer_id>",
            ))
        }
    }

    pub fn build(self) -> CResult<InterplexNode> {
        if self.namespace.is_none() {
            return Err(Error::build_node("Namespace must be specified"));
        }

        let key = if let Some(saved) = self.keypair {
            saved.keypair()
        } else {
            SavedKey::new().keypair()
        };

        Ok(InterplexNode::new(
            NodeIdentifier {
                peer_id: key.public().to_peer_id(),
                namespace: self.namespace.unwrap(),
                alias: self.alias,
                group: self.group,
                metadata: self.metadata,
                discoverability: self.discoverability,
            },
            key,
            self.rendezvous_nodes,
        ))
    }
}

impl SavedKey {
    pub fn new() -> Self {
        Self(
            Keypair::generate_ed25519()
                .try_into_ed25519()
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
    }

    pub fn keypair(&self) -> Keypair {
        let mut encoded = self.0.clone();
        Keypair::ed25519_from_bytes(&mut encoded).unwrap()
    }

    pub fn public(&self) -> PublicKey {
        self.keypair().public()
    }

    pub fn peer_id(&self) -> PeerId {
        self.public().to_peer_id()
    }
}

impl Into<Keypair> for SavedKey {
    fn into(self) -> Keypair {
        self.keypair()
    }
}

impl From<Keypair> for SavedKey {
    fn from(value: Keypair) -> Self {
        Self(value.try_into_ed25519().unwrap().to_bytes().to_vec())
    }
}

impl From<libp2p::identity::ed25519::Keypair> for SavedKey {
    fn from(value: libp2p::identity::ed25519::Keypair) -> Self {
        Self(value.to_bytes().to_vec())
    }
}

impl InterplexNode {
    pub fn new(
        identifier: NodeIdentifier,
        keypair: Keypair,
        rendezvous_nodes: HashMap<PeerId, Multiaddr>,
    ) -> Self {
        Self {
            commands: async_channel::unbounded::<CommandWrapper>(),
            events: async_channel::unbounded::<Event>(),
            identifier: Arc::new(Mutex::new(identifier)),
            running_network: Network::Stopped(Arc::new),
            event_hooks: Arc::new(Mutex::new(HashMap::new())),
            keypair,
            rendezvous_nodes: Arc::new(Mutex::new(rendezvous_nodes)),
        }
    }

    pub fn activate_network(&mut self) {
        if let Ok(node_id) = self.identifier.lock() {}
    }
}
