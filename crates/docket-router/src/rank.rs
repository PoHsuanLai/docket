//! How well an entry answers a search: the better the match, the earlier it is listed.

use docket_core::IndexEntry;

/// How an entry matches the words, best first (the order of the variants is the order of the
/// hits).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Rank {
    /// The title starts with the words.
    TitleStarts,
    /// The title contains them.
    TitleContains,
    /// A keyword contains them.
    Keyword,
    /// The subtitle contains them.
    Subtitle,
}

/// How `entry` matches `text` (case does not matter), or `None` when it does not.
pub(crate) fn rank_of(entry: &IndexEntry, text: &str) -> Option<Rank> {
    let needle = text.to_lowercase();
    let title = entry.title.to_lowercase();
    if title.starts_with(&needle) {
        Some(Rank::TitleStarts)
    } else if title.contains(&needle) {
        Some(Rank::TitleContains)
    } else if entry
        .keywords
        .iter()
        .any(|k| k.to_lowercase().contains(&needle))
    {
        Some(Rank::Keyword)
    } else if entry.subtitle.to_lowercase().contains(&needle) {
        Some(Rank::Subtitle)
    } else {
        None
    }
}
