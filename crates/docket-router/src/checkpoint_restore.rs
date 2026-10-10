//! `org.quire.Checkpoints`, the built-in provider's one action: `checkpoints.restore`, which
//! puts a session's workspace back to one of its restore points.
//!
//! The router serves it (the store, the log and the clock are its own): intentd's provider only
//! reaches it through a port, as it does for `org.quire.Companion`. The action is destructive,
//! asks every time (`ask_always`), and its sheet is hold-to-confirm and once-only. The plan the
//! person saw is bound to the call by its digest: a folder that changed since is refused, and
//! the person looks again. Before anything is written the current files are saved as a new point
//! (the safety point), so a restore can itself be undone.

use crate::router::Router;
use crate::seams::{Clock, Seams};
use docket_checkpoint::{
    ApplyAsk, CheckpointStore, DropAsk, PlanAsk, StoreFault, TakeAsk, WorkRoot,
};
use docket_core::{
    AppRefusal, Args, CallRequest, CallerId, CallerRole, CheckpointEvent, CheckpointFault,
    CheckpointId, CheckpointNote, FailText, Follow, Invocation, LabelText, Outcome, ParamName,
    PlanDigest, Preview, TurnId, TurnState, Undoable, Value, WireRefusal,
};
use docket_session::{Claimant, may_restore};
use porter_core::AppName;
use prov::{Actor, AgentRole, SessionId};

/// The name the built-in provider answers to.
pub const CHECKPOINTS_APP: &str = "org.quire.Checkpoints";

/// The one action it declares.
pub const CHECKPOINTS_RESTORE: &str = "checkpoints.restore";

fn app() -> Option<AppName> {
    AppName::parse(CHECKPOINTS_APP).ok()
}

/// A refusal in the sheet's words (`4.6`): typed causes in, plain sentences out, no tool named.
fn say(fault: CheckpointFault) -> AppRefusal {
    let words = match fault {
        CheckpointFault::NoSuchPoint => "There is no such restore point.",
        CheckpointFault::Gone => "That restore point was cleared.",
        CheckpointFault::TurnRunning => "Wait for this turn to finish",
        CheckpointFault::PlanStale => "Files changed while you were deciding. Look again.",
        CheckpointFault::NoHistory => "This folder can't keep restore points.",
        _ => "The restore could not be completed.",
    };
    AppRefusal::Failed(FailText(words.to_owned()))
}

/// What the call asks.
struct Ask {
    session: SessionId,
    point: CheckpointId,
    digest: PlanDigest,
}

fn named<'a>(args: &'a Args, name: &str) -> Option<&'a Value> {
    ParamName::parse(name)
        .ok()
        .and_then(|name| args.get(&name))
        .map(|arg| &arg.value)
}

fn missing(name: &str) -> AppRefusal {
    match ParamName::parse(name) {
        Ok(param) => AppRefusal::NeedsParam {
            param,
            options: Vec::new(),
        },
        Err(_) => AppRefusal::Unsupported,
    }
}

/// Only the person, their terminal and the companion's planner may ask; an agent never does
/// (the policy forbids it too, and the router refuses its role before the call is made).
fn may_ask(actor: &Actor) -> bool {
    match actor {
        Actor::User { .. } | Actor::Cli => true,
        Actor::Companion { role, .. } => matches!(role, AgentRole::Planner),
        _ => false,
    }
}

fn ask_of(inv: &Invocation) -> Result<Ask, AppRefusal> {
    if inv.action.as_str() != CHECKPOINTS_RESTORE || !may_ask(&inv.actor) {
        return Err(AppRefusal::Unsupported);
    }
    let Some(Value::Text(session)) = named(&inv.args, "session") else {
        return Err(missing("session"));
    };
    let Some(Value::Integer(point)) = named(&inv.args, "point") else {
        return Err(missing("point"));
    };
    let Some(Value::Text(plan)) = named(&inv.args, "plan") else {
        return Err(missing("plan"));
    };
    Ok(Ask {
        session: SessionId::parse(session).map_err(|_| say(CheckpointFault::NoSuchPoint))?,
        point: u32::try_from(*point)
            .map(CheckpointId)
            .map_err(|_| say(CheckpointFault::NoSuchPoint))?,
        digest: PlanDigest(plan.clone()),
    })
}

impl<S: Seams> Router<S> {
    /// Whether a caller in `role` may make this call at all, before any gate: only the person's
    /// own surfaces, a terminal and the companion, and each only for a session it may bring
    /// back. Any other call passes. A session stored but not live is brought back first.
    pub(crate) async fn checkpoint_reach(
        &self,
        caller: &CallerId,
        role: CallerRole,
        call: &CallRequest,
    ) -> Result<(), WireRefusal> {
        if call.action.app.as_str() != CHECKPOINTS_APP {
            return Ok(());
        }
        if !matches!(
            role,
            CallerRole::Launcher
                | CallerRole::Field
                | CallerRole::Editor
                | CallerRole::Companion
                | CallerRole::Cli
        ) {
            return Err(WireRefusal::NotAllowed);
        }
        let Some(Value::Text(text)) = named(&call.args, "session") else {
            // The argument check refuses a call without it.
            return Ok(());
        };
        let Ok(session) = SessionId::parse(text) else {
            return Ok(());
        };
        let claim = Claimant {
            role,
            app: &caller.app.name,
        };
        if !self.locked().sessions.contains_key(&session) {
            let _ = self.restore_for(&session, Some(claim)).await;
        }
        let opener = self
            .locked()
            .sessions
            .get(&session)
            .map(|record| record.opener.clone());
        match opener {
            Some(opener) if may_restore(claim, Some(&opener)) => Ok(()),
            _ => Err(WireRefusal::NoSuchSession),
        }
    }

    /// The workspace and the notes of the session a restore names.
    fn restore_target(
        &self,
        session: &SessionId,
    ) -> Result<(WorkRoot, Vec<CheckpointNote>), AppRefusal> {
        let st = self.locked();
        let record = st
            .sessions
            .get(session)
            .ok_or_else(|| say(CheckpointFault::NoSuchPoint))?;
        let root = record
            .cwd
            .as_ref()
            .and_then(|cwd| WorkRoot::of(cwd).ok())
            .ok_or_else(|| say(CheckpointFault::NoHistory))?;
        Ok((root, record.checkpoints.clone()))
    }

    /// Refuses `TurnRunning` while the session's agent is working or a save or a restore of its
    /// workspace is in flight.
    fn restore_clear(&self, session: &SessionId) -> Result<(), AppRefusal> {
        let stepping = self.locked().stepping.contains(session);
        match (self.turn_state(session), stepping) {
            (TurnState::Idle, false) => Ok(()),
            _ => Err(say(CheckpointFault::TurnRunning)),
        }
    }

    /// `DryRun` of `checkpoints.restore`: the sheet's lines for the plan as the folder is now.
    /// A plan that is not the one the call carries is refused, so the sheet never shows one
    /// thing while another is confirmed.
    pub async fn checkpoints_dry_run(&self, inv: Invocation) -> Result<Preview, AppRefusal> {
        let ask = ask_of(&inv)?;
        let (root, notes) = self.restore_target(&ask.session)?;
        self.restore_clear(&ask.session)?;
        if !crate::checkpoint_read::was_saved(&notes, ask.point) {
            return Err(say(CheckpointFault::NoSuchPoint));
        }
        let plan = self
            .seams
            .checkpoints()
            .plan(PlanAsk {
                root,
                session: ask.session,
                id: ask.point,
            })
            .await
            .map_err(|fault| match fault {
                StoreFault::NoSuchPoint => say(CheckpointFault::Gone),
                other => say(crate::checkpoint_read::fault_of(other)),
            })?;
        if plan.digest != ask.digest {
            return Err(say(CheckpointFault::PlanStale));
        }
        let provider = app().ok_or(AppRefusal::Unsupported)?;
        Ok(Preview::Facts(crate::checkpoint_sheet::restore_sheet(
            &plan, &provider,
        )))
    }

    /// `Perform` of `checkpoints.restore`: saves the files as they are, writes the point's files
    /// back, and records the restore in the session's log.
    pub async fn checkpoints_perform(&self, inv: Invocation) -> Result<Outcome, AppRefusal> {
        let ask = ask_of(&inv)?;
        let (root, notes) = self.restore_target(&ask.session)?;
        let Some(_step) = self.begin_step(&ask.session) else {
            return Err(say(CheckpointFault::TurnRunning));
        };
        // The mark is taken first, so no turn's save starts while this checks the turn.
        if self.turn_state(&ask.session) == TurnState::Running {
            return Err(say(CheckpointFault::TurnRunning));
        }
        if !crate::checkpoint_read::was_saved(&notes, ask.point) {
            return Err(say(CheckpointFault::NoSuchPoint));
        }
        let cfg = self.agent_config();
        let store = self.seams.checkpoints();
        let held = store
            .held(&root, &ask.session)
            .await
            .map_err(|fault| say(crate::checkpoint_read::fault_of(fault)))?;
        if !held.iter().any(|saved| saved.id == ask.point) {
            return Err(say(CheckpointFault::Gone));
        }
        let safety = self.next_point(&ask.session, &held);
        store
            .take(TakeAsk {
                root: root.clone(),
                session: ask.session.clone(),
                id: safety,
                at: self.seams.clock().now(),
                max: cfg.checkpoint_max_files,
            })
            .await
            .map_err(|fault| say(crate::checkpoint_read::fault_of(fault)))?;
        let applied = store
            .apply(ApplyAsk {
                root: root.clone(),
                session: ask.session.clone(),
                id: ask.point,
                expect: ask.digest,
            })
            .await;
        if let Err(fault) = applied {
            // Nothing was restored: the safety point is not needed, and not in the log.
            let forget = DropAsk {
                root: root.clone(),
                session: ask.session.clone(),
                ids: vec![safety],
            };
            let _ = store.drop_points(forget).await;
            return Err(say(crate::checkpoint_read::fault_of(fault)));
        }
        let turn = self
            .locked()
            .sessions
            .get(&ask.session)
            .and_then(|record| record.turns.last().map(|turn| turn.id))
            .unwrap_or(TurnId(0));
        let event = CheckpointEvent::Restored {
            to: ask.point,
            safety,
        };
        self.note_checkpoint(&ask.session, turn, event);
        self.prune_session(&ask.session, &root, &cfg).await;
        Ok(Outcome {
            value: None,
            said: LabelText::parse("Restored your files").ok(),
            show: Preview::None,
            undo: Undoable::No,
            follow: Follow::Nothing,
        })
    }
}
