//! The mail app's search.

/// A mail app's search, roughly: every word of the query, without case or a plural `s`, is in
/// the text. A live model words its query its own way ("Lisbon receipts" for "Lisbon hotel
/// receipt"), so an exact substring would answer it with nothing.
pub(crate) fn words_match(query: &str, text: &str) -> bool {
    let text = text.to_lowercase();
    let mut words = query.split_whitespace().peekable();
    words.peek().is_some()
        && words.all(|word| {
            let word = word.to_lowercase();
            let stem = word
                .strip_suffix('s')
                .filter(|s| s.len() > 2)
                .unwrap_or(&word);
            text.contains(stem)
        })
}

#[cfg(test)]
mod search_tests {
    use super::words_match;

    #[test]
    fn a_query_matches_by_words_not_by_the_whole_string() {
        assert!(words_match("Lisbon receipts", "Lisbon hotel receipt"));
        assert!(words_match("lisbon", "Lisbon train receipt"));
        assert!(words_match("Accounting", "Accounting"));
        assert!(!words_match("Porto receipts", "Lisbon hotel receipt"));
        assert!(!words_match("", "Lisbon hotel receipt"));
        assert!(!words_match("Lisbon", "Accounting"));
    }
}
