//! The sentencer: speech starts after about one sentence of the answer. Splits on `. ? !`
//! (ASCII `.` only before whitespace or the end, so `3.5` stays whole), on the full-width
//! `。？！`, and on newlines; caps each piece at 300 characters; joins very short pieces.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The longest piece, in characters.
pub const MAX_SENTENCE_CHARS: usize = 300;
/// Pieces shorter than this join their neighbour when the result still fits.
pub const MIN_SENTENCE_CHARS: usize = 12;

/// One piece to synthesise.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sentence(pub String);

// It is what the companion says about the person's data: Debug shows the length only.
impl fmt::Debug for Sentence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Sentence(<{} chars>)", self.0.chars().count())
    }
}

fn is_cjk(c: char) -> bool {
    matches!(c, '\u{3000}'..='\u{9fff}' | '\u{ff00}'..='\u{ffef}')
}

fn split(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut pieces = Vec::new();
    let mut current = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c == '\n' || c == '\r' {
            pieces.push(std::mem::take(&mut current));
            continue;
        }
        current.push(c);
        let ends = match c {
            '?' | '!' | '。' | '？' | '！' => true,
            '.' => chars.get(i + 1).is_none_or(|n| n.is_whitespace()),
            _ => false,
        };
        if ends {
            pieces.push(std::mem::take(&mut current));
        }
    }
    pieces.push(current);
    pieces
        .into_iter()
        .map(|p| p.trim().to_owned())
        .filter(|p| !p.is_empty())
        .collect()
}

fn cap(piece: String) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest: Vec<char> = piece.chars().collect();
    while rest.len() > MAX_SENTENCE_CHARS {
        let window = &rest[..MAX_SENTENCE_CHARS];
        let at = window
            .iter()
            .rposition(|c| c.is_whitespace())
            .filter(|&i| i > 0)
            .map_or(MAX_SENTENCE_CHARS, |i| i + 1);
        out.push(rest[..at].iter().collect::<String>().trim().to_owned());
        rest = rest[at..].to_vec();
    }
    let tail: String = rest.iter().collect::<String>().trim().to_owned();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

fn joined(a: &str, b: &str) -> String {
    let tight = a.chars().last().is_some_and(is_cjk) || b.chars().next().is_some_and(is_cjk);
    if tight {
        format!("{a}{b}")
    } else {
        format!("{a} {b}")
    }
}

fn short(p: &str) -> bool {
    p.chars().count() < MIN_SENTENCE_CHARS
}

fn fits(a: &str, b: &str) -> bool {
    joined(a, b).chars().count() <= MAX_SENTENCE_CHARS
}

/// The pieces of `text`, in order.
pub fn sentences(text: &str) -> Vec<Sentence> {
    let pieces: Vec<String> = split(text).into_iter().flat_map(cap).collect();
    let mut out: Vec<String> = Vec::new();
    for piece in pieces {
        match out.last_mut() {
            Some(last) if (short(last) || short(&piece)) && fits(last, &piece) => {
                *last = joined(last, &piece)
            }
            _ => out.push(piece),
        }
    }
    out.into_iter().map(Sentence).collect()
}
