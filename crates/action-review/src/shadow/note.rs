//! What a shadow run keeps: how the live Quick judge answered and what the shadow scored. No
//! request content, so a note holds nothing the person or a third party wrote.

use super::score::{ShadowFault, ShadowScore};
use crate::verdict::ReviewVerdict;
use docket_core::ReviewError;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// How the live Quick judge answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveCall {
    /// It passed the request.
    Passed,
    /// It flagged the request (any verdict that is not an allow).
    Flagged,
    /// It failed, which already asks.
    Failed,
}

impl LiveCall {
    /// The call a live result amounts to.
    pub fn of(live: &Result<ReviewVerdict, ReviewError>) -> Self {
        match live {
            Ok(ReviewVerdict::Allow) => Self::Passed,
            Ok(ReviewVerdict::Ask { .. } | ReviewVerdict::Deny { .. }) => Self::Flagged,
            Err(_) => Self::Failed,
        }
    }
}

/// One Quick review, shadowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowNote {
    /// What the live judge said.
    pub live: LiveCall,
    /// What the shadow scored, or why not.
    pub shadow: Result<ShadowScore, ShadowFault>,
}

/// Where notes go. Recording is all a sink does.
pub trait ShadowSink: Send + Sync {
    /// Keeps one note.
    fn record(&self, note: ShadowNote);
}

/// A sink that keeps notes in memory. Clones share the notes, so a run keeps one handle to read
/// them back from while the reviewer holds another.
#[derive(Debug, Clone, Default)]
pub struct ShadowLog(Arc<Mutex<Vec<ShadowNote>>>);

impl ShadowLog {
    /// Takes every note kept so far, leaving the log empty.
    pub fn take(&self) -> Vec<ShadowNote> {
        match self.0.lock() {
            Ok(mut notes) => std::mem::take(&mut *notes),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        }
    }
}

impl ShadowSink for ShadowLog {
    fn record(&self, note: ShadowNote) {
        match self.0.lock() {
            Ok(mut notes) => notes.push(note),
            Err(poisoned) => poisoned.into_inner().push(note),
        }
    }
}
