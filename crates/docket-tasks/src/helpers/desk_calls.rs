//! The planner's two helper actions, `companion.helper.post_job` and `companion.helper.propose`.
//! The router gates them like any call (and refuses a goal, a name or a role that came from
//! something untrusted, and always asks the person about a proposal); once it has allowed one,
//! the host that keeps the helpers carries it out here. A host with none refuses them.

use crate::desk::{PostFromPlanner, Proposal};
use crate::read::COMPANION_APP;
use crate::runtime::Companion;
use crate::seams::{Now, Surface};
use companion_wire::HelperReply;
use docket_client::Transport as IntentsTransport;
use docket_core::helper::Ceiling;
use docket_core::{ActionCard, AppRefusal, CallRefusal, CallRequest, FailText, Outcome, Value};
use porter_client::Transport as InferTransport;
use prov::TaskId;

/// Gives a job to a helper by its name.
pub(crate) const POST_JOB: &str = "companion.helper.post_job";
/// Proposes a helper to the person.
pub(crate) const PROPOSE: &str = "companion.helper.propose";

/// Whether the planner is offered `card` here: the two helper actions only where a host keeps
/// helpers (proposing), and giving a job only once the person keeps at least one. Every other
/// card is offered as before. Offering them nowhere else keeps the planner's action list short:
/// the list is cut to a budget, and two long cards no one can use would push out the last apps'.
pub(crate) fn offered_here(card: &ActionCard, keeps: Keeps) -> bool {
    if card.action.app.as_str() != COMPANION_APP {
        return true;
    }
    match (card.action.name.as_str(), keeps) {
        (POST_JOB, Keeps::Helpers) => true,
        (POST_JOB, Keeps::NoHelpers | Keeps::NoDesk) => false,
        (PROPOSE, Keeps::Helpers | Keeps::NoHelpers) => true,
        (PROPOSE, Keeps::NoDesk) => false,
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
    /// The person keeps at least one helper the planner is told of.
    Helpers,
}

/// Whether `call` is one of the two helper actions.
pub(crate) fn is_helper_action(call: &CallRequest) -> bool {
    call.action.app.as_str() == COMPANION_APP
        && matches!(call.action.name.as_str(), POST_JOB | PROPOSE)
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

impl<P: InferTransport, I: IntentsTransport, K: Now, S: Surface> Companion<P, I, K, S> {
    /// Carries out a helper action the router allowed, through the host's desk. The call ends as
    /// the router's outcome when the desk did it, and as the desk's plain refusal when it did not.
    pub(crate) async fn through_desk(
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
        let reply = if call.action.name.as_str() == POST_JOB {
            let (Some(helper), Some(goal)) = (text_of(call, "helper"), text_of(call, "goal"))
            else {
                return Err(refused("Say which helper and what it is to do."));
            };
            desk.post_from_planner(PostFromPlanner {
                helper,
                goal,
                after: text_of(call, "after"),
                giver: task.clone(),
                space,
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
            HelperReply::Done | HelperReply::Made(_) | HelperReply::Posted(_) => Ok(outcome),
        }
    }
}
