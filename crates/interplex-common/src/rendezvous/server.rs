use std::{
    ops::Deref,
    path::Path,
    task::{Context, Poll},
};

use crate::{
    error::{IResult, InterplexError},
    identification::NodeIdentifier,
};
use chrono::{DateTime, TimeDelta, Utc};
use derive_builder::Builder;
use heed::{
    types::{SerdeBincode, Str},
    Database, Env, EnvFlags, EnvOpenOptions, RoTxn, RwTxn,
};
use libp2p::{
    futures::ready, request_response::{self, ProtocolSupport}, swarm::{NetworkBehaviour, THandlerInEvent, ToSwarm}, Multiaddr, StreamProtocol
};
use serde::{Deserialize, Serialize};

use super::{message::{RendezvousRequest, RendezvousResponse}, registrations::Registration};

#[derive(Builder, Clone, Debug)]
#[builder(setter(into, strip_option))]
pub struct Config {
    environment: Env,

    #[builder(default = "chrono::TimeDelta::hours(12)")]
    max_lifetime: TimeDelta,

    #[builder(default = "Some(128)")]
    max_addresses: Option<u64>,

    #[builder(default)]
    allowed_namespaces: Option<Vec<String>>,

    #[builder(default = "chrono::TimeDelta::minutes(5)")]
    clean_interval: TimeDelta,
}

impl ConfigBuilder {
    pub fn database(&mut self, path: impl AsRef<Path>) -> &mut Self {
        let env = unsafe {
            EnvOpenOptions::new()
                .flags(EnvFlags::NO_SUB_DIR)
                .open(path.as_ref())
        }
        .expect("Expected to be able to open the environment.");
        self.environment(env)
    }
}

pub struct Behavior {
    inner: libp2p::request_response::cbor::Behaviour<RendezvousRequest, RendezvousResponse>,
    config: Config,
    last_clean: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub enum Event {
    CreatedRegistration(Registration),
    UpdatedRegistration(Registration),
    RemovedRegistration(Registration),
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
        result: NodeIdentifier,
    },
    FailedFind {
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
        todo!()
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
            last_clean: Utc::now(),
        }
    }

    fn ro(&self) -> IResult<(Database<Str, SerdeBincode<Registration>>, RoTxn<'_>)> {
        let txn = self
            .config
            .environment
            .read_txn()
            .or_else(|e| Err(InterplexError::wrap(e)))?;
        let database = self
            .config
            .environment
            .open_database::<Str, SerdeBincode<Registration>>(&txn, Some("registrations"))
            .or_else(|e| Err(InterplexError::wrap(e)))?
            .ok_or(InterplexError::wrap(
                "Cannot open non-existent database as RO.",
            ))?;
        Ok((database, txn))
    }

    fn rw(&self) -> IResult<(Database<Str, SerdeBincode<Registration>>, RwTxn<'_>)> {
        let mut txn = self
            .config
            .environment
            .write_txn()
            .or_else(|e| Err(InterplexError::wrap(e)))?;
        let database = self
            .config
            .environment
            .create_database::<Str, SerdeBincode<Registration>>(&mut txn, Some("registrations"))
            .or_else(|e| Err(InterplexError::wrap(e)))?;
        Ok((database, txn))
    }

    fn clean(&self) -> IResult<Vec<Registration>> {
        let (db, txn) = self.ro()?;
        db.get_greater_than(txn, key)
        let mut to_clean: Vec<Registration> = Vec::new();
        for item in db.iter(&txn).or_else(|e| Err(InterplexError::wrap(e)))? {
            if let Ok((_, registration)) = item {
                to_clean.push(registration);
            }
        }

        let (db, mut txn) = self.rw()?;
        for r in to_clean.clone() {
            db.delete(&mut txn, r.identity.key().as_str())
                .or_else(|e| Err(InterplexError::wrap(e)))?;
        }

        Ok(to_clean)
    }
}
