//! Characters that make words say something other than what they show. A model's words reach
//! the person (a recipient on a sheet, a reason in the activity view, an answer), and a
//! hostile one may write a bidirectional override so the text reads reversed or a zero-width
//! mark so two different strings look equal. These two predicates are the one definition the
//! readers of model-written text share.

/// A mark that reorders the text around it: the bidirectional embeddings, overrides and
/// isolates (U+202A to U+202E, U+2066 to U+2069). Nothing a model writes needs one.
pub fn reorders(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// A mark that draws nothing: zero-width space, word joiner and the invisible operators,
/// the byte order mark and the soft hyphen. (The joiners, U+200C and U+200D, and the
/// directional marks are left out: scripts and emoji sequences need them.)
pub fn hides(c: char) -> bool {
    matches!(
        c,
        '\u{200B}' | '\u{2060}'..='\u{2064}' | '\u{FEFF}' | '\u{00AD}'
    )
}

/// Whether `text` holds nothing that reorders or hides, and no control character but the
/// ones in `allowed`.
pub fn plain_text(text: &str, allowed: &[char]) -> bool {
    text.chars()
        .all(|c| !reorders(c) && !hides(c) && (!c.is_control() || allowed.contains(&c)))
}
