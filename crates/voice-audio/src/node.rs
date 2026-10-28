//! The audio nodes a device offers.

use serde::{Deserialize, Serialize};

/// A PipeWire node id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeId(pub u32);

/// What a node is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// A physical input (a microphone).
    Source,
    /// The monitor of an output: other apps' audio. Never captured.
    Monitor,
    /// An output.
    Sink,
}

/// The node's `media.class` property (`Audio/Source`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MediaClass(pub String);

/// One audio node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioNode {
    /// Its id.
    pub id: NodeId,
    /// Its name.
    pub name: String,
    /// What it is.
    pub kind: NodeKind,
    /// Its `media.class`.
    pub media_class: MediaClass,
}
