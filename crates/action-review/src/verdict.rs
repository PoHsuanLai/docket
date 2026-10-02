//! What a reviewer says.

use docket_core::{ReasonCode, ReasonText, VerdictKind};
use serde::{Deserialize, Serialize};

/// A reviewer's coded reason, with words for the person's activity view only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewReason {
    /// The code.
    pub code: ReasonCode,
    /// At most 200 characters, model-written. Never shown to a planner.
    pub text: ReasonText,
}

/// One stage's verdict. Cedar's `Ask` and `Deny` never reach a reviewer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ReviewVerdict {
    /// Nothing wrong with it.
    Allow,
    /// Ask the person.
    Ask {
        /// Why.
        why: ReviewReason,
    },
    /// Refuse.
    Deny {
        /// Why.
        why: ReviewReason,
    },
}

impl ReviewVerdict {
    /// The verdict without its reason, for the audit.
    pub fn kind(&self) -> VerdictKind {
        match self {
            ReviewVerdict::Allow => VerdictKind::Allow,
            ReviewVerdict::Ask { .. } => VerdictKind::Ask,
            ReviewVerdict::Deny { .. } => VerdictKind::Deny,
        }
    }
}
