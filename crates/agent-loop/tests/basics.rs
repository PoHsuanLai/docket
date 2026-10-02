//! The tier rule, the front pointer and completion notes.

mod support;

use agent_loop::*;
use porter_core::Count;
use prov::{AgentRef, ReportStatus, RunId};
use support::*;

#[test]
fn the_tier_rule_prefers_typed_then_hook_then_pixels() {
    use Offer::{Absent, Offered};
    let cases = [
        (
            "typed wins",
            Availability {
                typed: Offered,
                hook: Offered,
                cua: Offered,
            },
            Some(Tier::Typed),
        ),
        (
            "hook next",
            Availability {
                typed: Absent,
                hook: Offered,
                cua: Offered,
            },
            Some(Tier::Hook),
        ),
        (
            "pixels last",
            Availability {
                typed: Absent,
                hook: Absent,
                cua: Offered,
            },
            Some(Tier::Cua),
        ),
        (
            "nothing reaches the app",
            Availability {
                typed: Absent,
                hook: Absent,
                cua: Absent,
            },
            None,
        ),
        (
            "cua off by default is not offered",
            Availability {
                typed: Absent,
                hook: Offered,
                cua: Absent,
            },
            Some(Tier::Hook),
        ),
    ];
    for (name, available, want) in cases {
        assert_eq!(choose_tier(&available), want, "case: {name}");
    }
}

#[test]
fn the_front_pointer_follows_the_person_and_empties_when_its_task_ends() {
    let cases = [
        (
            "asking sets it",
            None,
            FrontEvent::Asked(task("t-1")),
            Some(task("t-1")),
        ),
        (
            "a newer ask replaces it",
            Some(task("t-1")),
            FrontEvent::Asked(task("t-2")),
            Some(task("t-2")),
        ),
        (
            "picking a row moves it",
            Some(task("t-1")),
            FrontEvent::Picked(task("t-5")),
            Some(task("t-5")),
        ),
        (
            "the front ending empties it",
            Some(task("t-1")),
            FrontEvent::Ended(task("t-1")),
            None,
        ),
        (
            "another task ending changes nothing",
            Some(task("t-1")),
            FrontEvent::Ended(task("t-3")),
            Some(task("t-1")),
        ),
    ];
    for (name, front, event, want) in cases {
        assert_eq!(front_step(front, &event), want, "case: {name}");
    }
}

#[test]
fn completion_lines_are_typed_facts_and_attention_is_rare() {
    let note = |agent: AgentRef, status, steps, values| CompletionNote {
        agent,
        status,
        steps: Count(steps),
        values: Count(values),
        attention: Attention::Quiet,
    };
    let run = AgentRef::Cua {
        run: RunId::parse("r-3").expect("run"),
    };
    assert_eq!(
        completion_line(&note(run, ReportStatus::Done, 9, 2)).as_deref(),
        Some("run r-3 finished: Done, 9 steps, 2 values")
    );
    assert_eq!(
        completion_line(&note(worker("t-4"), ReportStatus::Failed, 1, 0)).as_deref(),
        Some("task t-4 finished: Failed, 1 step, 0 values")
    );
    assert_eq!(
        completion_line(&note(worker("t-4"), ReportStatus::Progress, 1, 0)),
        None
    );
    let cases = [
        (ReportStatus::Done, AskedPerson::No, Attention::Quiet),
        (ReportStatus::Cancelled, AskedPerson::No, Attention::Quiet),
        (ReportStatus::Progress, AskedPerson::No, Attention::Quiet),
        (ReportStatus::Failed, AskedPerson::No, Attention::NeedsYou),
        (ReportStatus::Done, AskedPerson::Yes, Attention::NeedsYou),
    ];
    for (status, asked, want) in cases {
        assert_eq!(attention_of(status, asked), want, "{status:?} {asked:?}");
    }
}
