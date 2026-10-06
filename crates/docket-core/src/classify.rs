//! Per-call effect: an action's declared effect is a ceiling, and an action that opts in
//! (`per_call = "classified"` in its manifest) lets its provider say what one particular call
//! does. The router never lets that answer pass the ceiling (see `docket-router::classify`).
//!
//! `Delegates` is the answer of a provider whose call only forwards to another typed action
//! (a menu item that is a verified command id). The outer call is then gated as `Read`, and the
//! provider carries out the real action by answering a `Follow::Next` for exactly that action:
//! the router runs it as a child of the outer call, in the same chain and session, with its own
//! full gate. `Delegates` never lets a call with a `Destructive` effect of its own go unasked:
//! once a call delegates, its own effect is `Read`, and whatever it then does is a gated call.

use crate::ids::ActionRef;
use prov::Effect;
use serde::{Deserialize, Serialize};

/// Whether an action's effect is the same for every call, or its provider says per call.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerCall {
    /// The declared effect holds for every call.
    #[default]
    Declared,
    /// The declared effect is a ceiling: the provider classifies each call (`Classify`).
    Classified,
}

impl PerCall {
    /// For `skip_serializing_if`: the default is not written, so existing files are unchanged.
    pub fn is_declared(&self) -> bool {
        *self == PerCall::Declared
    }
}

/// What a provider says one call is.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum CallClass {
    /// This call has this effect (never more than the ceiling: the router clamps it).
    Effect(Effect),
    /// This call only forwards to this typed action, which is gated on its own.
    Delegates(ActionRef),
}

/// Why a classification fell back to the ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassifyFault {
    /// The provider answered with a refusal.
    Refused,
    /// It did not answer in time.
    TimedOut,
    /// It is not there, or answered something that is not the protocol.
    Unavailable,
    /// It has no `Classify` (the default).
    Unsupported,
    /// It delegated to an action that is not known, or to the action itself.
    BadDelegate,
    /// `Perform` found the call is no longer what was classified (`ClassificationChanged`).
    Changed,
}

/// What the provider's answer was, for the audit.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ClassifyAnswer {
    /// It said this.
    Said(CallClass),
    /// It said nothing usable; the ceiling holds.
    Failed(ClassifyFault),
}

/// How one call was classified: what the router gated on and what it passes to `Perform`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Classification {
    /// The declared effect.
    pub ceiling: Effect,
    /// What the provider answered.
    pub answer: ClassifyAnswer,
    /// The effect the call was gated on: `min(answer, ceiling)`, `Read` for a delegation, the
    /// ceiling for a failure.
    pub used: Effect,
    /// What `Perform` is told (`"classified"` in the invocation's JSON).
    pub sent: CallClass,
}

/// How a delegation ended, for the audit.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum DelegationEnd {
    /// The provider answered a follow-up call for the delegated action; it is gated on its own.
    Followed,
    /// It made no inner call: nothing was done beyond what `Read` allows.
    Unused,
    /// It made a different inner call, which is gated normally.
    Different(ActionRef),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wire_forms_are_pinned() {
        let json = |v: &CallClass| serde_json::to_string(v).expect("json");
        assert_eq!(
            json(&CallClass::Effect(Effect::Read)),
            r#"{"kind":"effect","v":"read"}"#
        );
        assert_eq!(PerCall::default(), PerCall::Declared);
        assert!(PerCall::Declared.is_declared());
        assert_eq!(
            serde_json::to_string(&PerCall::Classified).expect("json"),
            "\"classified\""
        );
    }
}
