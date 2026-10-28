//! The terminal's standing grants, the pure half: whether a terminal session holds a grant, and
//! the task policy that carries it. A terminal cannot tell the person from an agent typing in
//! it, so every act that is not a read asks; the one way to skip the ask is the person's own
//! answer on the sheet, "Allow from the terminal: this action, until logout".

use docket_context::state::SessionRecord;
use docket_manifest::{ActionDecl, TargetKind};
use docket_permissions_core::{ActionMatch, TaskPolicy, TaskPolicyState};
use docket_vocab::{ActionRef, LabelText};
use policy_point::TerminalGrant;
use porter_ids::Count;
use prov::{Actor, EntityKind, UnixSeconds};
use std::collections::BTreeSet;

/// Whether the terminal session `record` holds a standing grant for `action` at `now`.
pub fn held(record: &SessionRecord, action: &ActionRef, now: UnixSeconds) -> TerminalGrant {
    let named = ActionMatch::One(action.clone());
    match &record.policy {
        Some(policy)
            if record.actor == Actor::Cli
                && policy.state == TaskPolicyState::Active
                && policy.expires >= now
                && policy.actions.contains(&named) =>
        {
            TerminalGrant::Granted
        }
        _ => TerminalGrant::NotGranted,
    }
}

/// The kinds of thing `decl` acts on, which a terminal grant for it covers.
pub fn kinds_of(decl: &ActionDecl) -> BTreeSet<EntityKind> {
    match &decl.on {
        TargetKind::One(kind) | TargetKind::Many(kind) => BTreeSet::from([kind.clone()]),
        TargetKind::Nothing | TargetKind::Text | TargetKind::Files => BTreeSet::new(),
    }
}

/// The task policy a terminal session starts with when the person first allows an action from
/// the terminal: no action yet, the declaration's kinds and ceiling, until logout.
pub fn grant_policy(record: &SessionRecord, decl: &ActionDecl, mass_at: Count) -> TaskPolicy {
    TaskPolicy {
        task: record.task.clone(),
        space: record.space.clone(),
        from: vec![],
        actions: BTreeSet::new(),
        kinds: kinds_of(decl),
        ceiling: decl.effect,
        max_count: mass_at,
        recipients: vec![],
        destinations: vec![],
        paths: vec![],
        expires: UnixSeconds(i64::MAX),
        rationale: LabelText::parse("Allowed from the terminal until logout")
            .expect("a fixed sheet phrase obeys the label grammar"),
        state: TaskPolicyState::Active,
    }
}

/// Why a revocation did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Revoked {
    /// The grant is gone.
    Done,
    /// The terminal held no grant for that action.
    NotHeld,
}
