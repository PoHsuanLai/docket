//! What an MCP client's arguments are worth.

use prov::{ClientName, Confidentiality, Integrity, Label, Source};
use std::collections::BTreeSet;

/// The label of every argument from `client`: `Untrusted`, from `Source::Mcp(client)`, no data
/// class of its own and not private to any Space (the client typed it; it holds nothing of the
/// person's yet).
pub fn mcp_label(client: &ClientName) -> Label {
    Label {
        integrity: Integrity::Untrusted,
        confidentiality: Confidentiality::Public,
        classes: BTreeSet::new(),
        sources: BTreeSet::from([Source::Mcp(client.clone())]),
    }
}
