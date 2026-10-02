//! The planner loop table: phases, effects, and the guarantees around refusals and halts.

mod support;

use agent_loop::*;
use companion_wire::{AnswerPhase, NeedsYou};
use docket_core::*;
use porter_core::{AppName, Count};
use prov::{ActionName, ReportStatus, SpaceScope};
use std::collections::BTreeMap;
use support::*;

fn state(phase: LoopPhase, pending: &[u64]) -> LoopState {
    LoopState {
        phase,
        turn: Some(TurnId(1)),
        steps: 1,
        pending: pending.iter().copied().map(CallId).collect(),
    }
}

fn request() -> CallRequest {
    CallRequest {
        action: ActionRef {
            app: AppName::parse("org.quire.Mail").expect("app"),
            name: ActionName::parse("mail.message.archive").expect("action"),
        },
        target: TargetValue::Nothing,
        args: BTreeMap::new(),
        origin: Origin::Companion,
    }
}

fn planned(n: usize) -> LoopInput {
    let call = || PlannedCall {
        call: request(),
        tier: Tier::Typed,
    };
    LoopInput::Planned(ModelOutput::Calls((0..n).map(|_| call()).collect()))
}

fn done() -> StepEnd {
    StepEnd::Done {
        said: None,
        value: None,
        undo: None,
    }
}

fn refused(r: CallRefusal) -> StepEnd {
    StepEnd::Refused(r)
}

fn run(from: LoopState, inputs: Vec<LoopInput>) -> (LoopState, Vec<LoopEffect>) {
    inputs
        .into_iter()
        .fold((from, vec![]), |(s, mut all), input| {
            let (next, effects) = agent_step(s, input);
            all.extend(effects);
            (next, all)
        })
}

#[test]
fn asking_starts_planning_and_a_planned_batch_waits_for_its_calls() {
    let (s, e) = agent_step(state(LoopPhase::Idle, &[]), LoopInput::Asked(TurnId(2)));
    assert_eq!((s.phase, s.turn), (LoopPhase::Planning, Some(TurnId(2))));
    assert_eq!(
        e,
        [
            LoopEffect::Publish(AnswerPhase::Thinking),
            LoopEffect::AskPlanner
        ]
    );
    let (s, e) = agent_step(s, planned(2));
    assert_eq!(s.phase, LoopPhase::AwaitingCalls);
    assert_eq!(s.pending, [CallId(0), CallId(1)]);
    assert_eq!(s.steps, 2);
    assert_eq!(e.len(), 2);
    assert!(e.iter().all(|x| matches!(x, LoopEffect::Call(_))));
}

#[test]
fn each_call_end_returns_to_planning_only_when_none_are_pending() {
    let s = state(LoopPhase::AwaitingCalls, &[0, 1]);
    let (s, e) = agent_step(s, LoopInput::CallEnded(CallId(1), done()));
    assert_eq!((s.phase, e.is_empty()), (LoopPhase::AwaitingCalls, true));
    let (s, e) = agent_step(s, LoopInput::CallEnded(CallId(0), done()));
    assert_eq!(s.phase, LoopPhase::Planning);
    assert_eq!(e, [LoopEffect::AskPlanner]);
}

#[test]
fn an_unknown_or_repeated_call_end_changes_nothing() {
    let s = state(LoopPhase::AwaitingCalls, &[0]);
    let (same, e) = agent_step(s.clone(), LoopInput::CallEnded(CallId(9), done()));
    assert_eq!((same, e), (s, vec![]));
}

#[test]
fn a_refusal_reaches_the_planner_as_the_coarse_code_and_is_never_lost() {
    let cases = [
        CallRefusal::Denied(DenyCode::OutsideTask),
        CallRefusal::Timeout,
        CallRefusal::Unconfirmed(ConfirmEnd::Refused),
    ];
    for r in cases {
        let (s, e) = agent_step(
            state(LoopPhase::AwaitingCalls, &[0]),
            LoopInput::CallEnded(CallId(0), refused(r.clone())),
        );
        assert_eq!(s.phase, LoopPhase::Planning, "{r:?}");
        assert_eq!(e, [LoopEffect::Refused(r), LoopEffect::AskPlanner]);
    }
}

#[test]
fn a_trip_pauses_until_the_person_speaks_and_resume_replans() {
    for trip in [
        BreakerTrip::Consecutive,
        BreakerTrip::Recent,
        BreakerTrip::Probing,
    ] {
        let (s, e) = agent_step(
            state(LoopPhase::AwaitingCalls, &[0, 1]),
            LoopInput::Tripped(trip),
        );
        assert_eq!(s.phase, LoopPhase::Paused(trip));
        assert!(matches!(
            e.as_slice(),
            [LoopEffect::Publish(AnswerPhase::NeedsYou(
                NeedsYou::Question { .. }
            ))]
        ));
        let (s, e) = agent_step(s, LoopInput::CallEnded(CallId(0), done()));
        assert_eq!((s.phase, e.is_empty()), (LoopPhase::Paused(trip), true));
        let (s, _) = agent_step(s, LoopInput::CallEnded(CallId(1), done()));
        let (s, e) = agent_step(s, LoopInput::Resumed);
        assert_eq!(
            (s.phase, e),
            (LoopPhase::Planning, vec![LoopEffect::AskPlanner])
        );
    }
}

#[test]
fn a_paused_refusal_from_the_router_pauses_and_a_second_trip_keeps_the_first() {
    let (s, e) = agent_step(
        state(LoopPhase::AwaitingCalls, &[0]),
        LoopInput::CallEnded(CallId(0), refused(CallRefusal::Paused(BreakerTrip::Recent))),
    );
    assert_eq!(s.phase, LoopPhase::Paused(BreakerTrip::Recent));
    assert!(matches!(e[0], LoopEffect::Refused(CallRefusal::Paused(_))));
    let (s, e) = agent_step(s, LoopInput::Tripped(BreakerTrip::Probing));
    assert_eq!(
        (s.phase, e),
        (LoopPhase::Paused(BreakerTrip::Recent), vec![])
    );
}

#[test]
fn halts_cancels_and_a_spent_budget_finish_the_loop() {
    let live = |p| state(p, &[0]);
    for phase in [
        LoopPhase::Idle,
        LoopPhase::Planning,
        LoopPhase::AwaitingCalls,
    ] {
        for input in [LoopInput::Halted, LoopInput::Cancelled] {
            let (s, e) = agent_step(live(phase), input);
            assert_eq!(s.phase, LoopPhase::Finished(FinishedAs::Cancelled));
            assert!(s.pending.is_empty());
            assert_eq!(
                e,
                [
                    LoopEffect::Publish(AnswerPhase::Cancelled),
                    LoopEffect::CloseTask
                ]
            );
        }
    }
    let (s, e) = agent_step(
        live(LoopPhase::AwaitingCalls),
        LoopInput::CallEnded(CallId(0), refused(CallRefusal::Halted(SpaceScope::Any))),
    );
    assert_eq!(s.phase, LoopPhase::Finished(FinishedAs::Cancelled));
    assert_eq!(e.last(), Some(&LoopEffect::CloseTask));
    let (s, e) = agent_step(
        live(LoopPhase::AwaitingCalls),
        LoopInput::CallEnded(
            CallId(0),
            refused(CallRefusal::OverBudget(BudgetKind::Calls)),
        ),
    );
    assert_eq!(s.phase, LoopPhase::Finished(FinishedAs::Failed));
    assert!(matches!(e[0], LoopEffect::Refused(_)));
    assert_eq!(e.last(), Some(&LoopEffect::CloseTask));
}

#[test]
fn a_finished_loop_ignores_everything() {
    let over = state(LoopPhase::Finished(FinishedAs::Done), &[]);
    let inputs = [
        LoopInput::Asked(TurnId(5)),
        LoopInput::Planned(ModelOutput::Finish),
        LoopInput::Resumed,
        LoopInput::Tripped(BreakerTrip::Recent),
        LoopInput::Cancelled,
        LoopInput::ModelFailed,
        LoopInput::Completed(note(ReportStatus::Done, Attention::Quiet)),
    ];
    for input in inputs {
        assert_eq!(agent_step(over.clone(), input), (over.clone(), vec![]));
    }
}

#[test]
fn the_model_outputs_that_end_a_step() {
    let planning = state(LoopPhase::Planning, &[]);
    let ask = ReaderAsk {
        inputs: vec![Handle(1)],
        want: ValueSchema::Date,
        task: ReaderTask::Extract,
    };
    let (s, e) = agent_step(
        planning.clone(),
        LoopInput::Planned(ModelOutput::Read(ask.clone())),
    );
    assert_eq!(s.phase, LoopPhase::AwaitingReader);
    assert_eq!(e, [LoopEffect::Read(Box::new(ask))]);
    let (s, e) = agent_step(s, LoopInput::ReadAnswered(Reveal::Handle(Handle(2))));
    assert_eq!(
        (s.phase, e),
        (LoopPhase::Planning, vec![LoopEffect::AskPlanner])
    );

    let (s, e) = agent_step(
        planning.clone(),
        LoopInput::Planned(ModelOutput::Say("hi".into())),
    );
    assert_eq!(
        (s.phase, e),
        (
            LoopPhase::Planning,
            vec![LoopEffect::Publish(AnswerPhase::Streaming)]
        )
    );

    let question = ModelOutput::Ask {
        text: "Which?".into(),
        choices: vec!["a".into()],
    };
    let (s, e) = agent_step(planning.clone(), LoopInput::Planned(question));
    assert_eq!(s.phase, LoopPhase::Idle);
    assert_eq!(
        e,
        [LoopEffect::Publish(AnswerPhase::NeedsYou(
            NeedsYou::Question {
                text: "Which?".into(),
                choices: vec!["a".into()],
            }
        ))]
    );
    let (s, _) = agent_step(s, LoopInput::Asked(TurnId(2)));
    assert_eq!(s.phase, LoopPhase::Planning);

    let (s, e) = agent_step(planning.clone(), LoopInput::Planned(ModelOutput::Finish));
    assert_eq!(s.phase, LoopPhase::Finished(FinishedAs::Done));
    assert_eq!(
        e,
        [
            LoopEffect::Publish(AnswerPhase::Done),
            LoopEffect::CloseTask
        ]
    );

    let (s, _) = agent_step(
        planning.clone(),
        LoopInput::Planned(ModelOutput::Calls(vec![])),
    );
    assert_eq!(
        s.phase,
        LoopPhase::Finished(FinishedAs::Failed),
        "an empty batch is a bad reply"
    );
    let (s, e) = agent_step(planning, LoopInput::ModelFailed);
    assert_eq!(s.phase, LoopPhase::Finished(FinishedAs::Failed));
    assert_eq!(
        e,
        [
            LoopEffect::Publish(AnswerPhase::Failed),
            LoopEffect::CloseTask
        ]
    );
}

#[test]
fn inputs_that_do_not_fit_the_phase_change_nothing() {
    let cases = [
        (
            "plan while calls are out",
            LoopPhase::AwaitingCalls,
            LoopInput::Planned(ModelOutput::Finish),
        ),
        (
            "a read answer with no read",
            LoopPhase::Planning,
            LoopInput::ReadAnswered(Reveal::Handle(Handle(1))),
        ),
        (
            "resume without a pause",
            LoopPhase::Planning,
            LoopInput::Resumed,
        ),
        (
            "model failure while idle",
            LoopPhase::Idle,
            LoopInput::ModelFailed,
        ),
    ];
    for (name, phase, input) in cases {
        let s = state(phase, &[0]);
        assert_eq!(agent_step(s.clone(), input), (s, vec![]), "case: {name}");
    }
}

fn note(status: ReportStatus, attention: Attention) -> CompletionNote {
    CompletionNote {
        agent: worker("t-4"),
        status,
        steps: Count(2),
        values: Count(1),
        attention,
    }
}

#[test]
fn a_completion_note_is_appended_and_never_starts_or_moves_the_loop() {
    for phase in [
        LoopPhase::Idle,
        LoopPhase::Planning,
        LoopPhase::AwaitingCalls,
        LoopPhase::Paused(BreakerTrip::Recent),
    ] {
        let s = state(phase, &[0]);
        let (after, e) = agent_step(
            s.clone(),
            LoopInput::Completed(note(ReportStatus::Done, Attention::Quiet)),
        );
        assert_eq!(after, s);
        assert_eq!(
            e,
            [LoopEffect::Note(
                "task t-4 finished: Done, 2 steps, 1 value".into()
            )]
        );
    }
    let (_, e) = agent_step(
        state(LoopPhase::Idle, &[]),
        LoopInput::Completed(note(ReportStatus::Failed, Attention::NeedsYou)),
    );
    assert_eq!(e.len(), 2);
    let (_, e) = agent_step(
        state(LoopPhase::Idle, &[]),
        LoopInput::Completed(note(ReportStatus::Progress, Attention::Quiet)),
    );
    assert!(e.is_empty(), "progress is not a completion");
}

#[test]
fn a_new_ask_while_working_updates_the_turn_and_replans_only_while_planning() {
    let (s, e) = agent_step(
        state(LoopPhase::AwaitingCalls, &[0]),
        LoopInput::Asked(TurnId(7)),
    );
    assert_eq!(
        (s.phase, s.turn, e),
        (LoopPhase::AwaitingCalls, Some(TurnId(7)), vec![])
    );
    let (s, e) = agent_step(state(LoopPhase::Planning, &[]), LoopInput::Asked(TurnId(7)));
    assert_eq!(
        (s.turn, e.contains(&LoopEffect::AskPlanner)),
        (Some(TurnId(7)), true)
    );
}

#[test]
fn a_full_run_asks_calls_reads_and_finishes() {
    let (s, e) = run(
        state(LoopPhase::Idle, &[]),
        vec![
            LoopInput::Asked(TurnId(1)),
            planned(1),
            LoopInput::CallEnded(CallId(0), done()),
            LoopInput::Planned(ModelOutput::Finish),
        ],
    );
    assert_eq!(s.phase, LoopPhase::Finished(FinishedAs::Done));
    assert_eq!(s.steps, 3);
    assert_eq!(
        e.iter().filter(|x| **x == LoopEffect::AskPlanner).count(),
        2
    );
}
