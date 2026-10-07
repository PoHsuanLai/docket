//! Calls, confirmations, audit records, messages and the planner's view survive JSON.

mod support;

use docket_core::*;
use porter_core::Count;
use prov::{Address, AgentRef, ConfirmId, Effect, MessageKind, MessageText, ReportStatus, TaskId};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::BTreeSet;
use support::*;

fn round<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: &T) -> String {
    let json = serde_json::to_string(value).expect("json");
    let back: T = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{json}: {e}"));
    assert_eq!(&back, value, "{json}");
    json
}

#[test]
fn calls_confirmations_and_audit_round_trip() {
    let mut args = Args::new();
    args.insert(
        param("to"),
        lab(Value::Entity(entity("mail.contact", "c")), trusted()),
    );
    args.insert(
        param("body"),
        lab(Value::Handle(Handle(2)), untrusted_mail("work")),
    );
    let call = CallRequest {
        action: ActionRef {
            app: app("org.quire.Mail"),
            name: action("mail.message.send"),
        },
        target: TargetValue::Entities(vec![entity("mail.thread", "t")]),
        args,
        origin: Origin::Companion,
    };
    round(&call);
    let confirm = ConfirmRequest {
        id: ConfirmId::parse("c-1").expect("id"),
        space: space("work"),
        actor: planner(),
        app: app("org.quire.Mail"),
        action: words("Send 1 message"),
        effect: Effect::Outbound,
        count: Count(1),
        detail: ConfirmDetail::Recipients(vec![Shown::Quoted {
            text: "x@evil.test".into(),
            from: prov::Source::Mail,
        }]),
        lines: vec![ArgLine {
            label: words("To"),
            value: Shown::Plain("Accounting".into()),
        }],
        why: vec![AskReason::Tainted, AskReason::RuleOfTwo],
        taint: TaintNote::ReadUntrusted(BTreeSet::from([prov::Source::Mail])),
        offer: ConfirmOffer::OnceOnly,
        gesture: Gesture::Press,
        anchor: Anchor::Launcher,
        expires: Seconds(120),
    };
    round(&confirm);
    for record in [
        AuditRecord::Breaker {
            at: at(5),
            session: prov::SessionId::parse("s-1").expect("s"),
            trip: BreakerTrip::Consecutive,
        },
        AuditRecord::Halt {
            at: at(6),
            scope: prov::SpaceScope::Any,
            cause: HaltCause::KillChord,
        },
        AuditRecord::Call {
            at: at(7),
            call: CallId(3),
            actor: planner(),
            action: ActionRef {
                app: app("org.quire.Mail"),
                name: action("mail.thread.archive"),
            },
            targets: vec![entity("mail.thread", "t")],
            effect: Effect::UndoableWrite,
            space: space("work"),
            decided: DecidedBy::Policy(vec![PolicyId("grid".into())]),
            end: CallEnd::Done,
        },
    ] {
        round(&record);
    }
}

#[test]
fn the_audit_record_holds_no_content() {
    let record = AuditRecord::Review {
        at: at(1),
        call: CallId(1),
        mark: ReviewMark {
            stage: Stage::Quick,
            verdict: Ok(VerdictKind::Ask),
            code: ReasonCode::Uncertain,
            latency: Millis(120),
            model: porter_core::ModelId::parse("holo").expect("model"),
        },
    };
    let json = round(&record);
    assert!(
        json.contains(r#""code":"uncertain""#) && !json.contains("text"),
        "{json}"
    );
}

#[test]
fn messages_use_the_one_prov_model() {
    let draft = MessageDraft {
        to: Address::new(
            AgentRef::Worker {
                task: TaskId::parse("t-9").expect("task"),
            },
            space("home"),
        ),
        thread: None,
        in_reply_to: None,
        kind: MessageKind::Request,
        parts: vec![
            DraftPart::Text(MessageText::new("skip the newsletter folder")),
            DraftPart::Handle(Handle(4)),
        ],
    };
    let json = round(&draft);
    assert!(json.contains(r#""kind":"request""#), "{json}");
    for status in [
        ReportStatus::Done,
        ReportStatus::Failed,
        ReportStatus::Cancelled,
        ReportStatus::Progress,
    ] {
        round(&MessageKind::Report { status });
    }
    round(&SendRefusal::Malformed(prov::Fault::NoParts));
    round(&Delivery {
        message: prov::MessageId::parse("m-1").expect("m"),
        thread: prov::ThreadId::parse("m-1").expect("t"),
        crossing: prov::Crossing::Across,
    });
    round(&InboxAsk {
        agent: AgentRef::Companion,
        after: None,
    });
}

#[test]
fn the_planner_view_and_the_roster_round_trip() {
    let view = PlannerView {
        turns: vec![UserTurn {
            id: TurnId(1),
            text: "summarise this".into(),
            at: at(1),
            from: TurnSource::Field(app("org.quire.Mail")),
            via: TurnVia::Spoken,
        }],
        context: ContextView {
            app: app("org.quire.Mail"),
            window: Reveal::Handle(Handle(1)),
            here: HereView::Nowhere,
            selection: SelectionView::Nothing,
            visible: VisibleView {
                kind: None,
                items: vec![],
                total: Count(0),
            },
            text_target: TextTargetView::None,
        },
        actions: vec![],
        handles: vec![HandleCard {
            handle: Handle(1),
            shape: HandleShape::Text,
            from: prov::Source::Mail,
            size: CharCount(40),
        }],
        history: vec![StepLine {
            call: CallId(1),
            action: ActionRef {
                app: app("org.quire.Mail"),
                name: action("mail.thread.read"),
            },
            effect: Effect::Read,
            end: StepEnd::Refused(CallRefusal::Denied(DenyCode::NotAllowed)),
            shown: StepShown::Masked,
            with: vec![Handle(1)],
        }],
        taint: prov::Integrity::Untrusted,
        task_policy: None,
        primer: Some(PrimerText("likes short answers".into())),
        profile: vec![ProfileLine("name: Po".into())],
        rollup: None,
        roster: Roster {
            entries: vec![RosterLine {
                agent: AgentRef::User,
                space: space("work"),
                state: RosterState::Working,
                detail: RosterDetail::PresenceOnly,
            }],
        },
        episodes: vec![],
        recalled: vec![],
        inbox: vec![],
        skills: vec![],
        skill_texts: vec![],
    };
    let json = round(&view);
    // The view holds a handle where the window title was, and the history codes only.
    assert!(
        json.contains(r#""window":{"kind":"handle","v":1}"#),
        "{json}"
    );
    assert!(
        json.contains(r#""kind":"denied","v":"not_allowed""#),
        "{json}"
    );
}

#[test]
fn the_cua_ask_carries_an_untrusted_screen_label() {
    use cua_action::{CuaAction, WindowSpace};
    let ask = CuaAsk {
        run: prov::RunId::parse("r-1").expect("r"),
        step: 4,
        app: app("org.mozilla.Firefox"),
        trust: WindowTrust::Flatpak,
        mode: RunMode::InPlace,
        space: space("work"),
        action: CuaAction::<WindowSpace>::Observe,
        node: None,
        effect: Effect::Read,
        basis: EffectBasis::DefaultTable,
        screen: untrusted_mail("work"),
    };
    round(&ask);
}

#[test]
fn the_terminal_words_are_pinned() {
    assert_eq!(round(&Origin::Cli), r#""cli""#);
    assert_eq!(round(&CallerRole::Cli), r#""cli""#);
    assert_eq!(round(&GrantCaller::Cli), r#"{"kind":"cli"}"#);
    assert_eq!(GrantCaller::Cli.kind(), prov::ActorKind::Cli);
    assert_eq!(
        round(&AskReason::FromTerminal),
        r#"{"kind":"from_terminal"}"#
    );
    assert_eq!(
        round(&ConfirmOffer::OnceOrFromTerminal),
        r#""once_or_from_terminal""#
    );
    assert_eq!(
        round(&ConfirmAnswerKind::AllowedFromTerminal),
        r#"{"kind":"allowed_from_terminal"}"#
    );
    let receipt = prov::ConfirmReceipt {
        id: ConfirmId::parse("c-1").expect("id"),
        input: prov::InputProof::HardwareSeat,
        at: at(1),
        covers: prov::Confidentiality::Secret,
    };
    let answer = ConfirmAnswer::AllowedFromTerminal {
        receipt: receipt.clone(),
    };
    assert!(round(&answer).starts_with(r#"{"kind":"allowed_from_terminal""#));
    // A sheet that never offered the grant (a computer-use step, a widened policy) cannot be
    // answered with it: it reads as the router withdrawing the sheet.
    assert_eq!(
        answer.without_terminal_grant(),
        ConfirmAnswer::Ended(ConfirmEnd::Cancelled)
    );
    let plain = ConfirmAnswer::Allowed {
        scope: GrantScope::Once,
        receipt,
    };
    assert_eq!(plain.clone().without_terminal_grant(), plain);
}

#[test]
fn a_dry_run_is_a_member_of_its_own() {
    let request = IntentsRequest::DryRun {
        call: CallRequest {
            action: ActionRef {
                app: app("org.quire.Mail"),
                name: action("mail.thread.archive"),
            },
            target: TargetValue::Nothing,
            args: Args::new(),
            origin: Origin::Cli,
        },
        session: None,
    };
    assert_eq!(request.member(), Member::DryRun);
    round(&request);
    assert_eq!(
        IntentsRequest::ControlTerminalGrants.member(),
        Member::ControlTerminalGrants
    );
    round(&IntentsReply::TerminalGrants(vec![ActionRef {
        app: app("org.quire.Mail"),
        name: action("mail.thread.archive"),
    }]));
}
