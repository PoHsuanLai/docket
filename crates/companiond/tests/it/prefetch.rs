//! Memory comes to the planner: on a turn the person typed, the companion searches memory for
//! their words before its first planner call, through the router, with no sheet; a worker's
//! turn, a memory that is not there and the recall switch leave the turn as it was.

use crate::support::infer::{Say, call, words};
use crate::support::world::*;
use companion_wire::AnswerBody;
use docket_core::*;
use docket_fake::{Answering, BoxFut, HostedApp};
use prov::{Actor, Labelled};
use serde_json::json;
use std::sync::{Arc, Mutex};

/// Memory as the router's built-in provider answers a search: one fact, by entity.
#[derive(Debug, Default)]
struct Recall(Mutex<Vec<String>>);

impl Recall {
    fn asked(&self) -> Vec<String> {
        self.0.lock().expect("log").clone()
    }
}

impl HostedApp for Recall {
    fn perform(&self, inv: Invocation) -> BoxFut<Result<Outcome, AppRefusal>> {
        let query = inv.args.values().find_map(|a| match &a.value {
            Value::Text(t) => Some(t.clone()),
            _ => None,
        });
        if let (Ok(mut log), Some(query)) = (self.0.lock(), query) {
            log.push(query);
        }
        Box::pin(async {
            Ok(Outcome {
                value: Some(Labelled {
                    value: Value::Entities(vec![prov::EntityId {
                        app: app("org.quire.Memory"),
                        kind: prov::EntityKind::parse("memory.fact").expect("kind"),
                        key: prov::EntityKey::parse("f1").expect("key"),
                    }]),
                    label: prov::Label::trusted_user(),
                }),
                said: LabelText::parse("Found 1 thing").ok(),
                show: Preview::None,
                undo: Undoable::No,
                follow: Follow::Nothing,
            })
        })
    }
}

fn with_memory(script: Vec<Say>) -> (World, Arc<Recall>) {
    let w = world(script);
    let memory = Arc::new(Recall::default());
    w.router
        .seams
        .link
        .host(app("org.quire.Memory"), memory.clone());
    (w, memory)
}

fn recalls(w: &World) -> usize {
    w.records()
        .iter()
        .filter(|r| {
            matches!(
                r,
                AuditRecord::Call { actor: Actor::Companion { .. }, action, end: CallEnd::Done, .. }
                    if action.name.as_str() == "memory.recall"
            )
        })
        .count()
}

#[tokio::test]
async fn the_persons_words_are_searched_before_the_first_planner_call_with_no_sheet() {
    let (mut w, memory) = with_memory(vec![words("Taipei.")]);
    let opened = w.open("work").await;
    w.say(&opened.session, "What is the capital of Taiwan?")
        .await;

    assert_eq!(memory.asked(), ["What is the capital of Taiwan?"]);
    assert!(
        w.infer.user_text(0).contains("Recall done"),
        "the first planner call already holds the result: {}",
        w.infer.user_text(0)
    );
    assert!(w.router.seams.confirmer.requests().is_empty(), "no sheet");
    assert_eq!(recalls(&w), 1, "one call, audited as the companion's");
    let AnswerBody::Text { lines } = w
        .companion
        .shared
        .answer(&opened.task)
        .expect("answer")
        .body
    else {
        panic!("text")
    };
    assert_eq!(lines, [Reveal::Plain("Taipei.".into())]);
}

#[tokio::test]
async fn a_worker_a_missing_memory_and_the_recall_switch_leave_the_turn_as_it_was() {
    // A worker's turns are not the person's: one search, for the person's turn.
    let (mut w, memory) = with_memory(vec![
        call(
            "org_quire_Companion-companion_task_start",
            json!({ "goal": "tidy the downloads" }),
        ),
        words("on it"),
        words("done"),
    ]);
    let front = w.open("work").await;
    w.say(&front.session, "get someone to tidy up").await;
    assert_eq!(memory.asked(), ["get someone to tidy up"]);

    // Memory is not there: the turn goes on without it.
    let (mut w, memory) = with_memory(vec![words("Taipei.")]);
    w.router
        .seams
        .link
        .answer_from(&app("org.quire.Memory"), Answering::Absent);
    let opened = w.open("work").await;
    w.say(&opened.session, "What is the capital of Taiwan?")
        .await;
    assert!(memory.asked().is_empty());
    assert!(!w.infer.user_text(0).contains("memory.recall"));
    assert_eq!(
        w.companion
            .shared
            .answer(&opened.task)
            .expect("answer")
            .phase,
        companion_wire::AnswerPhase::Done
    );

    // No recall budget: no search.
    let (mut w, memory) = with_memory(vec![words("Taipei.")]);
    w.companion.config.assembler.recall = porter_core::Tokens(0);
    let opened = w.open("work").await;
    w.say(&opened.session, "What is the capital of Taiwan?")
        .await;
    assert!(memory.asked().is_empty());
}
