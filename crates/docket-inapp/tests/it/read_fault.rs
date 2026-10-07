//! A model's mistake on `quire_read` is recoverable the way an unreadable reply is: a want that is
//! not a shape, an input that is not held, or a reader answer outside the shape is told to the
//! planner as a line of its history, and the turn goes on until the same bound as for unreadable
//! replies. Every model is scripted, the memory and the mail are in-process fakes, the clock is
//! virtual.

use crate::support::TestSheet;
use crate::support::host::{clock, parts, thread};
use crate::support::infer::{ScriptedInfer, call, words};
use docket_core::{ChoiceId, Handle, ReaderAsk, ReaderTask, ValueSchema};
use docket_inapp::{Ending, Failure, InAppAgent, InAppKit, TransportReader};
use serde_json::{Value, json};

const READ: &str = "org.quire.Mail-mail.thread.read";

fn good_ask() -> Value {
    let ask = ReaderAsk {
        inputs: vec![Handle(1)],
        want: ValueSchema::Choice(vec![
            ChoiceId::parse("invoice").expect("choice"),
            ChoiceId::parse("newsletter").expect("choice"),
        ]),
        task: ReaderTask::Classify,
    };
    serde_json::to_value(&ask).expect("ask")
}

fn bad_want() -> Value {
    json!({ "inputs": [1], "task": "classify", "want": { "handle": "the invoice thread" } })
}

fn open_thread() -> crate::support::infer::Say {
    call(READ, json!({ "target": thread("t1") }))
}

async fn turn(planner: &ScriptedInfer, reader: &ScriptedInfer) -> docket_inapp::Reply {
    let sheet = TestSheet::answering(vec![docket_inapp::SheetAnswer::Once; 2]);
    let mut agent = InAppAgent::with_kit(
        parts(planner, &sheet, &clock()),
        InAppKit::default().reader(TransportReader::in_process(reader.clone())),
    )
    .expect("agent");
    agent
        .ask("what kind of mail is the invoice")
        .await
        .expect("turn")
}

#[tokio::test]
async fn a_want_that_is_not_a_shape_is_told_and_the_second_try_is_answered() {
    let planner = ScriptedInfer::new(vec![
        open_thread(),
        call("quire_read", bad_want()),
        call("quire_read", good_ask()),
        words("It is an invoice."),
    ]);
    let reader = ScriptedInfer::new(vec![words("invoice")]);
    let reply = turn(&planner, &reader).await;
    assert_eq!(reply.ending, Ending::Done, "{reply:?}");
    assert_eq!(
        reader.asked().len(),
        1,
        "the bad ask never reached the reader"
    );
    let after_fault = planner.user_text(2);
    assert!(
        after_fault.contains("\"want\" is not a shape of the answer"),
        "{after_fault}"
    );
    assert!(
        after_fault.contains("\"kind\": \"choice\""),
        "{after_fault}"
    );
    assert!(
        planner.user_text(3).contains("invoice"),
        "the answer is shown after the retry"
    );
}

#[tokio::test]
async fn an_input_the_session_does_not_hold_is_told_to_the_planner() {
    let unheld = json!({ "inputs": [77], "task": "classify", "want": good_ask()["want"] });
    let planner = ScriptedInfer::new(vec![
        open_thread(),
        call("quire_read", unheld),
        call("quire_read", good_ask()),
        words("It is an invoice."),
    ]);
    let reader = ScriptedInfer::new(vec![words("invoice")]);
    let reply = turn(&planner, &reader).await;
    assert_eq!(reply.ending, Ending::Done, "{reply:?}");
    assert!(
        planner.user_text(2).contains("not a handle you were shown"),
        "{}",
        planner.user_text(2)
    );
}

#[tokio::test]
async fn a_reader_answer_outside_the_shape_is_told_to_the_planner() {
    let planner = ScriptedInfer::new(vec![
        open_thread(),
        call("quire_read", good_ask()),
        call("quire_read", good_ask()),
        words("It is an invoice."),
    ]);
    let reader = ScriptedInfer::new(vec![words("a memo"), words("invoice")]);
    let reply = turn(&planner, &reader).await;
    assert_eq!(reply.ending, Ending::Done, "{reply:?}");
    assert_eq!(reader.asked().len(), 2);
    assert!(
        planner.user_text(2).contains("quire_read gave no answer"),
        "{}",
        planner.user_text(2)
    );
}

#[tokio::test]
async fn three_bad_reads_running_stop_the_turn_and_ask_the_person() {
    let planner = ScriptedInfer::new(vec![
        open_thread(),
        call("quire_read", bad_want()),
        call("quire_read", bad_want()),
        call("quire_read", bad_want()),
        words("never reached"),
    ]);
    let reply = turn(&planner, &ScriptedInfer::new(vec![])).await;
    assert!(
        matches!(reply.ending, Ending::Asked { .. }),
        "asks, as for unreadable replies: {reply:?}"
    );
    assert_eq!(
        planner.asked().len(),
        4,
        "the planner was not asked a fifth time"
    );
}

#[tokio::test]
async fn a_reader_that_is_not_there_still_fails_the_turn_with_its_cause() {
    let planner = ScriptedInfer::new(vec![open_thread(), call("quire_read", good_ask())]);
    let sheet = TestSheet::answering(vec![docket_inapp::SheetAnswer::Once; 2]);
    let mut agent = InAppAgent::new(parts(&planner, &sheet, &clock())).expect("agent");
    let reply = agent
        .ask("what kind of mail is the invoice")
        .await
        .expect("turn");
    assert_eq!(reply.ending, Ending::Failed(Failure::Reader), "{reply:?}");
}
