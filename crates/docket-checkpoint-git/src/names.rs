//! The names the store gives things: refs and index files. Pure.

use docket_core::CheckpointId;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use prov::SessionId;

/// What `segment` keeps as it is: letters, digits, `_` and `-`.
const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'_').remove(b'-');

/// Where points live in a repository: outside `refs/heads` and `refs/tags`, so no branch or tag
/// listing shows them and a clone does not fetch them.
const FAMILY: &str = "refs/docket/checkpoints";

/// The session as one ref segment: letters, digits, `_` and `-` as they are, every other byte as
/// `%` and two hex digits, so no session id can make a ref name git refuses or one that collides
/// with another session's.
pub(crate) fn segment(session: &SessionId) -> String {
    utf8_percent_encode(session.as_str(), SEGMENT)
        .map(|chunk| {
            if chunk.starts_with('%') {
                chunk.to_ascii_lowercase()
            } else {
                chunk.to_owned()
            }
        })
        .collect()
}

/// The ref of point `id` of `session`: `refs/docket/checkpoints/<session>/<n>`.
pub fn ref_name(session: &SessionId, id: CheckpointId) -> String {
    format!("{FAMILY}/{}/{}", segment(session), id.0)
}

/// The prefix every ref of `session` has, ending in `/`.
pub(crate) fn session_prefix(session: &SessionId) -> String {
    format!("{FAMILY}/{}/", segment(session))
}

/// The point number a ref of `session` names, if `refname` is one.
pub(crate) fn id_of_ref(session: &SessionId, refname: &str) -> Option<CheckpointId> {
    refname
        .strip_prefix(&session_prefix(session))
        .and_then(|rest| rest.parse::<u32>().ok())
        .map(CheckpointId)
}

/// Which private index file of a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexKind {
    /// Kept between turns, so each `add` only looks at what changed.
    Taken,
    /// For reading the folder as it is now (a plan, the start of a restore).
    Planned,
    /// For writing files back.
    Restoring,
}

/// The file name of a session's private index of `kind`.
pub(crate) fn index_file(session: &SessionId, kind: IndexKind) -> String {
    let stem = match kind {
        IndexKind::Taken => "index",
        IndexKind::Planned => "plan",
        IndexKind::Restoring => "restore",
    };
    format!("{stem}-{}", segment(session))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_segment_keeps_safe_bytes_and_escapes_the_rest_in_lower_case() {
        let session = SessionId::parse("s.1_x-y").expect("session");
        assert_eq!(segment(&session), "s%2e1_x-y");
    }
}
