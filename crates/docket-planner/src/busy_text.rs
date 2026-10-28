//! A refusal because the app is busy, told with who holds it. The app has only one run at a
//! time: when a background task the planner started is the one running, the planner is told so
//! (and by which handle it holds that task) instead of retrying a call that cannot succeed.
//! Everything here is structure the planner view already carries: the roster's agents and states,
//! the last step of each, and the handles of tasks it started. No text an app wrote is used.

use crate::step_text::Seen;
use docket_core::{HandleShape, RosterDetail, RosterState, StepLine};
use prov::AgentRef;

/// The kind of thing a background task is.
const TASK_KIND: &str = "companion.task";

/// Whether a roster state is one in which a task still holds what it started.
fn holds(state: RosterState) -> bool {
    matches!(
        state,
        RosterState::Starting | RosterState::Working | RosterState::NeedsYou | RosterState::Paused
    )
}

/// How many live background tasks of the planner last used the app `step` called: those are
/// the ones that may hold it.
fn holders(step: &StepLine, seen: &Seen<'_>) -> usize {
    seen.roster
        .iter()
        .filter(|line| matches!(line.agent, AgentRef::Worker { .. }) && holds(line.state))
        .filter(|line| match &line.detail {
            RosterDetail::Full(full) => full
                .last
                .as_ref()
                .is_some_and(|last| last.action.app == step.action.app),
            RosterDetail::PresenceOnly => false,
        })
        .count()
}

/// The handles of the background tasks the planner holds, as `#n`.
fn task_handles(seen: &Seen<'_>) -> Vec<String> {
    seen.handles
        .iter()
        .filter(|card| matches!(&card.shape, HandleShape::Entity(kind) if kind.to_string() == TASK_KIND))
        .map(|card| format!("#{}", card.handle.0))
        .collect()
}

/// What to tell the planner about a refusal as busy.
pub(crate) fn busy_hint(step: &StepLine, seen: &Seen<'_>) -> String {
    let tasks = task_handles(seen);
    let own = match (holders(step, seen), tasks.as_slice()) {
        (0, _) => return "another run is already in progress, so this one cannot start; wait for it to finish, or ask the person with quire_ask".to_owned(),
        (1, [one]) => format!("your background task {one}"),
        (_, []) => "one of your background tasks".to_owned(),
        (_, many) => format!("one of your background tasks ({})", many.join(", ")),
    };
    format!(
        "another run is already in progress, started by {own}; do not start another: wait for its report, or message that task"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use docket_core::{HandleCard, RosterFull, RosterLine, StepEnd, StepShown};
    use docket_vocab::{ActionRef, CallId, CharCount, Handle};
    use prov::{ActionName, Effect, Source};

    fn action(app: &str) -> ActionRef {
        ActionRef {
            app: porter_core::AppName::parse(app).expect("app"),
            name: ActionName::parse("cua.run.start").expect("name"),
        }
    }

    fn step(app: &str) -> StepLine {
        StepLine {
            call: CallId(0),
            action: action(app),
            effect: Effect::Outbound,
            end: StepEnd::Interrupted,
            shown: StepShown::Full,
            with: Vec::new(),
        }
    }

    fn worker(last_app: &str, state: RosterState) -> RosterLine {
        RosterLine {
            agent: AgentRef::Worker {
                task: prov::TaskId::parse("t-27").expect("task"),
            },
            space: prov::SpaceId::desktop(),
            state,
            detail: RosterDetail::Full(Box::new(RosterFull {
                goal: docket_core::Reveal::Plain(String::new()),
                last: Some(step(last_app)),
                told: None,
                used: None,
                why: None,
            })),
        }
    }

    fn task_card(n: u64) -> HandleCard {
        HandleCard {
            handle: Handle(n),
            shape: HandleShape::Entity(prov::EntityKind::parse(TASK_KIND).expect("kind")),
            from: Source::Mail,
            size: CharCount(0),
        }
    }

    #[test]
    fn busy_names_the_planners_own_task_only_when_one_of_them_last_used_that_app() {
        let cua = "org.quire.Cua";
        let table = [
            (
                "nobody",
                vec![],
                vec![],
                "another run is already in progress, so this one cannot start;",
            ),
            (
                "one task",
                vec![worker(cua, RosterState::Working)],
                vec![task_card(4)],
                "started by your background task #4;",
            ),
            (
                "two tasks",
                vec![
                    worker(cua, RosterState::Working),
                    worker(cua, RosterState::Working),
                ],
                vec![task_card(4), task_card(5)],
                "started by one of your background tasks (#4, #5);",
            ),
            (
                "another app",
                vec![worker("org.quire.Mail", RosterState::Working)],
                vec![task_card(4)],
                "so this one cannot start;",
            ),
            (
                "a task that ended",
                vec![worker(cua, RosterState::Done)],
                vec![task_card(4)],
                "so this one cannot start;",
            ),
        ];
        for (name, roster, handles, want) in table {
            let seen = Seen {
                handles: &handles,
                earlier: &[],
                actions: &[],
                roster: &roster,
            };
            let hint = busy_hint(&step(cua), &seen);
            assert!(hint.contains(want), "case {name}: {hint}");
        }
    }
}
