//! The planner's three helper actions, `companion.helper.post_job`, `companion.helper.propose` and
//! `companion.routine.propose`. (`companion.brief.confirm`, which checks a brief with the person,
//! is the router's own and never reaches the host.)
//! The router gates them like any call (and refuses a goal, a name or a role that came from
//! something untrusted, and always asks the person about a proposal); once it has allowed one,
//! the host that keeps the helpers carries it out here. A host with none refuses them.

use crate::desk::{PostFromPlanner, Proposal, RoutineAsk, RoutineFor};
use crate::runtime::Companion;
use crate::seams::Surface;
use companion_model::helper::{Ceiling, GoalOrigin, JobId, PlannerSaw};
use companion_wire::HelperReply;
use docket_client::Transport as IntentsTransport;
use docket_core::{ActionCard, AppRefusal, CallRefusal, FailText, Outcome, from_checked_brief};
use docket_permissions_core::{COMPANION_APP, HAND_ON_JOB, PROPOSE_ROUTINE};
use docket_planner::Planner;
use docket_vocab::{CallRequest, Clock, LabelText, Value};
use docket_wire::NoteAsk;
use prov::TaskId;

/// Proposes a helper to the person.
pub(crate) const PROPOSE: &str = "companion.helper.propose";

/// Asks the person to check a brief before helpers work from it. The router answers it itself,
/// so it never goes through the desk.
pub(crate) const CONFIRM_BRIEF: &str = "companion.brief.confirm";

/// Whether the planner is offered `card` here: the three helper actions only where a host keeps
/// helpers (proposing), and giving a job only once the person keeps at least one. Every other
/// card is offered as before. Offering them nowhere else keeps the planner's action list short:
/// the list is cut to a budget, and two long cards no one can use would push out the last apps'.
pub(crate) fn offered_here(card: &ActionCard, keeps: Keeps) -> bool {
    if card.action.app.as_str() != COMPANION_APP {
        return true;
    }
    match (card.action.name.as_str(), keeps) {
        (HAND_ON_JOB, Keeps::Helpers) => true,
        (HAND_ON_JOB, Keeps::NoHelpers | Keeps::OnlyItself | Keeps::NoDesk) => false,
        (PROPOSE, Keeps::Helpers | Keeps::NoHelpers) => true,
        (PROPOSE, Keeps::OnlyItself | Keeps::NoDesk) => false,
        (PROPOSE_ROUTINE, Keeps::Helpers | Keeps::OnlyItself) => true,
        (PROPOSE_ROUTINE, Keeps::NoHelpers | Keeps::NoDesk) => false,
        (CONFIRM_BRIEF, Keeps::Helpers) => true,
        (CONFIRM_BRIEF, Keeps::NoHelpers | Keeps::OnlyItself | Keeps::NoDesk) => false,
        _ => true,
    }
}

/// What the host keeps, for which helper actions the planner is offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Keeps {
    /// No host keeps helpers here (a test, an app hosting the companion in itself).
    NoDesk,
    /// A host keeps helpers, and the person has none yet.
    NoHelpers,
    /// The planner is a helper's own and the person keeps no other: it can propose a routine for
    /// its card and nothing else of the helpers' actions.
    OnlyItself,
    /// The person keeps at least one helper the planner is told of.
    Helpers,
}

/// Whether `call` is one of the helper actions.
pub(crate) fn is_helper_action(call: &CallRequest) -> bool {
    call.action.app.as_str() == COMPANION_APP
        && matches!(
            call.action.name.as_str(),
            HAND_ON_JOB | PROPOSE | PROPOSE_ROUTINE
        )
}

fn refused(why: &str) -> CallRefusal {
    CallRefusal::App(AppRefusal::Failed(FailText(why.to_owned())))
}

fn text_of(call: &CallRequest, name: &str) -> Option<String> {
    call.args.iter().find_map(|(param, arg)| match &arg.value {
        Value::Text(text) if param.as_str() == name => Some(text.clone()),
        _ => None,
    })
}

/// The choice made for `name`, by its id.
fn choice_of(call: &CallRequest, name: &str) -> Option<String> {
    call.args.iter().find_map(|(param, arg)| match &arg.value {
        Value::Choice(choice) if param.as_str() == name => Some(choice.as_str().to_owned()),
        _ => None,
    })
}

/// How far the proposed helper may go: what was chosen, and looking when nothing was.
fn ceiling_of(call: &CallRequest) -> Option<Ceiling> {
    match choice_of(call, "ceiling").as_deref() {
        Some("look") | None => Some(Ceiling::Look),
        Some("change") => Some(Ceiling::Change),
        Some("send") => Some(Ceiling::Send),
        Some(_) => None,
    }
}

impl<P: Planner, I: IntentsTransport, K: Clock, S: Surface> Companion<P, I, K, S> {
    /// Carries out a helper action the router allowed, through the host's desk. The call ends as
    /// the router's outcome when the desk did it, and as the desk's plain refusal when it did not;
    /// the router, which answered before the desk acted, is told of a refusal so that its own
    /// record of the call does not say done.
    pub(crate) async fn through_desk(
        &mut self,
        task: &TaskId,
        call: &CallRequest,
        outcome: Outcome,
    ) -> Result<Outcome, CallRefusal> {
        let result = self.at_desk(task, call, outcome).await;
        if let Err(why) = &result {
            self.tell_desk_refused(task, call, why).await;
        }
        result
    }

    /// Amends the router's record of a helper action the desk refused.
    async fn tell_desk_refused(&self, task: &TaskId, call: &CallRequest, why: &CallRefusal) {
        let Some(session) = self.runtimes.get(task).map(|rt| rt.session.clone()) else {
            return;
        };
        let note = NoteAsk::DeskRefused {
            action: call.action.clone(),
            why: why.clone(),
        };
        if let Err(error) = self.intents.session_note(session, note).await {
            tracing::warn!(%task, %error, "the router was not told the desk refused a call");
        }
    }

    async fn at_desk(
        &mut self,
        task: &TaskId,
        call: &CallRequest,
        outcome: Outcome,
    ) -> Result<Outcome, CallRefusal> {
        let Some(desk) = self.desk.clone() else {
            return Err(refused("Helpers are not set up here."));
        };
        let Some(space) = self.runtimes.get(task).map(|rt| rt.space.clone()) else {
            return Err(CallRefusal::Timeout);
        };
        let reply = if call.action.name.as_str() == PROPOSE_ROUTINE {
            let (Some(goal), Some(when), Some(why)) = (
                text_of(call, "goal"),
                text_of(call, "when"),
                text_of(call, "why"),
            ) else {
                return Err(refused("Say what it is to do, when, and why."));
            };
            let helper = match self.as_helper.as_ref() {
                Some(own) => RoutineFor::Itself(own.id.clone()),
                None => match text_of(call, "helper") {
                    Some(name) => RoutineFor::Named(name),
                    None => return Err(refused("Say which helper it is for.")),
                },
            };
            desk.propose_routine(RoutineAsk {
                helper,
                goal,
                when,
                why,
                space,
            })
            .await
        } else if call.action.name.as_str() == HAND_ON_JOB {
            let (Some(helper), Some(goal)) = (text_of(call, "helper"), text_of(call, "goal"))
            else {
                return Err(refused("Say which helper and what it is to do."));
            };
            let origin = match from_checked_brief(&outcome) {
                true => GoalOrigin::ConfirmedBrief,
                false => GoalOrigin::Planner(PlannerSaw::Nothing),
            };
            desk.post_from_planner(PostFromPlanner {
                helper,
                goal,
                after: text_of(call, "after"),
                giver: task.clone(),
                space,
                origin,
            })
            .await
        } else {
            let (Some(name), Some(role), Some(ceiling)) = (
                text_of(call, "name"),
                text_of(call, "role"),
                ceiling_of(call),
            ) else {
                return Err(refused("Say what to call it and what it is for."));
            };
            let apps = text_of(call, "apps")
                .map(|apps| {
                    apps.split(',')
                        .map(|app| app.trim().to_owned())
                        .filter(|app| !app.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            desk.propose(Proposal {
                name,
                role,
                apps,
                ceiling,
                space,
            })
            .await
        };
        match reply {
            HelperReply::Refused(why) => Err(refused(&why)),
            HelperReply::Posted(job) => Ok(posted(outcome, text_of(call, "helper"), job)),
            HelperReply::Done | HelperReply::Made(_) | HelperReply::OpenTerminal(_) => Ok(outcome),
        }
    }
}

/// The outcome of giving a job, saying which helper has it and under what number (and that it
/// came from a brief the person checked), so the step
/// reads as finished and not as something to do again. The router's own words stay when the
/// helper's name is not at hand.
fn posted(outcome: Outcome, helper: Option<String>, job: JobId) -> Outcome {
    let from = match from_checked_brief(&outcome) {
        true => ", from your checked brief",
        false => "",
    };
    let said = helper
        .and_then(|name| LabelText::parse(&format!("Gave {name} the job {job}{from}")).ok())
        .or(outcome.said);
    Outcome { said, ..outcome }
}
