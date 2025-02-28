use std::{collections::HashSet, fs::create_dir_all, path::Path};

use chrono::{DateTime, TimeDelta, Utc};
use heed::{
    types::{SerdeBincode, Str},
    Database, Env, EnvOpenOptions, RoTxn, RwTxn,
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
    pub ttl: TimeDelta
}

impl Registration {
    pub fn expiration(&self) -> DateTime<Utc> {
        self.last_registration + self.ttl
    }
}

#[derive(Clone, Debug)]
pub struct Registrations(Env);

#[allow(dead_code)]
impl Registrations {
    pub fn new(path: impl AsRef<Path>) -> Self {
        let _path = path.as_ref();
        if !_path.exists() {
            create_dir_all(_path).expect("Failed to create db folder");
        }

        let env = unsafe {
            EnvOpenOptions::new()
                .max_dbs(12)
                .open(_path)
        }
        .expect("Unable to open registration store.");
        let created = Self(env);
        let mut txn = created.rw().unwrap();
        created.expirations_read_write(&mut txn).expect("Unable to create expiration DB");
        created.registrations_read_write(&mut txn).expect("Unable to create registration DB");
        
        txn.commit().unwrap();

        created.clone()
    }

    fn rw(&self) -> IResult<RwTxn<'_>> {
        self.0.write_txn().or_else(|e| Err(InterplexError::wrap(e)))
    }

    fn ro(&self) -> IResult<RoTxn<'_>> {
        self.0.read_txn().or_else(|e| Err(InterplexError::wrap(e)))
    }

    fn registrations_read_only(
        &self,
        txn: &RoTxn<'_>
    ) -> IResult<Database<Str, SerdeBincode<Registration>>> {
        let db = self
            .0
            .open_database::<Str, SerdeBincode<Registration>>(txn, Some("registrations"))
            .or_else(|e| Err(InterplexError::wrap(e)))?
            .ok_or(InterplexError::wrap(
                "Registration database not initialized.",
            ))?;
        Ok(db)
    }

    fn registrations_read_write(
        &self,
        txn: &mut RwTxn<'_>
    ) -> IResult<Database<Str, SerdeBincode<Registration>>> {
        let db = self
            .0
            .create_database::<Str, SerdeBincode<Registration>>(txn, Some("registrations"))
            .or_else(|e| Err(InterplexError::wrap(e)))?;
        Ok(db)
    }

    fn expirations_read_only(&self, txn: &RoTxn<'_>) -> IResult<Database<Str, Str>> {
        let db = self
            .0
            .open_database::<Str, Str>(txn, Some("expirations"))
            .or_else(|e| Err(InterplexError::wrap(e)))?
            .ok_or(InterplexError::wrap("Expiration database not initialized."))?;
        Ok(db)
    }

    fn expirations_read_write(&self, txn: &mut RwTxn<'_>) -> IResult<Database<Str, Str>> {
        let db = self
            .0
            .create_database::<Str, Str>(txn, Some("expirations"))
            .or_else(|e| Err(InterplexError::wrap(e)))?;
        Ok(db)
    }

    pub fn register(
        &self,
        node: NodeIdentifier,
        addresses: Vec<Multiaddr>,
        ttl: TimeDelta
    ) -> IResult<Registration> {
        let mut rw = self.rw()?;
        let ro = self.ro()?;

        let rdb = self.registrations_read_write(&mut rw)?;
        let edb = self.expirations_read_write(&mut rw)?;

        let current_time = Utc::now();
        let (created, last_exp) = if let Some(mut reg) = rdb
            .get(&ro, &node.key())
            .or_else(|e| Err(InterplexError::wrap(e)))?
        {
            reg.addresses = addresses.clone();
            reg.identity.discoverability = node.clone().discoverability;
            reg.identity.alias = node.clone().alias;
            reg.identity.metadata = node.clone().metadata;
            let last_exp = reg.last_registration.timestamp();
            reg.last_registration = current_time;
            reg.ttl = ttl.clone();

            (reg, Some(last_exp))
        } else {
            (
                Registration {
                    identity: node.clone(),
                    addresses: addresses.clone(),
                    last_registration: current_time,
                    ttl: ttl.clone()
                },
                None,
            )
        };

        if let Some(exp) = last_exp {
            edb.delete(&mut rw, &format!("{}:{}", exp, node.clone().key()))
                .or_else(|e| Err(InterplexError::wrap(e)))?;
        }

        edb.put(
            &mut rw,
            &format!("{}:{}", current_time.timestamp(), node.clone().key()),
            &node.clone().key(),
        )
        .or_else(|e| Err(InterplexError::wrap(e)))?;
        rdb.put(&mut rw, &created.identity.key(), &created)
            .or_else(|e| Err(InterplexError::wrap(e)))?;
        rw.commit().or_else(|e| Err(InterplexError::wrap(e)))?;
        ro.commit().or_else(|e| Err(InterplexError::wrap(e)))?;
        Ok(created.clone())
    }

    pub fn deregister(&self, node: NodeIdentifier) -> IResult<()> {
        let mut rw = self.rw()?;

        let rdb = self.registrations_read_write(&mut rw)?;
        rdb.delete(&mut rw, &node.key())
            .or_else(|e| Err(InterplexError::wrap(e)))?;
        rw.commit().or_else(|e| Err(InterplexError::wrap(e)))?;
        Ok(())
    }

    pub fn poll(&self, expiration: TimeDelta) -> IResult<Option<Registration>> {
        let mut rw = self.rw()?;
        let ro = self.ro()?;

        let rdb = self.registrations_read_write(&mut rw)?;
        let edb = self.expirations_read_write(&mut rw)?;
        let expired = if let Ok(Some((key, value))) = edb.first(&ro) {
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
            let _ = edb.delete(&mut rw, &expired_key);
            if let Ok(Some(reg)) = rdb.get(&ro, &expired_value) {
                let _ = rdb.delete(&mut rw, &expired_key);
                let _ = rw.commit();
                let _ = ro.commit();
                return Ok(Some(reg));
            }
        }

        let _ = ro.commit();
        let _ = rw.commit();
        Ok(None)
    }

    pub fn discover(
        &self,
        node: NodeIdentifier,
        group: Option<impl AsRef<str>>,
    ) -> IResult<Vec<Registration>> {
        let ro = self.ro()?;
        let rdb = self.registrations_read_only(&ro)?;
        let prefix = match group {
            Some(g) => format!("{}/{}/", node.namespace.clone(), g.as_ref().to_string()),
            None => format!("{}/", node.namespace.clone()),
        };
        let mut discovered: Vec<Registration> = Vec::new();
        for result in rdb
            .prefix_iter(&ro, &prefix)
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
        let _ = ro.commit();
        Ok(discovered)
    }

    pub fn get(&self, key: impl Into<String>) -> IResult<Option<Registration>> {
        let ro = self.ro()?;
        let rdb = self.registrations_read_only(&ro)?;
        if let Ok(Some(result)) = rdb.get(&ro, &key.into()) {
            let _ = ro.commit();
            Ok(Some(result))
        } else {
            let _ = ro.commit();
            Ok(None)
        }
    }

    pub fn groups(&self, namespace: impl Into<String>) -> IResult<Vec<String>> {
        let ro = self.ro()?;
        let rdb = self.registrations_read_only(&ro)?;
        let mut groups: HashSet<String> = HashSet::new();
        for registration in rdb.prefix_iter(&ro, &format!("{}/", namespace.into())).or_else(|e| Err(InterplexError::wrap(e)))? {
            if let Ok((_, Registration {identity: NodeIdentifier {group: Some(group), ..}, ..})) = registration {
                groups.insert(group);
            }
        }
        let _ = ro.commit();
        Ok(groups.into_iter().collect())
    }
}
