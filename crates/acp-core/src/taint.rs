//! Which thing put an agent session under the rule "after any file is served, a command asks and
//! offers no always". The router holds the taint itself (its labels decide); this is the host's
//! typed record of the first cause, so a later narrowing (taint by path, or only content the agent
//! then uses in a command) is a change to what counts here and to the label the performer gives,
//! not a hunt through string compares.

use docket_core::{AbsPath, PermissionKind};
use prov::Source;

/// What caused it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaintSource {
    /// A file served to the agent through `fs/read_text_file`.
    Served(AbsPath),
    /// A tool call the agent reported having run itself that brings content in (a read, a search,
    /// a fetch, a command).
    Reported(PermissionKind),
    /// The session was resumed tainted: the log says it was, not which read did it.
    Resumed,
}

/// Whether a tool call the agent reports brings untrusted content into the session.
pub fn brings_content(kind: PermissionKind) -> bool {
    matches!(
        kind,
        PermissionKind::Read
            | PermissionKind::Search
            | PermissionKind::Fetch
            | PermissionKind::Execute
            | PermissionKind::Other
    )
}

/// Where what a tainting kind brings in comes from: the network for a fetch, a file for the rest.
/// `None` for a kind that brings nothing in. The one rule behind both the host's record
/// ([`brings_content`]) and the label the performer gives a reported call.
pub fn source(kind: PermissionKind) -> Option<Source> {
    match (brings_content(kind), kind) {
        (false, _) => None,
        (true, PermissionKind::Fetch) => Some(Source::Web),
        (true, _) => Some(Source::File),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_exists_exactly_for_the_kinds_that_taint() {
        for kind in PermissionKind::ALL {
            assert_eq!(source(kind).is_some(), brings_content(kind), "{kind:?}");
        }
        assert_eq!(source(PermissionKind::Fetch), Some(Source::Web));
        assert_eq!(source(PermissionKind::Read), Some(Source::File));
    }

    #[test]
    fn only_the_tools_that_bring_content_in_taint() {
        let tainting: Vec<PermissionKind> = PermissionKind::ALL
            .into_iter()
            .filter(|k| brings_content(*k))
            .collect();
        assert_eq!(
            tainting,
            [
                PermissionKind::Read,
                PermissionKind::Search,
                PermissionKind::Execute,
                PermissionKind::Fetch,
                PermissionKind::Other
            ]
        );
    }
}
