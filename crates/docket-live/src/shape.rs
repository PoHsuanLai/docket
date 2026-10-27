//! Shape checks on what the companion said, whichever flow it answers: short, the right words in
//! it, none of the words that give away the machinery, and no identifier a developer would
//! recognise (an action id, a tool name). Pure, so a flow only says which shape it wants.

/// What an answer must look like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    /// The most characters the answer may have.
    pub max_chars: usize,
    /// Words the answer must hold (any case).
    pub contains: &'static [&'static str],
    /// Words or phrases the answer must not hold (any case, whole words).
    pub lacks: &'static [&'static str],
}

impl Shape {
    /// What does not hold of `said`, one sentence each; empty is a pass. An answer never shows
    /// a developer word, whatever else the shape says.
    pub fn check(&self, said: &str) -> Vec<String> {
        let mut out = Vec::new();
        let chars = said.chars().count();
        if said.trim().is_empty() {
            out.push("the answer is empty".to_owned());
        }
        if chars > self.max_chars {
            out.push(format!(
                "the answer is {chars} characters long, over {}",
                self.max_chars
            ));
        }
        let lower = said.to_lowercase();
        for want in self.contains {
            if !lower.contains(&want.to_lowercase()) {
                out.push(format!("the answer does not say {want:?}"));
            }
        }
        let words = words_of(&lower);
        for banned in self.lacks {
            let low = banned.to_lowercase();
            if has_phrase(&words, &words_of(&low)) {
                out.push(format!("the answer says {banned:?}"));
            }
        }
        if let Some(word) = developer_word(said) {
            out.push(format!("the answer shows a developer word: {word:?}"));
        }
        out
    }
}

fn words_of(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect()
}

fn has_phrase(words: &[&str], phrase: &[&str]) -> bool {
    !phrase.is_empty() && words.windows(phrase.len()).any(|w| w == phrase)
}

/// The first word of `said` that is an identifier rather than language: a name with an
/// underscore (`org_quire_Memory-memory_recall`) or with two dots or more (`mail.thread.search`).
pub fn developer_word(said: &str) -> Option<&str> {
    said.split_whitespace()
        .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()))
        .find(|t| is_identifier(t))
}

fn is_identifier(token: &str) -> bool {
    token.chars().next().is_some_and(char::is_alphabetic)
        && token
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && (token.contains('_') || token.matches('.').count() >= 2)
        && !token.starts_with("www.")
}
