//! `.Checkpoint.List` and `.Checkpoint.Plan`: a session's restore points and what restoring one
//! would change. Reads for the person's own surfaces (the role table keeps agents out); a
//! surface sees only the sessions it may bring back, as for a stored session (`may_restore`).

use crate::router::Router;
use crate::seams::Seams;
use docket_checkpoint::{CheckpointStore, PlanAsk, StoreFault, WorkRoot};
use docket_core::{
    CallerId, CallerRole, CheckpointEvent, CheckpointFault, CheckpointId, CheckpointList,
    CheckpointNote, CheckpointRow, IntentsReply, RestorePlan, Retention, SavedState, WireRefusal,
    Workspace,
};
use docket_session::{Claimant, may_restore, note_rows};
use prov::SessionId;

/// What a surface may know of one session's restore points.
struct View {
    cwd: Option<Workspace>,
    notes: Vec<CheckpointNote>,
}

/// Whether the log names point `id` as one that was saved (a turn's, or a restore's safety).
pub(crate) fn was_saved(notes: &[CheckpointNote], id: CheckpointId) -> bool {
    notes.iter().any(|note| match &note.event {
        CheckpointEvent::Taken(taken) => *taken == id,
        CheckpointEvent::Restored { safety, .. } => *safety == id,
        _ => false,
    })
}

/// A store's refusal as the wire tells it.
pub(crate) fn fault_of(fault: StoreFault) -> CheckpointFault {
    match fault {
        StoreFault::NoHistory => CheckpointFault::NoHistory,
        StoreFault::NoSuchPoint => CheckpointFault::NoSuchPoint,
        StoreFault::PlanStale => CheckpointFault::PlanStale,
        _ => CheckpointFault::Failed,
    }
}

impl<S: Seams> Router<S> {
    fn checkpoint_view(
        &self,
        caller: &CallerId,
        role: CallerRole,
        session: &SessionId,
    ) -> Result<View, WireRefusal> {
        let st = self.locked();
        let record = st.sessions.get(session).ok_or(WireRefusal::NoSuchSession)?;
        let claim = Claimant {
            role,
            app: &caller.app.name,
        };
        // Told to nobody: a session the caller may not bring back is one that does not exist.
        if !may_restore(claim, Some(&record.opener)) {
            return Err(WireRefusal::NoSuchSession);
        }
        Ok(View {
            cwd: record.cwd.clone(),
            notes: record.checkpoints.clone(),
        })
    }

    /// `.Checkpoint.List`: the log's notes, each saved point marked as still held or cleared.
    pub(crate) async fn checkpoint_list(
        &self,
        caller: &CallerId,
        role: CallerRole,
        session: &SessionId,
    ) -> IntentsReply {
        let view = match self.checkpoint_view(caller, role, session) {
            Ok(view) => view,
            Err(why) => return IntentsReply::Refused(why),
        };
        let cfg = self.agent_config();
        let held = match view.cwd.as_ref().and_then(|cwd| WorkRoot::of(cwd).ok()) {
            Some(root) => self
                .bounded(&cfg, self.seams.checkpoints().held(&root, session))
                .await
                .and_then(Result::ok)
                .unwrap_or_default(),
            None => Vec::new(),
        };
        let rows = note_rows(&view.notes)
            .into_iter()
            .map(|row| match row {
                CheckpointRow::Saved { id, turn, at, .. } => CheckpointRow::Saved {
                    id,
                    turn,
                    at,
                    state: if held.iter().any(|saved| saved.id == id) {
                        SavedState::Available
                    } else {
                        SavedState::Gone
                    },
                },
                other => other,
            })
            .collect();
        IntentsReply::Checkpoints(Box::new(CheckpointList {
            session: session.clone(),
            rows,
            keeps: Retention {
                last: cfg.checkpoint_keep,
                days: cfg.checkpoint_days,
            },
            turn: self.turn_state(session),
        }))
    }

    /// `.Checkpoint.Plan`: what restoring `id` would change. Writes nothing.
    pub(crate) async fn checkpoint_plan(
        &self,
        caller: &CallerId,
        role: CallerRole,
        session: &SessionId,
        id: CheckpointId,
    ) -> IntentsReply {
        match self.checkpoint_view(caller, role, session) {
            Ok(view) => IntentsReply::CheckpointPlan(self.plan_of(&view, session, id).await),
            Err(why) => IntentsReply::Refused(why),
        }
    }

    async fn plan_of(
        &self,
        view: &View,
        session: &SessionId,
        id: CheckpointId,
    ) -> Result<RestorePlan, CheckpointFault> {
        let root = view
            .cwd
            .as_ref()
            .and_then(|cwd| WorkRoot::of(cwd).ok())
            .ok_or(CheckpointFault::NoHistory)?;
        if !was_saved(&view.notes, id) {
            return Err(CheckpointFault::NoSuchPoint);
        }
        let ask = PlanAsk {
            root,
            session: session.clone(),
            id,
        };
        match self.seams.checkpoints().plan(ask).await {
            Ok(plan) => Ok(plan),
            // The log names it and the store does not hold it: it was cleared.
            Err(StoreFault::NoSuchPoint) => Err(CheckpointFault::Gone),
            Err(other) => Err(fault_of(other)),
        }
    }
}
