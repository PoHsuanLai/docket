//! The Companion1 bodies and the session records survive JSON; the forms sill reads are pinned.

use companion_wire::*;
use docket_core::*;
use porter_core::{AppName, Count, DataClass};
use prov::{AgentRef, Effect, SpaceId, TaskId};
use serde::{Serialize, de::DeserializeOwned};

fn round<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: &T) -> String {
    let json = serde_json::to_string(value).expect("json");
    let back: T = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{json}: {e}"));
    assert_eq!(&back, value, "{json}");
    json
}

fn task(s: &str) -> TaskId {
    TaskId::parse(s).expect("task")
}

fn footer() -> FooterWire {
    FooterWire {
        served: vec![],
        sources: vec![],
        keep: ContextKeep {
            query: Keep::Kept,
            results: Keep::Dropped,
            selection: Keep::Kept,
            window: Keep::Kept,
        },
    }
}

#[test]
fn every_phase_pins_its_json() {
    let cases = [
        (AnswerPhase::Thinking, r#"{"kind":"thinking"}"#),
        (AnswerPhase::Streaming, r#"{"kind":"streaming"}"#),
        (AnswerPhase::Done, r#"{"kind":"done"}"#),
        (AnswerPhase::Failed, r#"{"kind":"failed"}"#),
        (AnswerPhase::Cancelled, r#"{"kind":"cancelled"}"#),
        (
            AnswerPhase::NeedsYou(NeedsYou::Confirm(ConfirmId::parse("c-1").expect("id"))),
            r#"{"kind":"needs_you","v":{"kind":"confirm","v":"c-1"}}"#,
        ),
        (
            AnswerPhase::NeedsYou(NeedsYou::Question {
                text: "Which one?".into(),
                choices: vec!["a".into(), "b".into()],
            }),
            r#"{"kind":"needs_you","v":{"kind":"question","v":{"text":"Which one?","choices":["a","b"]}}}"#,
        ),
    ];
    for (phase, json) in cases {
        assert_eq!(round(&phase), json);
    }
}

#[test]
fn bodies_round_trip() {
    let app = AppName::parse("org.quire.Mail").expect("app");
    let action = ActionRef {
        app: app.clone(),
        name: prov::ActionName::parse("mail.thread.archive").expect("action"),
    };
    let call = CallRequest {
        action: action.clone(),
        target: TargetValue::Nothing,
        args: Args::new(),
        origin: Origin::Companion,
    };
    let card = CardWire {
        id: CardActionId::parse("send").expect("id"),
        label: LabelText::parse("Send").expect("label"),
        effect: Effect::Outbound,
        call,
    };
    let form = FormWire {
        action: action.clone(),
        params: vec![],
        values: Args::new(),
    };
    let bodies = vec![
        AnswerBody::Text {
            lines: vec![Reveal::Plain("done".into()), Reveal::Handle(Handle(3))],
        },
        AnswerBody::Plan(PlanWire {
            steps: vec![
                PlanStepWire {
                    id: StepId(1),
                    action: action.clone(),
                    label: LabelText::parse("Archive").expect("l"),
                    effect: Effect::UndoableWrite,
                    state: StepWireState::Done {
                        undo: Some(UndoId(4)),
                    },
                    call: Some(CallId(9)),
                },
                PlanStepWire {
                    id: StepId(2),
                    action,
                    label: LabelText::parse("Send").expect("l"),
                    effect: Effect::Outbound,
                    state: StepWireState::Failed(CallRefusal::Denied(DenyCode::NeedsUser)),
                    call: None,
                },
            ],
        }),
        AnswerBody::Replace {
            original: Reveal::Plain("a".into()),
            proposed: Reveal::Plain("b".into()),
            undo: None,
        },
        AnswerBody::Form(form.clone()),
        AnswerBody::Refused(RefusalWire::NeedsCloud(DataClass::Mail)),
        AnswerBody::Refused(RefusalWire::NoWay { app }),
        AnswerBody::Refused(RefusalWire::OverBudget(BudgetKind::Outbound)),
        AnswerBody::DraftReply {
            to: vec![],
            subject: Reveal::Handle(Handle(1)),
            body: Reveal::Handle(Handle(2)),
            actions: vec![card],
        },
        AnswerBody::ProposedEvent {
            title: "Lunch".into(),
            when: TimeRange {
                from: prov::UnixSeconds(1),
                to: prov::UnixSeconds(2),
            },
            people: vec![],
            actions: vec![],
        },
    ];
    for body in bodies {
        let answer = AnswerWire {
            task: task("t-1"),
            phase: AnswerPhase::Done,
            body,
            footer: footer(),
        };
        round(&answer);
    }
    round(&NeedsYou::Form(form));
}

#[test]
fn asks_and_the_front_pointer_round_trip() {
    let ask = AskWire {
        session: prov::SessionId::parse("s-1").expect("s"),
        turn: UserTurn {
            id: TurnId(4),
            text: "archive the newsletters".into(),
            at: prov::UnixSeconds(5),
            from: TurnSource::Launcher,
            via: TurnVia::Typed,
        },
        keep: ContextKeep {
            query: Keep::Kept,
            results: Keep::Dropped,
            selection: Keep::Dropped,
            window: Keep::Kept,
        },
        parent_window: WindowKey::parse("w-9").expect("w"),
        app: Some(AppName::parse("org.quire.Mail").expect("app")),
    };
    assert_eq!(
        round(&ask),
        concat!(
            r#"{"session":"s-1","turn":{"id":4,"text":"archive the newsletters","at":5,"#,
            r#""from":{"kind":"launcher"},"via":"typed"},"#,
            r#""keep":{"query":"kept","results":"dropped","selection":"dropped","window":"kept"},"#,
            r#""parent_window":"w-9","app":"org.quire.Mail"}"#
        )
    );
    let front = FrontTask {
        task: Some(task("t-2")),
        session: Some(prov::SessionId::parse("s-2").expect("s")),
    };
    assert_eq!(round(&front), r#"{"task":"t-2","session":"s-2"}"#);
    assert_eq!(
        round(&FrontTask {
            task: None,
            session: None
        }),
        r#"{"task":null,"session":null}"#
    );
}

fn skeleton() -> almanac_core::Skeleton {
    docket_core::skeleton_of(&TaskLedger {
        task: task("t-1"),
        agent: AgentRef::Companion,
        parent: None,
        space: SpaceId::parse("work").expect("space"),
        started: prov::UnixSeconds(0),
        asked: vec![],
        steps: vec![],
        touched: vec![],
        results: vec![],
    })
}

#[test]
fn session_records_carry_the_turn_verbatim_and_pin_their_tags() {
    let turn = UserTurn {
        id: TurnId(1),
        text: "archive the newsletters".into(),
        at: prov::UnixSeconds(5),
        from: TurnSource::Launcher,
        via: TurnVia::Typed,
    };
    let records = vec![
        (
            SessionRecord::Opened {
                task: task("t-1"),
                space: SpaceId::parse("work").expect("space"),
                agent: AgentRef::Companion,
                parent: None,
            },
            "opened",
        ),
        (
            SessionRecord::Asked {
                turn,
                to: AgentRef::Worker { task: task("t-9") },
                task: task("t-9"),
            },
            "asked",
        ),
        (
            SessionRecord::Replied {
                task: task("t-1"),
                phase: AnswerPhase::Done,
                digest: skeleton(),
            },
            "replied",
        ),
        (
            SessionRecord::Finished {
                task: task("t-1"),
                phase: AnswerPhase::Cancelled,
            },
            "finished",
        ),
        (SessionRecord::Closed, "closed"),
    ];
    for (record, slug) in records {
        let json = round(&record);
        assert!(json.starts_with(&format!(r#"{{"kind":"{slug}""#)), "{json}");
        assert_eq!(record.slug(), slug);
    }
    assert_eq!(SESSION_KIND_PREFIX, "companion.session");
}

#[test]
fn an_asked_record_holds_the_persons_words_and_the_target_agent() {
    let turn = UserTurn {
        id: TurnId(2),
        text: "skip the newsletter folder".into(),
        at: prov::UnixSeconds(9),
        from: TurnSource::Launcher,
        via: TurnVia::Spoken,
    };
    let record = SessionRecord::Asked {
        turn,
        to: AgentRef::Cua {
            run: prov::RunId::parse("r-3").expect("run"),
        },
        task: task("t-4"),
    };
    let json = round(&record);
    assert!(
        json.contains("skip the newsletter folder")
            && json.contains(r#""via":"spoken""#)
            && json.contains(r#""kind":"cua""#),
        "{json}"
    );
    let _ = Count(0);
}
