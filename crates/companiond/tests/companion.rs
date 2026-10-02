//! Completion notes reach only a live front task; restart rebuilds from a source.

use agent_loop::*;
use companion_wire::{AnswerPhase, NeedsYou, SessionRecord};
use companiond::*;
use docket_core::RosterState;
use prov::{AgentRef, ReportStatus, RunId, SpaceId, TaskId};

fn note(status: ReportStatus, attention: Attention) -> CompletionNote {
    CompletionNote {
        agent: AgentRef::Cua {
            run: RunId::parse("r-3").expect("run"),
        },
        status,
        steps: porter_count(9),
        values: porter_count(2),
        attention,
    }
}

fn working() -> LoopState {
    LoopState {
        phase: LoopPhase::Planning,
        turn: None,
        steps: 1,
        pending: vec![],
    }
}

#[test]
fn a_quiet_completion_is_one_line_in_a_live_front_task() {
    let effects = completion_effects(
        &note(ReportStatus::Done, Attention::Quiet),
        Some(&working()),
    );
    assert_eq!(
        effects,
        [LoopEffect::Note(
            "run r-3 finished: Done, 9 steps, 2 values".into()
        )]
    );
}

#[test]
fn a_result_that_needs_the_person_also_makes_the_answer_wait_for_them() {
    let effects = completion_effects(
        &note(ReportStatus::Failed, Attention::NeedsYou),
        Some(&working()),
    );
    assert_eq!(effects.len(), 2);
    assert!(matches!(
        &effects[1],
        LoopEffect::Publish(AnswerPhase::NeedsYou(NeedsYou::Question { .. }))
    ));
}

#[test]
fn nothing_is_added_to_a_finished_or_absent_front_task_or_for_progress() {
    let finished = LoopState {
        phase: LoopPhase::Finished(FinishedAs::Done),
        turn: None,
        steps: 3,
        pending: vec![],
    };
    assert!(
        completion_effects(&note(ReportStatus::Done, Attention::Quiet), Some(&finished)).is_empty()
    );
    assert!(completion_effects(&note(ReportStatus::Done, Attention::Quiet), None).is_empty());
    assert!(
        completion_effects(
            &note(ReportStatus::Progress, Attention::Quiet),
            Some(&working())
        )
        .is_empty()
    );
}

struct Scripted(Result<Vec<ReplayEvent>, ReplayFault>);

impl ReplaySource for Scripted {
    async fn events(&self) -> Result<Vec<ReplayEvent>, ReplayFault> {
        self.0.clone()
    }
}

#[tokio::test]
async fn restart_rebuilds_the_front_and_the_roster_from_the_stored_records() {
    let task = TaskId::parse("t-1").expect("task");
    let events = vec![ReplayEvent {
        at: prov::UnixSeconds(1),
        what: ReplayWhat::Session(Box::new(SessionRecord::Opened {
            task: task.clone(),
            space: SpaceId::parse("work").expect("space"),
            agent: AgentRef::Companion,
            parent: None,
        })),
    }];
    let rebuilt = recover(&Scripted(Ok(events))).await.expect("rebuilt");
    assert_eq!(rebuilt.front, Some(task));
    assert_eq!(rebuilt.roster().entries[0].state, RosterState::Working);
}

#[tokio::test]
async fn an_unreadable_record_is_a_fault_not_a_guess() {
    assert_eq!(
        recover(&Scripted(Err(ReplayFault::Malformed))).await,
        Err(ReplayFault::Malformed)
    );
    assert_eq!(
        recover(&Scripted(Err(ReplayFault::Unavailable))).await,
        Err(ReplayFault::Unavailable)
    );
}

#[test]
fn the_binary_is_a_skeleton_that_exits_two() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_companiond"))
        .output()
        .expect("runs");
    assert_eq!(output.status.code(), Some(2));
}

fn porter_count(n: u32) -> porter_core::Count {
    porter_core::Count(n)
}
