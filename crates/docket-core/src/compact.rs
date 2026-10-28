//! Compaction's vocabulary: what a summary of the start of a task is, and what it may say.
//!
//! A summary stands in for the oldest turns and steps of a task once the task outgrows its budget.
//! It is written by a model from what the planner was already shown (the person's own words, the
//! planner's own steps, handles as `#n`), never from a raw app result, so it cannot carry untrusted
//! text into trusted context. A handle it cites stays a handle: [`SummaryText::parse`] refuses a
//! summary that names a handle the planner never held. The integrity of a summary is never above
//! that of the task when it was written.

use docket_vocab::Handle;
use porter_core::Count;
use prov::Integrity;
use serde::{Deserialize, Serialize};

/// The most characters a summary may have.
pub const SUMMARY_MAX_CHARS: usize = 2000;

/// How much of a task's start a summary stands for: the oldest `turns` of the person's turns and
/// the oldest `steps` of its steps. It is a count from the start, so a later summary covers
/// everything an earlier one did, and a step is never split from its result (a step line is a
/// call and how it ended).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Covers {
    /// The person's turns it stands for, from the first.
    pub turns: Count,
    /// The steps it stands for, from the first.
    pub steps: Count,
}

impl Covers {
    /// Nothing covered.
    pub const NONE: Covers = Covers {
        turns: Count(0),
        steps: Count(0),
    };
}

/// Why a model's summary was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SummaryFault {
    /// It said nothing.
    #[error("the summary is empty")]
    Empty,
    /// It is longer than [`SUMMARY_MAX_CHARS`].
    #[error("the summary is too long")]
    TooLong,
    /// It names a handle the planner never held.
    #[error("the summary names handle #{0}, which the planner never held")]
    UnknownHandle(u64),
}

/// The words of a summary: a short account of the start of a task, in the planner's own terms.
/// Its `Debug` shows the length only, as the person's words may be in it.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SummaryText(String);

impl std::fmt::Debug for SummaryText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SummaryText(<{} bytes>)", self.0.len())
    }
}

/// The numbers written after a `#` in `text`, in order.
fn cited(text: &str) -> Vec<Option<u64>> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find('#') {
        rest = &rest[at + 1..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            found.push(digits.parse::<u64>().ok());
        }
    }
    found
}

impl SummaryText {
    /// A model's words as a summary, when they are not empty, fit, and cite (as `#n`) only
    /// handles in `held`: what the planner had been shown.
    pub fn parse(words: &str, held: &[Handle]) -> Result<Self, SummaryFault> {
        let words = words.trim();
        if words.is_empty() {
            return Err(SummaryFault::Empty);
        }
        if words.chars().count() > SUMMARY_MAX_CHARS {
            return Err(SummaryFault::TooLong);
        }
        for number in cited(words) {
            match number {
                Some(n) if held.iter().any(|h| h.0 == n) => {}
                Some(n) => return Err(SummaryFault::UnknownHandle(n)),
                None => return Err(SummaryFault::UnknownHandle(u64::MAX)),
            }
        }
        Ok(SummaryText(words.to_owned()))
    }

    /// The words.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A summary as the planner is given it: what it covers, what it says, and how far to trust it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    /// What it stands for.
    pub covers: Covers,
    /// What it says.
    pub text: SummaryText,
    /// `Untrusted` when the task was tainted when it was written: it keeps the taint of what it
    /// covers and never gains trust.
    pub integrity: Integrity,
}
