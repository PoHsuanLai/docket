//! Whether a model's text is the person's own words: it must stand in one of their turns as
//! whole tokens. A truncation of an address they typed (`bob@example.co` out of
//! `bob@example.com`, `corp.com` out of `alice@corp.com`) is the model's writing, not theirs; a
//! phrase that starts and ends on a word edge (`the meeting at 3`) still is theirs.

/// Characters that glue a match to the token around it: letters and digits, and the marks an
/// address, a path or a link is made of. A full stop glues only when more token follows it.
fn glued(c: char) -> bool {
    c.is_alphanumeric()
        || matches!(
            c,
            '@' | '_' | '-' | '+' | '/' | '\\' | '%' | '~' | '=' | '#' | '&'
        )
}

/// Whether the text before a match leaves its start on a token edge.
fn open_before(head: &str) -> bool {
    let mut back = head.chars().rev();
    match back.next() {
        None => true,
        Some(c) if glued(c) => false,
        Some('.') => back.next().is_none_or(char::is_whitespace),
        Some(_) => true,
    }
}

/// Whether the text after a match leaves its end on a token edge.
fn open_after(tail: &str) -> bool {
    let mut next = tail.chars();
    match next.next() {
        None => true,
        Some(c) if glued(c) => false,
        Some('.') => !next.next().is_some_and(glued),
        Some(_) => true,
    }
}

/// Whether `text` stands in `turn` from one token edge to another.
pub(crate) fn whole_in(turn: &str, text: &str) -> bool {
    !text.is_empty()
        && turn
            .match_indices(text)
            .any(|(at, hit)| open_before(&turn[..at]) && open_after(&turn[at + hit.len()..]))
}

#[cfg(test)]
mod tests {
    use super::whole_in;

    #[test]
    fn only_whole_tokens_of_a_turn_are_the_persons_words() {
        let turn = "mail alice@corp.com and bob@example.com. The meeting at 3 pm, ok?";
        let rows = [
            ("alice@corp.com", true),
            ("bob@example.com", true),
            ("the meeting at 3", false),
            ("The meeting at 3", true),
            ("The meeting at 3 pm", true),
            ("bob@example.co", false),
            ("corp.com", false),
            ("example.com", false),
            ("alice", false),
            ("meeting", true),
            ("meet", false),
            ("", false),
        ];
        for (text, spoken) in rows {
            assert_eq!(whole_in(turn, text), spoken, "{text:?}");
        }
    }
}
