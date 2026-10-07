//! The repeat guard through the loop machine: a stale repeat is held back with a note, the next
//! one ends the turn by asking the person, and a changed call is never held.

use agent_loop::*;
use companion_wire::{AnswerPhase, NeedsYou};
use docket_core::*;
use porter_core::AppName;
use prov::{ActionName, Confidentiality, Integrity, Label};
use std::collections::{BTreeMap, BTreeSet};

fn idle() -> LoopState {
    LoopState {
        phase: LoopPhase::Idle,
        turn: None,
        steps: 0,
        pending: vec![],
        guard: Guard::default(),
    }
}

fn label() -> Label {
    Label {
        integrity: Integrity::Untrusted,
        confidentiality: Confidentiality::Public,
        classes: BTreeSet::new(),
        sources: BTreeSet::new(),
    }
}

fn call(action: &str, query: &str) -> PlannedCall {
    let mut args = BTreeMap::new();
    args.insert(
        ParamName::parse("query").expect("param"),
        prov::Labelled::new(Value::Text(query.to_owned()), label()),
    );
    PlannedCall {
        call: CallRequest {
            action: ActionRef {
                app: AppName::parse("org.quire.Mail").expect("app"),
                name: ActionName::parse(action).expect("action"),
            },
            target: TargetValue::Nothing,
            args,
            origin: Origin::Companion,
        },
        tier: Tier::Typed,
    }
}

fn plan(calls: Vec<PlannedCall>) -> LoopInput {
    LoopInput::Planned(ModelOutput::Calls(calls))
}

fn nothing() -> StepEnd {
    StepEnd::Done {
        said: None,
        value: Some(Reveal::Plain(Value::Entities(vec![]))),
        undo: None,
    }
}

fn text(t: &str) -> StepEnd {
    StepEnd::Done {
        said: None,
        value: Some(Reveal::Plain(Value::Text(t.to_owned()))),
        undo: None,
    }
}

fn asked() -> LoopState {
    agent_step(idle(), LoopInput::Asked(TurnId(1))).0
}

/// Plans `calls`, and ends each call that went out with `end`.
fn round(s: LoopState, calls: Vec<PlannedCall>, end: &StepEnd) -> (LoopState, Vec<LoopEffect>) {
    let (mut s, mut effects) = agent_step(s, plan(calls));
    let out = s.pending.clone();
    for id in out {
        let (next, more) = agent_step(s, LoopInput::CallEnded(id, end.clone()));
        s = next;
        effects.extend(more);
    }
    (s, effects)
}

fn calls_in(effects: &[LoopEffect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, LoopEffect::Call(_)))
        .count()
}

fn holds_in(effects: &[LoopEffect]) -> Vec<Held> {
    effects
        .iter()
        .filter_map(|e| match e {
            LoopEffect::Held(_, why) => Some(*why),
            _ => None,
        })
        .collect()
}

fn asks_person(effects: &[LoopEffect]) -> Option<&str> {
    effects.iter().find_map(|e| match e {
        LoopEffect::Publish(AnswerPhase::NeedsYou(NeedsYou::Question { text, .. })) => {
            Some(text.as_str())
        }
        _ => None,
    })
}

#[test]
fn the_second_identical_call_after_nothing_is_held_and_the_third_asks_the_person() {
    let search = || vec![call("mail.thread.search", "Lisbon")];
    let (s, e) = round(asked(), search(), &nothing());
    assert_eq!(calls_in(&e), 1);
    assert_eq!(s.phase, LoopPhase::Planning);

    let (s, e) = agent_step(s, plan(search()));
    assert_eq!(calls_in(&e), 0);
    assert_eq!(holds_in(&e), [Held::Empty]);
    assert!(e.contains(&LoopEffect::AskPlanner));
    assert_eq!(s.phase, LoopPhase::Planning);

    let (s, e) = agent_step(s, plan(search()));
    assert_eq!(s.phase, LoopPhase::Idle, "asking, not finished");
    assert_eq!(calls_in(&e), 0);
    let question = asks_person(&e).expect("a question for the person");
    assert!(question.contains("couldn't find anything with mail.thread.search"));
    assert!(!e.contains(&LoopEffect::AskPlanner));
}

#[test]
fn a_call_with_other_arguments_is_not_held() {
    let (s, _) = round(
        asked(),
        vec![call("mail.thread.search", "Lisbon")],
        &nothing(),
    );
    let (s, e) = agent_step(s, plan(vec![call("mail.thread.search", "Porto")]));
    assert_eq!((calls_in(&e), holds_in(&e).len()), (1, 0));
    assert_eq!(s.phase, LoopPhase::AwaitingCalls);
}

#[test]
fn a_repeat_of_a_call_that_found_something_runs_until_the_answer_stops_changing() {
    let search = || vec![call("mail.thread.search", "Lisbon")];
    let (s, _) = round(asked(), search(), &text("a"));
    let (s, e) = round(s, search(), &text("a"));
    assert_eq!(calls_in(&e), 1, "once is not a pattern");
    let (_, e) = agent_step(s, plan(search()));
    assert_eq!(holds_in(&e), [Held::Unchanged]);
}

#[test]
fn a_repeat_whose_answer_changed_is_not_stale() {
    let search = || vec![call("mail.thread.search", "Lisbon")];
    let (s, _) = round(asked(), search(), &text("a"));
    let (s, _) = round(s, search(), &text("b"));
    let (_, e) = agent_step(s, plan(search()));
    assert_eq!((calls_in(&e), holds_in(&e).len()), (1, 0));
}

#[test]
fn a_handle_answer_cannot_be_compared_so_it_is_never_stale() {
    let handle = StepEnd::Done {
        said: None,
        value: Some(Reveal::Handle(Handle(3))),
        undo: None,
    };
    let search = || vec![call("mail.thread.read", "x")];
    let (s, _) = round(asked(), search(), &handle);
    let (s, _) = round(s, search(), &handle);
    let (_, e) = agent_step(s, plan(search()));
    assert_eq!(calls_in(&e), 1);
}

#[test]
fn alternating_two_empty_searches_ends_on_the_fifth_call() {
    let a = || vec![call("mail.thread.search", "Lisbon")];
    let b = || vec![call("mail.contact.search", "Lisbon")];
    let (s, _) = round(asked(), a(), &nothing());
    let (s, _) = round(s, b(), &nothing());
    let (s, e) = agent_step(s, plan(a()));
    assert_eq!(holds_in(&e), [Held::Empty]);
    let (s, e) = agent_step(s, plan(b()));
    assert_eq!(holds_in(&e), [Held::Empty]);
    let (s, e) = agent_step(s, plan(a()));
    assert!(asks_person(&e).is_some());
    assert_eq!(s.phase, LoopPhase::Idle);
}

#[test]
fn many_different_stale_calls_stop_the_turn_by_the_turn_cap() {
    let calls: Vec<PlannedCall> = (0..=MOST_HOLDS_PER_TURN)
        .map(|n| call("mail.thread.search", &format!("q{n}")))
        .collect();
    let mut s = asked();
    for c in &calls {
        s = round(s, vec![c.clone()], &nothing()).0;
    }
    for c in &calls[..usize::from(MOST_HOLDS_PER_TURN)] {
        let (next, e) = agent_step(s, plan(vec![c.clone()]));
        assert_eq!(holds_in(&e).len(), 1);
        s = next;
    }
    let (s, e) = agent_step(
        s,
        plan(vec![calls[usize::from(MOST_HOLDS_PER_TURN)].clone()]),
    );
    assert_eq!(s.phase, LoopPhase::Idle);
    assert!(
        asks_person(&e)
            .expect("asks")
            .contains("going round in circles")
    );
}

#[test]
fn a_call_refused_for_its_arguments_is_held_when_made_again() {
    let refused = StepEnd::Refused(CallRefusal::BadArgs {
        param: ParamName::parse("query").expect("param"),
        why: ArgFault::UnknownHandle,
    });
    let go = || vec![call("mail.message.forward", "x")];
    let (s, _) = round(asked(), go(), &refused);
    let (_, e) = agent_step(s, plan(go()));
    assert_eq!(holds_in(&e), [Held::Refused]);
}

#[test]
fn a_transient_refusal_is_not_held() {
    let go = || vec![call("mail.thread.search", "x")];
    let (s, _) = round(asked(), go(), &StepEnd::Refused(CallRefusal::Timeout));
    let (_, e) = agent_step(s, plan(go()));
    assert_eq!(calls_in(&e), 1);
}

#[test]
fn in_a_batch_the_stale_call_is_held_and_the_new_one_goes_out() {
    let (s, _) = round(
        asked(),
        vec![call("mail.thread.search", "Lisbon")],
        &nothing(),
    );
    let batch = vec![
        call("mail.thread.search", "Lisbon"),
        call("mail.thread.search", "Porto"),
    ];
    let (s, e) = agent_step(s, plan(batch));
    assert_eq!(holds_in(&e), [Held::Empty]);
    assert_eq!(calls_in(&e), 1);
    assert_eq!(
        (s.phase, s.pending),
        (LoopPhase::AwaitingCalls, vec![CallId(0)])
    );
}

#[test]
fn a_new_ask_forgets_the_turns_ledger() {
    let search = || vec![call("mail.thread.search", "Lisbon")];
    let (s, _) = round(asked(), search(), &nothing());
    let (s, _) = agent_step(s, LoopInput::Asked(TurnId(2)));
    let (_, e) = agent_step(s, plan(search()));
    assert_eq!(calls_in(&e), 1);
}

fn unread() -> LoopInput {
    LoopInput::Planned(ModelOutput::Unread(ReplyFault::NotJson))
}

fn unread_lines(effects: &[LoopEffect]) -> usize {
    effects
        .iter()
        .filter(|e| matches!(e, LoopEffect::Unread(_)))
        .count()
}

#[test]
fn an_unreadable_reply_is_told_and_the_planner_asked_again() {
    let (s, e) = agent_step(asked(), unread());
    assert_eq!(s.phase, LoopPhase::Planning);
    assert_eq!(unread_lines(&e), 1);
    assert!(e.contains(&LoopEffect::AskPlanner));
    assert_eq!(asks_person(&e), None);
}

#[test]
fn the_third_unreadable_reply_running_asks_the_person_instead() {
    let (s, _) = agent_step(asked(), unread());
    let (s, e) = agent_step(s, unread());
    assert_eq!((unread_lines(&e), s.phase), (1, LoopPhase::Planning));
    let (s, e) = agent_step(s, unread());
    assert_eq!(s.phase, LoopPhase::Idle, "asking, not failed");
    assert_eq!(unread_lines(&e), 1);
    assert!(!e.contains(&LoopEffect::AskPlanner));
    let question = asks_person(&e).expect("a question for the person");
    assert!(
        question.contains("How would you like me to go on?"),
        "{question}"
    );
}

#[test]
fn a_readable_call_between_unreadable_replies_starts_the_count_again() {
    let (s, _) = agent_step(asked(), unread());
    let (s, _) = agent_step(s, unread());
    let (s, _) = round(s, vec![call("mail.thread.search", "Lisbon")], &text("x"));
    let (s, e) = agent_step(s, unread());
    assert_eq!((unread_lines(&e), s.phase), (1, LoopPhase::Planning));
    let (s, e) = agent_step(s, unread());
    assert_eq!((unread_lines(&e), s.phase), (1, LoopPhase::Planning));
}

#[test]
fn a_search_that_names_the_same_handles_again_is_unchanged_and_held() {
    let found = StepEnd::Done {
        said: Some(LabelText::parse("Found threads").expect("text")),
        value: Some(Reveal::Plain(Value::List(vec![
            Value::Handle(Handle(1)),
            Value::Handle(Handle(2)),
        ]))),
        undo: None,
    };
    let search = || vec![call("mail.thread.search", "Lisbon")];
    let (s, _) = round(asked(), search(), &found);
    let (s, _) = round(s, search(), &found);
    let (_, e) = agent_step(s, plan(search()));
    assert_eq!(holds_in(&e), [Held::Unchanged]);
}
