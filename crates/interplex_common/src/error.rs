use std::fmt::Debug;

use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum InterplexError {
    #[error("Failed to serialize data: {0}")]
    Serialization(String),

    #[error("Failed to deserialize data: {0}")]
    Deserialization(String),

    #[error("Record not found with key: {0}")]
    NotFound(String),

    #[error("An unknown error occurred: {0}")]
    Unknown(String),

    #[error("{0}")]
    Wrapped(String),

    #[error("This node is not externally accessible (no external addresses)")]
    NodeInaccessible,

    #[error("Failed to dispatch request to {peer}: {namespace}({command})")]
    RequestDispatch {peer: PeerId, namespace: String, command: String},

    #[error("Improperly specified address: {addr}: {reason}")]
    Address {addr: String, reason: String}
}

impl InterplexError {
    pub fn serialization(err: impl Debug) -> Self {
        Self::Serialization(format!("{err:?}"))
    }

    pub fn deserialization(err: impl Debug) -> Self {
        Self::Deserialization(format!("{err:?}"))
    }

    pub fn unknown(err: impl Debug) -> Self {
        Self::Unknown(format!("{err:?}"))
    }

    pub fn not_found(key: impl Into<String>) -> Self {
        Self::NotFound(key.into())
    }

    pub fn wrap(err: impl Debug) -> Self {
        Self::Wrapped(format!("{err:?}"))
    }
}

pub type IResult<T> = Result<T, InterplexError>;
