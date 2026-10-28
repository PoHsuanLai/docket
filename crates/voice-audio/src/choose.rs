//! Which node to capture.

use crate::node::{AudioNode, NodeKind};

/// Why a node was chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChosenBy {
    /// `input = "<node.name>"` in voiced.toml named it.
    Override,
    /// It is PipeWire's default source.
    Default,
    /// Nothing better: the first physical source.
    First,
}

impl ChosenBy {
    /// The word the by-hand script prints.
    pub fn word(self) -> &'static str {
        match self {
            Self::Override => "override",
            Self::Default => "default",
            Self::First => "first",
        }
    }
}

/// The node to capture and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chosen<'a> {
    /// The node.
    pub node: &'a AudioNode,
    /// Why.
    pub by: ChosenBy,
}

fn physical(node: &AudioNode) -> bool {
    node.kind == NodeKind::Source && node.media_class.0 == "Audio/Source"
}

/// The source to capture: the configured `input` if that physical node exists, else PipeWire's
/// default source if it is a physical node, else the first physical source. A monitor or a sink is
/// never returned, whatever the override or the metadata names.
pub fn choose_capture<'a>(
    nodes: &'a [AudioNode],
    default: Option<&str>,
    input: Option<&str>,
) -> Option<Chosen<'a>> {
    let named = |name: Option<&str>, by| {
        let name = name?;
        nodes
            .iter()
            .find(|n| physical(n) && n.name == name)
            .map(|node| Chosen { node, by })
    };
    named(input, ChosenBy::Override)
        .or_else(|| named(default, ChosenBy::Default))
        .or_else(|| {
            nodes.iter().find(|n| physical(n)).map(|node| Chosen {
                node,
                by: ChosenBy::First,
            })
        })
}
