use std::path::Path;

use chrono::{DateTime, TimeDelta, Utc};
use heed::{
    types::{SerdeBincode, Str},
    Database, Env, EnvFlags, EnvOpenOptions, RoTxn, RwTxn,
};
use libp2p::Multiaddr;
use serde::{Deserialize, Serialize};

use crate::{
    error::{IResult, InterplexError},
    identification::{Discoverability, NodeIdentifier},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Registration {
    pub identity: NodeIdentifier,
    pub addresses: Vec<Multiaddr>,
    pub last_registration: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct Registrations(Env);

impl Registrations {
    pub fn new(path: impl AsRef<Path>) -> Self {
        let env = unsafe {
            EnvOpenOptions::new()
                .flags(EnvFlags::NO_SUB_DIR)
                .open(path.as_ref())
        }
        .expect("Unable to open registration store.");
        let created = Self(env);
        created
            .expirations_read_write()
            .expect("Failed to initialize expiration database");
        created
            .registrations_read_write()
            .expect("Failed to initialize registration database");
        created
    }

    fn rw(&self) -> IResult<RwTxn<'_>> {
        self.0.write_txn().or_else(|e| Err(InterplexError::wrap(e)))
    }

    fn ro(&self) -> IResult<RoTxn<'_>> {
        self.0.read_txn().or_else(|e| Err(InterplexError::wrap(e)))
    }

    fn registrations_read_only(
        &self,
    ) -> IResult<(Database<Str, SerdeBincode<Registration>>, RoTxn<'_>)> {
        let txn = self.ro()?;
        let db = self
            .0
            .open_database::<Str, SerdeBincode<Registration>>(&txn, Some("registrations"))
            .or_else(|e| Err(InterplexError::wrap(e)))?
            .ok_or(InterplexError::wrap(
                "Registration database not initialized.",
            ))?;
        Ok((db, txn))
    }

    fn registrations_read_write(
        &self,
    ) -> IResult<(
        Database<Str, SerdeBincode<Registration>>,
        RoTxn<'_>,
        RwTxn<'_>,
    )> {
        let rtxn = self.ro()?;
        let mut wtxn = self.rw()?;
        let db = self
            .0
            .create_database::<Str, SerdeBincode<Registration>>(&mut wtxn, Some("registrations"))
            .or_else(|e| Err(InterplexError::wrap(e)))?;
        Ok((db, rtxn, wtxn))
    }

    fn expirations_read_only(&self) -> IResult<(Database<Str, Str>, RoTxn<'_>)> {
        let txn = self.ro()?;
        let db = self
            .0
            .open_database::<Str, Str>(&txn, Some("expirations"))
            .or_else(|e| Err(InterplexError::wrap(e)))?
            .ok_or(InterplexError::wrap("Expiration database not initialized."))?;
        Ok((db, txn))
    }

    fn expirations_read_write(&self) -> IResult<(Database<Str, Str>, RoTxn<'_>, RwTxn<'_>)> {
        let rtxn = self.ro()?;
        let mut wtxn = self.rw()?;
        let db = self
            .0
            .create_database::<Str, Str>(&mut wtxn, Some("expirations"))
            .or_else(|e| Err(InterplexError::wrap(e)))?;
        Ok((db, rtxn, wtxn))
    }

    pub fn register(
        &self,
        node: NodeIdentifier,
        addresses: Vec<Multiaddr>,
    ) -> IResult<Registration> {
        let (rdb, rro, mut rrw) = self.registrations_read_write()?;
        let (edb, _, mut erw) = self.expirations_read_write()?;
        let current_time = Utc::now();
        let (created, last_exp) = if let Some(mut reg) = rdb
            .get(&rro, &node.key())
            .or_else(|e| Err(InterplexError::wrap(e)))?
        {
            reg.addresses = addresses.clone();
            reg.identity.discoverability = node.clone().discoverability;
            let last_exp = reg.last_registration.timestamp();
            reg.last_registration = current_time;

            (reg, Some(last_exp))
        } else {
            (
                Registration {
                    identity: node.clone(),
                    addresses: addresses.clone(),
                    last_registration: current_time,
                },
                None,
            )
        };

        if let Some(exp) = last_exp {
            edb.delete(&mut erw, &format!("{}:{}", exp, node.clone().key()))
                .or_else(|e| Err(InterplexError::wrap(e)))?;
        }

        edb.put(
            &mut erw,
            &format!("{}:{}", current_time.timestamp(), node.clone().key()),
            &node.clone().key(),
        )
        .or_else(|e| Err(InterplexError::wrap(e)))?;
        rdb.put(&mut rrw, &created.identity.key(), &created)
            .or_else(|e| Err(InterplexError::wrap(e)))?;
        rrw.commit().or_else(|e| Err(InterplexError::wrap(e)))?;
        erw.commit().or_else(|e| Err(InterplexError::wrap(e)))?;
        Ok(created.clone())
    }

    pub fn deregister(&self, node: NodeIdentifier) -> IResult<()> {
        let (rdb, _, mut rrw) = self.registrations_read_write()?;
        rdb.delete(&mut rrw, &node.key())
            .or_else(|e| Err(InterplexError::wrap(e)))?;
        rrw.commit().or_else(|e| Err(InterplexError::wrap(e)))?;
        Ok(())
    }

    pub fn poll(&self, expiration: TimeDelta) -> IResult<Option<Registration>> {
        let (rdb, rro) = self.registrations_read_only()?;
        let (edb, ero) = self.expirations_read_only()?;
        let expired = if let Ok(Some((key, value))) = edb.first(&ero) {
            if let Some((last_reg, _)) = key.split_once(":") {
                if let Ok(ts) = last_reg.parse::<i64>() {
                    if DateTime::from_timestamp(ts, 0).unwrap() + expiration < Utc::now() {
                        Some((key.to_string(), value.to_string()))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        if let Some((expired_key, expired_value)) = expired {
            let (edb, _, mut erw) = self.expirations_read_write()?;
            let _ = edb.delete(&mut erw, &expired_key);
            let _ = erw.commit();
            if let Ok(Some(reg)) = rdb.get(&rro, &expired_value) {
                let (rdb, _, mut rrw) = self.registrations_read_write()?;
                let _ = rdb.delete(&mut rrw, &expired_key);
                let _ = rrw.commit();
                return Ok(Some(reg));
            }
        }

        Ok(None)
    }

    pub fn discover(
        &self,
        node: NodeIdentifier,
        group: Option<impl AsRef<str>>,
    ) -> IResult<Vec<Registration>> {
        let (rdb, rro) = self.registrations_read_only()?;
        let prefix = match group {
            Some(g) => format!("{}/{}/", node.namespace.clone(), g.as_ref().to_string()),
            None => format!("{}/", node.namespace.clone()),
        };
        let mut discovered: Vec<Registration> = Vec::new();
        for result in rdb
            .prefix_iter(&rro, &prefix)
            .or_else(|e| Err(InterplexError::wrap(e)))?
        {
            if let Ok((key, registration)) = result {
                if node.key() != key.to_string() {
                    match registration.identity.discoverability {
                        Discoverability::Namespace => {
                            discovered.push(registration.clone());
                        }
                        Discoverability::Group => {
                            if registration.identity.group() == node.group() {
                                discovered.push(registration.clone());
                            }
                        },
                        _ => ()
                    }
                }
            }
        }

        Ok(discovered)
    }

    pub fn get(&self, key: impl Into<String>) -> IResult<Option<Registration>> {
        let (rdb, rro) = self.registrations_read_only()?;
        if let Ok(Some(result)) = rdb.get(&rro, &key.into()) {
            Ok(Some(result))
        } else {
            Ok(None)
        }
    }
}
