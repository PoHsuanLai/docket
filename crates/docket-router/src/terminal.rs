//! The terminal's standing grants. A terminal cannot tell the person from an agent typing in
//! it, so every act that is not a read asks; the one way to skip the ask is the person's own
//! answer on the sheet, "Allow from the terminal: this action, until logout". The grant is kept
//! as a task policy of the terminal's session that names the action (`ActionMatch::One`), so
//! the control centre reads it like any policy and revokes it; it ends with the session. There
//! is no flag, option or environment variable that grants it.

use crate::prepared::Prepared;
use crate::router::Router;
use crate::seams::{Clock, EventSink, Seams};
use crate::state::{RouterState, SessionRecord};
use docket_core::{
    ActionDecl, ActionMatch, ActionRef, AgentReach, AskReason, AuditRecord, CallerRole, LabelText,
    TargetKind, TaskPolicy, TaskPolicyState,
};
use policy_point::TerminalGrant;
use porter_core::Count;
use prov::{Actor, Effect, EntityKind, UnixSeconds};
use std::collections::BTreeSet;

/// Whether the terminal session `record` holds a standing grant for `action` at `now`.
pub(crate) fn held(record: &SessionRecord, action: &ActionRef, now: UnixSeconds) -> TerminalGrant {
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

/// Whether the sheet may offer the standing grant for `p`: it comes from the terminal, the only
/// thing that asks is the terminal rule itself (no Rule of Two, untrusted sink, mass or
/// memory rule would ask on top, which a grant could not lift), and the action is neither
/// destructive nor one that asks every time.
pub(crate) fn offers_grant(p: &Prepared, why: &[AskReason]) -> bool {
    p.who.actor == Actor::Cli
        && p.decl.effect != Effect::Destructive
        && p.decl.reach == AgentReach::Offered
        && why == [AskReason::FromTerminal]
}

fn kinds_of(decl: &ActionDecl) -> BTreeSet<EntityKind> {
    match &decl.on {
        TargetKind::One(kind) | TargetKind::Many(kind) => BTreeSet::from([kind.clone()]),
        TargetKind::Nothing | TargetKind::Text | TargetKind::Files => BTreeSet::new(),
    }
}

fn grant_policy(record: &SessionRecord, decl: &ActionDecl, mass_at: Count) -> TaskPolicy {
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

impl RouterState {
    /// The terminal sessions: every implicit session of the `Cli` role.
    fn terminal_sessions(&self) -> Vec<prov::SessionId> {
        self.implicit
            .iter()
            .filter(|((role, _), _)| *role == CallerRole::Cli)
            .map(|(_, session)| session.clone())
            .collect()
    }
}

impl<S: Seams> Router<S> {
    /// The person answered "allow from the terminal": the terminal session of `p` now holds the
    /// grant. Ignored for any call that did not come from the terminal.
    pub(crate) fn record_terminal_grant(&self, p: &Prepared) {
        let (Some(session), true) = (&p.who.session, p.who.actor == Actor::Cli) else {
            return;
        };
        let at = self.seams.clock().now();
        let task = {
            let mut st = self.locked();
            let Some(record) = st.sessions.get_mut(session) else {
                return;
            };
            let mut policy = record
                .policy
                .take()
                .unwrap_or_else(|| grant_policy(record, &p.decl, self.config.mass_at));
            policy
                .actions
                .insert(ActionMatch::One(p.request.action.clone()));
            policy.kinds.extend(kinds_of(&p.decl));
            policy.ceiling = policy.ceiling.max(p.decl.effect);
            let task = policy.task.clone();
            record.policy = Some(policy);
            task
        };
        self.seams.sink().append(AuditRecord::TaskPolicy {
            at,
            task,
            state: TaskPolicyState::Active,
            change: docket_core::PolicyChangeKind::Widens,
        });
    }

    /// The actions the terminal may run without asking, until logout: what the control centre
    /// shows.
    pub fn terminal_grants(&self) -> Vec<ActionRef> {
        let st = self.locked();
        st.terminal_sessions()
            .iter()
            .filter_map(|id| st.sessions.get(id))
            .filter_map(|r| r.policy.as_ref())
            .flat_map(|p| p.actions.iter())
            .filter_map(|m| match m {
                ActionMatch::One(action) => Some(action.clone()),
                ActionMatch::AppUpTo(..) => None,
            })
            .collect()
    }

    /// Withdraws the standing grant for `action`: the next call from the terminal asks again.
    pub fn revoke_terminal_grant(&self, action: &ActionRef) -> Revoked {
        let at = self.seams.clock().now();
        let task = {
            let mut st = self.locked();
            let mut revoked = None;
            for id in st.terminal_sessions() {
                if let Some(record) = st.sessions.get_mut(&id)
                    && let Some(policy) = record.policy.as_mut()
                    && policy.actions.remove(&ActionMatch::One(action.clone()))
                {
                    revoked = Some(policy.task.clone());
                }
            }
            revoked
        };
        match task {
            Some(task) => {
                self.seams.sink().append(AuditRecord::TaskPolicy {
                    at,
                    task,
                    state: TaskPolicyState::Revoked,
                    change: docket_core::PolicyChangeKind::Narrows,
                });
                Revoked::Done
            }
            None => Revoked::NotHeld,
        }
    }

    /// Logout: every terminal session ends and takes its standing grants with it. The daemon
    /// calls this when the person's session ends.
    pub fn end_terminal_sessions(&self) {
        let mut st = self.locked();
        for id in st.terminal_sessions() {
            st.sessions.remove(&id);
        }
        st.implicit.retain(|(role, _), _| *role != CallerRole::Cli);
    }
}
