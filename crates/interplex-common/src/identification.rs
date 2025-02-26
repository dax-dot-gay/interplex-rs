use std::collections::HashMap;

use derive_builder::Builder;
use libp2p::PeerId;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_cbor::Value;

use crate::error::{IResult, InterplexError};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Discoverability {
    Namespace,
    Group,
    Direct,
}

impl Default for Discoverability {
    fn default() -> Self {
        Self::Direct
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Builder)]
#[builder(setter(into, strip_option), name = "NodeBuilder")]
pub struct NodeIdentifier {
    pub peer_id: PeerId,
    pub namespace: String,

    #[serde(default)]
    #[builder(default)]
    pub alias: Option<String>,

    #[serde(default)]
    #[builder(default)]
    pub group: Option<String>,

    #[serde(default)]
    #[builder(default)]
    pub metadata: HashMap<String, Value>,

    #[serde(default)]
    #[builder(default)]
    pub discoverability: Discoverability,
}

impl NodeIdentifier {
    pub fn name(&self) -> String {
        self.alias.clone().unwrap_or(self.peer_id.to_string())
    }

    pub fn meta<T: Serialize + DeserializeOwned>(&self, key: impl Into<String>) -> IResult<T> {
        let norm: String = key.into();
        if let Some(record) = self.metadata.get(&norm) {
            match serde_cbor::value::from_value::<T>(record.clone()) {
                Ok(v) => Ok(v),
                Err(e) => Err(InterplexError::deserialization(e)),
            }
        } else {
            Err(InterplexError::not_found(norm))
        }
    }

    pub fn group(&self) -> String {
        self.group.clone().unwrap_or(String::from("default"))
    }

    pub fn key(&self) -> String {
        format!(
            "{}/{}::{}",
            self.namespace.clone(),
            self.group(),
            self.peer_id.to_string()
        )
    }
}

impl NodeBuilder {
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            peer_id: Some(PeerId::random()),
            namespace: Some(namespace.into()),
            alias: None,
            group: None,
            metadata: Some(HashMap::new()),
            discoverability: Some(Discoverability::default()),
        }
    }

    pub fn new_from_id(namespace: impl Into<String>, peer_id: PeerId) -> Self {
        Self {
            peer_id: Some(peer_id),
            namespace: Some(namespace.into()),
            alias: None,
            group: None,
            metadata: Some(HashMap::new()),
            discoverability: Some(Discoverability::default()),
        }
    }

    pub fn with_meta(
        &mut self,
        key: impl Into<String>,
        value: impl Serialize + DeserializeOwned,
    ) -> Result<&mut Self, NodeBuilderError> {
        self.metadata.get_or_insert_default().insert(
            key.into(),
            serde_cbor::value::to_value(value).or(Err(NodeBuilderError::ValidationError(
                String::from("Unable to serialize metadata value."),
            )))?,
        );
        Ok(self)
    }
}
