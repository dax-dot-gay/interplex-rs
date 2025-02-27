use std::{cmp::Ordering, path::Path};

use chrono::{DateTime, Utc};
use heed::{
    Comparator, DefaultComparator, Env, EnvFlags, EnvOpenOptions, LexicographicComparator, RoTxn,
    RwTxn,
};
use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};

use crate::{
    error::{IResult, InterplexError},
    identification::NodeIdentifier,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Registration {
    pub identity: NodeIdentifier,
    pub addresses: Vec<Multiaddr>,
    pub last_registration: DateTime<Utc>,
}

struct RegistrationComparator;

impl Comparator for RegistrationComparator {
    fn compare(a: &[u8], b: &[u8]) -> std::cmp::Ordering {
        for (part_a, part_b) in a.splitn(3, |v| v == &b'/').zip(b.splitn(3, |v| v == &b'/')) {
            match DefaultComparator::compare(part_a, part_b) {
                Ordering::Equal => (),
                otherwise => return otherwise,
            };
        }
        Ordering::Equal
    }
}

struct ExpirationComparator;

impl Comparator for ExpirationComparator {
    fn compare(a: &[u8], b: &[u8]) -> Ordering {}
}

pub struct Registrations(Env);

impl Registrations {
    pub fn new(path: impl AsRef<Path>) -> Self {
        let env = unsafe {
            EnvOpenOptions::new()
                .flags(EnvFlags::NO_SUB_DIR)
                .open(path.as_ref())
        }
        .expect("Unable to open registration store.");
        Self(env)
    }

    fn rw(&self) -> IResult<RwTxn<'_>> {
        self.0.write_txn().or_else(|e| Err(InterplexError::wrap(e)))
    }

    fn ro(&self) -> IResult<RoTxn<'_>> {
        self.0.read_txn().or_else(|e| Err(InterplexError::wrap(e)))
    }
}
