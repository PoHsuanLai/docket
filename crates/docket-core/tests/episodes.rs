//! The episode skeleton a finished task leaves.

mod support;

use almanac_core::{EpisodeKind, EpisodeOutcome, StepOutcome};
use docket_core::*;
use prov::{AgentRef, Effect, Integrity, TaskId};
use std::collections::BTreeSet;
use support::*;

fn ledger() -> TaskLedger {
    let thread = entity("mail.thread", "t1");
    TaskLedger {
        task: TaskId::parse("t-7").expect("task"),
        agent: AgentRef::Companion,
        parent: None,
        space: space("work"),
        started: at(100),
        asked: vec![UserTurn {
            id: TurnId(1),
            text: "archive the newsletters".into(),
            at: at(100),
            from: TurnSource::Launcher,
            via: TurnVia::Typed,
        }],
        steps: vec![
            LedgerStep {
                call: CallId(1),
                action: action("mail.thread.archive"),
                targets: vec![thread.clone()],
                effect: Effect::UndoableWrite,
                end: CallEnd::Done,
                undo: Some(UndoId(41)),
            },
            LedgerStep {
                call: CallId(2),
                action: action("mail.message.send"),
                targets: vec![],
                effect: Effect::Outbound,
                end: CallEnd::Refused(CallRefusal::Denied(DenyCode::NeedsUser)),
                undo: None,
            },
            LedgerStep {
                call: CallId(3),
                action: action("mail.thread.archive"),
                targets: vec![],
                effect: Effect::UndoableWrite,
                end: CallEnd::Refused(CallRefusal::Timeout),
                undo: None,
            },
        ],
        touched: vec![],
        results: vec![],
    }
}

#[test]
fn the_skeleton_is_trusted_typed_and_private_to_the_space() {
    let skeleton = skeleton_of(&ledger());
    assert_eq!(skeleton.label.integrity, Integrity::Trusted);
    assert_eq!(
        skeleton.label.confidentiality,
        prov::Confidentiality::Private(BTreeSet::from([space("work")]))
    );
    assert_eq!(skeleton.asked[0].as_str(), "archive the newsletters");
    let outcomes: Vec<StepOutcome> = skeleton.steps.iter().map(|s| s.outcome).collect();
    assert_eq!(
        outcomes,
        [StepOutcome::Done, StepOutcome::Denied, StepOutcome::Failed]
    );
    assert_eq!(
        skeleton.steps[0]
            .undo
            .as_ref()
            .map(prov::UndoHandle::as_str),
        Some("41")
    );
    assert!(skeleton.steps[1].undo.is_none());
}

#[test]
fn closing_a_task_writes_an_episode_without_a_narrative() {
    let episode = close(&ledger(), EpisodeKind::Task, at(160), EpisodeOutcome::Done)
        .expect("a valid episode id");
    assert_eq!(episode.id.as_str(), "t-7");
    assert_eq!((episode.started, episode.ended), (at(100), at(160)));
    assert!(episode.narrative.is_none());
    assert_eq!(episode.skeleton, skeleton_of(&ledger()));
}
