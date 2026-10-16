//! The person chose a coding agent as the assistant's model. inferd declines the planner's
//! request naming it, and the companion hands the turn to the agent's host instead of planning:
//! the agent's words are the answer, the footer names the agent, and a host that refuses ends the
//! turn the way a model that cannot answer does.

use crate::support::infer::{Say, words};
use crate::support::world::*;
use companion_wire::{AnswerBody, AnswerPhase, AnswerWire, RefusalWire, footer_line};
use docket_core::{HandOver, HandRefusal, HandReply, Handed, Reveal};
use docket_tasks::Handoff;
use porter_infer::{Declined, DeclinedBecause, InferEvent, InferRefusal, ModelRef};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

/// A host that answers what a test scripted and remembers what it was asked.
#[derive(Debug)]
struct Host {
    reply: HandReply,
    asked: Mutex<Vec<HandOver>>,
}

impl Host {
    fn answering(reply: HandReply) -> Arc<Self> {
        Arc::new(Self {
            reply,
            asked: Mutex::new(Vec::new()),
        })
    }

    fn asked(&self) -> Vec<HandOver> {
        self.asked.lock().expect("lock").clone()
    }
}

impl Handoff for Host {
    fn hand_over(&self, ask: HandOver) -> Pin<Box<dyn Future<Output = HandReply> + Send + '_>> {
        self.asked.lock().expect("lock").push(ask);
        let reply = self.reply.clone();
        Box::pin(async move { reply })
    }
}

/// What inferd sends when the person's pick for the assistant is the agent `claude-code`.
fn chose_the_agent() -> Say {
    let declined = Declined {
        model: ModelRef::agent("claude-code").expect("agent"),
        because: DeclinedBecause::AgentChosen,
    };
    Say::Routed(
        vec![InferEvent::Declined(declined)],
        Box::new(Say::Refuse(InferRefusal::Unavailable)),
    )
}

fn handed(text: &str) -> HandReply {
    HandReply::Handed(Handed {
        words: text.to_owned(),
        label: "Claude Code".to_owned(),
    })
}

#[tokio::test]
async fn the_agents_words_are_the_answer_and_the_footer_names_it() {
    let mut w = world(vec![chose_the_agent()]);
    let host = Host::answering(handed("The invoice is from Eve."));
    w.companion.set_handoff(host.clone());
    let opened = w.open("work").await;
    w.say(&opened.session, "what is the invoice about").await;

    let answer: AnswerWire = w.companion.shared.answer(&opened.task).expect("answer");
    assert_eq!(answer.phase, AnswerPhase::Done);
    assert_eq!(
        answer.body,
        AnswerBody::Text {
            lines: vec![Reveal::Plain("The invoice is from Eve.".to_owned())]
        }
    );
    let notes = w.companion.shared.route_of(&opened.task).expect("notes");
    assert_eq!(
        footer_line(&notes),
        "Answered by Claude Code (you chose it)"
    );
    // One hand-over, for the agent the person chose, carrying their own words.
    let asked = host.asked();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(asked[0].program.as_str(), "claude-code");
    assert_eq!(asked[0].turn.text, "what is the invoice about");
    assert_eq!(asked[0].space, space("work"));
}

#[tokio::test]
async fn a_host_that_refuses_ends_the_turn_in_words_the_person_can_act_on() {
    let rows = [
        (
            "switched off asks for a model",
            HandRefusal::TurnedOff,
            RefusalWire::NoModel,
        ),
        (
            "not set up asks for a model",
            HandRefusal::NotSetUp,
            RefusalWire::NoModel,
        ),
        (
            "a sign-in is named",
            HandRefusal::NeedsSignIn {
                label: "Claude Code".to_owned(),
            },
            RefusalWire::Failed("Sign in to Claude Code first.".to_owned()),
        ),
    ];
    for (name, refusal, shown) in rows {
        let mut w = world(vec![chose_the_agent()]);
        w.companion
            .set_handoff(Host::answering(HandReply::Refused(refusal)));
        let opened = w.open("work").await;
        w.say(&opened.session, "hello").await;
        let answer: AnswerWire = w.companion.shared.answer(&opened.task).expect("answer");
        assert_eq!(answer.phase, AnswerPhase::Failed, "{name}");
        assert_eq!(answer.body, AnswerBody::Refused(shown), "{name}");
    }
}

#[tokio::test]
async fn a_companion_with_no_host_cannot_serve_the_choice_and_asks_for_a_model() {
    let mut w = world(vec![chose_the_agent()]);
    let opened = w.open("work").await;
    w.say(&opened.session, "hello").await;
    let answer: AnswerWire = w.companion.shared.answer(&opened.task).expect("answer");
    assert_eq!(answer.body, AnswerBody::Refused(RefusalWire::NoModel));
}

#[tokio::test]
async fn a_model_the_person_chose_is_planned_with_and_the_host_is_never_asked() {
    let mut w = world(vec![words("Done.")]);
    let host = Host::answering(handed("never said"));
    w.companion.set_handoff(host.clone());
    let opened = w.open("work").await;
    w.say(&opened.session, "hello").await;
    assert!(host.asked().is_empty());
    let answer: AnswerWire = w.companion.shared.answer(&opened.task).expect("answer");
    assert_eq!(answer.phase, AnswerPhase::Done);
}
