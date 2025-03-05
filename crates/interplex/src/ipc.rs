use std::collections::HashMap;

use async_channel::Sender;
use interplex_common::identification::NodeIdentifier;
use libp2p::{bytes::Bytes, Multiaddr, PeerId};
use uuid::Uuid;

use crate::error::Error;

#[derive(Clone, Debug)]
pub(crate) enum StreamRole {
    Source,
    Sink,
}

#[derive(Clone, Debug)]
pub(crate) enum Command {
    OpenStream(PeerId),
    CloseStream(Uuid),
    WriteStream { stream_id: Uuid, data: Vec<u8> },
    ReadStream { stream_id: Uuid, buf_size: usize },
    Subscribe(Vec<String>),
    Unsubscribe(Vec<String>),
    ExitLoop,
    AddRendezvous(Multiaddr),
    RemoveRendezvous(PeerId),
    UpdateRemotes(Option<String>)
}

#[derive(Clone, Debug)]
pub(crate) enum CommandResponse {
    OpenStream(Uuid),
    CloseStream,
    WriteStream(usize),
    ReadStream { data: Vec<u8>, bytes_read: usize },
    Subscribe,
    Unsubscribe,
    ExitLoop,
    AddRendezvous(PeerId),
    RemoveRendezvous,
    UpdateRemotes
}

#[derive(Clone)]
pub(crate) struct CommandWrapper {
    pub response_channel: Sender<Result<CommandResponse, Error>>,
    pub command: Command,
}

#[derive(Clone, Debug)]
pub(crate) enum Event {
    StreamOpened {
        stream_id: Uuid,
        remote: PeerId,
        role: StreamRole,
    },
    StreamClosed {
        stream_id: Uuid,
        remote: PeerId,
        role: StreamRole,
    },
    SubscribedMessage {
        source: PeerId,
        data: Bytes,
        topics: Vec<String>,
    },
    DiscoveredPeers(HashMap<PeerId, NodeIdentifier>),
    LostPeer(NodeIdentifier)
}
