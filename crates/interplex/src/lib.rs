mod network;
mod node;
mod ipc;
mod error;
mod netwrapper;

pub use node::{InterplexNode, SavedKey, NodeBuilder};
pub use error::Error;

// notes for future me
// request/response with streams! Allows arbitrary-size thingies
// base64 code blocks, use ASCII control codes to split blocks & mark start and end
// i still don't know if we wanna do single request/response per stream or just open a stream and let the client close it when it's done