use std::fmt::Debug;

use interplex_common::error::InterplexError;
use libp2p::PeerId;
use libp2p_stream::OpenStreamError;
use thiserror::Error as ErrorDe;
use uuid::Uuid;

#[derive(Clone, Debug, ErrorDe)]
pub enum Error {
    #[error("Encountered an internal networking error: {0:?}")]
    Internal(InterplexError),

    #[error("Failed to open stream to remote peer with ID {id}: {reason}")]
    OpenStream { id: PeerId, reason: String },

    #[error("Unknown stream ID: {id}")]
    UnknownStream { id: Uuid },

    #[error("Failed to close stream {id} to peer {peer}: {reason}")]
    CloseStream {
        id: Uuid,
        peer: PeerId,
        reason: String,
    },

    #[error("Failed to read {buffer_size} bytes from stream {id} and peer {peer}: {reason}")]
    ReadStream {
        id: Uuid,
        peer: PeerId,
        buffer_size: usize,
        reason: String,
    },

    #[error("Failed to write data to stream {id} and peer {peer}: {reason}")]
    WriteStream {
        id: Uuid,
        peer: PeerId,
        reason: String,
    },

    #[error("A connection error occurred while {context}: {reason}")]
    ConnectionError { context: String, reason: String },
}

impl Error {
    pub fn open_stream(peer_id: impl Into<PeerId>, error: OpenStreamError) -> Self {
        Error::OpenStream {
            id: peer_id.into(),
            reason: error.to_string(),
        }
    }

    pub fn close_stream(stream_id: Uuid, peer_id: impl Into<PeerId>, error: impl Debug) -> Self {
        Error::CloseStream {
            id: stream_id,
            peer: peer_id.into(),
            reason: format!("{error:?}"),
        }
    }

    pub fn unknown_stream(id: Uuid) -> Self {
        Error::UnknownStream { id }
    }

    pub fn read_stream(
        stream_id: Uuid,
        peer_id: impl Into<PeerId>,
        bufsize: usize,
        error: impl Debug,
    ) -> Self {
        Error::ReadStream {
            id: stream_id,
            peer: peer_id.into(),
            buffer_size: bufsize,
            reason: format!("{error:?}"),
        }
    }

    pub fn write_stream(stream_id: Uuid, peer_id: impl Into<PeerId>, error: impl Debug) -> Self {
        Error::WriteStream {
            id: stream_id,
            peer: peer_id.into(),
            reason: format!("{error:?}"),
        }
    }

    pub fn connection(context: impl Into<String>, error: impl Debug) -> Self {
        Error::ConnectionError {
            context: context.into(),
            reason: format!("{error:?}"),
        }
    }
}

impl From<InterplexError> for Error {
    fn from(value: InterplexError) -> Self {
        Error::Internal(value)
    }
}

pub type CResult<T> = Result<T, Error>;
