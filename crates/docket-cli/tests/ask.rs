//! `quire-do ask` over the fake router and a scripted companion: the turn is recorded by the
//! router as the terminal's, the companion is asked with it, the answer is printed (text, plan
//! steps, footer), a confirmation is never answered here, and the exit code says how it ended.

mod support;

use companion_wire::{
    AnswerBody, AnswerPhase, AnswerWire, AskWire, FooterWire, NeedsYou, PlanStepWire, PlanWire,
    RefusalWire, StepWireState,
};
use docket_cli::{Exit, JsonFlag, Stdout, ask_companion};
use docket_client::{CompanionTransport, Follow, InProcess, Intents, TransportError};
use docket_core::{
    ActionRef, CallerRole, ConfirmId, ContextKeep, Keep, LabelText, Reveal, SessionOpen,
    SessionOpened, StepId, TurnSource,
};
use docket_fake::FakeSeams;
use porter_core::{AccountId, DataClass, Locality, ModelId};
use porter_infer::ServedBy;
use prov::{ActionName, Effect, SessionId, UnixSeconds};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use support::{Desk, caller};

/// What the scripted companion saw.
#[derive(Default)]
struct Seen {
    asked: Vec<AskWire>,
    closed: Vec<SessionId>,
}

struct Views(VecDeque<AnswerWire>);

impl Follow for Views {
    async fn next(&mut self) -> Result<Option<AnswerWire>, TransportError> {
        Ok(self.0.pop_front())
    }
}

/// A companion that opens real sessions in the router (as companiond does, in its own role) and
/// answers every ask with the same views.
struct Scripted {
    opener: Intents<InProcess<FakeSeams>>,
    views: Vec<AnswerWire>,
    seen: Arc<Mutex<Seen>>,
    down: bool,
}

impl CompanionTransport for Scripted {
    type Answer = Views;

    async fn open(&self, open: &SessionOpen) -> Result<SessionOpened, TransportError> {
        if self.down {
            return Err(TransportError::Closed);
        }
        self.opener
            .session_open(open.clone())
            .await
            .map_err(|e| TransportError::Bus(e.to_string()))
    }

    async fn ask(&self, ask: &AskWire) -> Result<Views, TransportError> {
        self.seen.lock().expect("lock").asked.push(ask.clone());
        Ok(Views(self.views.iter().cloned().collect()))
    }

    async fn close(&self, session: &SessionId) -> Result<(), TransportError> {
        self.seen.lock().expect("lock").closed.push(session.clone());
        Ok(())
    }
}

fn keep() -> ContextKeep {
    ContextKeep {
        query: Keep::Dropped,
        results: Keep::Dropped,
        selection: Keep::Dropped,
        window: Keep::Dropped,
    }
}

fn footer() -> FooterWire {
    FooterWire {
        served: vec![ServedBy {
            account: AccountId::parse("openrouter").expect("account"),
            model: ModelId::parse("some-model").expect("model"),
            locality: Locality::Cloud { region: None },
        }],
        sources: vec![],
        keep: keep(),
    }
}

fn view(phase: AnswerPhase, body: AnswerBody) -> AnswerWire {
    AnswerWire {
        task: prov::TaskId::parse("t-1").expect("task"),
        phase,
        body,
        footer: footer(),
    }
}

fn text(lines: &[&str]) -> AnswerBody {
    AnswerBody::Text {
        lines: lines
            .iter()
            .map(|l| Reveal::Plain((*l).to_owned()))
            .collect(),
    }
}

fn plan() -> AnswerBody {
    AnswerBody::Plan(PlanWire {
        steps: vec![PlanStepWire {
            id: StepId(1),
            action: ActionRef {
                app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
                name: ActionName::parse("mail.thread.archive").expect("action"),
            },
            label: LabelText::parse("Archive the digest").expect("label"),
            effect: Effect::UndoableWrite,
            state: StepWireState::Pending,
            call: None,
        }],
    })
}

struct Run {
    report: docket_cli::Report,
    seen: Arc<Mutex<Seen>>,
    desk: Desk,
}

async fn ask(views: Vec<AnswerWire>, down: bool, stdout: Stdout, json: JsonFlag) -> Run {
    let desk = Desk::new();
    let seen = Arc::new(Mutex::new(Seen::default()));
    let companion = Scripted {
        opener: Intents::over(InProcess::new(
            desk.router.clone(),
            caller("org.quire.Companion1", CallerRole::Companion),
        )),
        views,
        seen: seen.clone(),
        down,
    };
    let terminal = Intents::over(InProcess::new(
        desk.router.clone(),
        caller("org.quire.Do", CallerRole::Cli),
    ));
    let report = ask_companion(
        &terminal,
        &companion,
        "what is on my calendar",
        prov::SpaceId::desktop(),
        UnixSeconds(1_000),
        stdout,
        json,
    )
    .await;
    Run { report, seen, desk }
}

#[tokio::test]
async fn an_answer_prints_its_text_the_plan_and_what_it_was_made_of_and_the_session_closes() {
    let views = vec![
        view(AnswerPhase::Thinking, text(&[])),
        view(
            AnswerPhase::Done,
            text(&["Nothing today.", "Tomorrow: dentist."]),
        ),
    ];
    let run = ask(views, false, Stdout::Tty, JsonFlag::Auto).await;
    let out = &run.report.stdout;
    assert_eq!(run.report.exit, Exit::Done, "{:?}", run.report);
    assert!(out.contains("Nothing today.\nTomorrow: dentist."), "{out}");
    assert!(
        out.contains("answered by openrouter/some-model in the cloud"),
        "{out}"
    );
    assert!(!out.contains("Waiting"), "{out}");
    let seen = run.seen.lock().expect("lock");
    assert_eq!(seen.asked.len(), 1);
    let ask = &seen.asked[0];
    assert_eq!(ask.turn.text, "what is on my calendar");
    assert_eq!(ask.turn.from, TurnSource::Terminal);
    assert_eq!(ask.turn.at, UnixSeconds(1_000));
    assert_eq!(seen.closed, vec![ask.session.clone()]);
    assert_eq!(run.desk.sheets(), 0);
}

#[tokio::test]
async fn a_plan_is_printed_step_by_step() {
    let run = ask(
        vec![view(AnswerPhase::Done, plan())],
        false,
        Stdout::Tty,
        JsonFlag::Auto,
    )
    .await;
    assert!(
        run.report
            .stdout
            .contains("Plan\n  1. pending  Archive the digest (undoable_write)"),
        "{}",
        run.report.stdout
    );
}

#[tokio::test]
async fn a_confirmation_is_printed_never_answered_and_the_conversation_stays_open() {
    let id = ConfirmId::parse("c-7").expect("id");
    let run = ask(
        vec![view(AnswerPhase::NeedsYou(NeedsYou::Confirm(id)), plan())],
        false,
        Stdout::Tty,
        JsonFlag::Auto,
    )
    .await;
    let out = &run.report.stdout;
    assert_eq!(run.report.exit, Exit::NeedsYou, "{:?}", run.report);
    assert_eq!(run.report.exit.code(), 8);
    assert!(out.contains("Waiting for your confirmation (c-7)"), "{out}");
    assert!(out.contains("answered in the shell"), "{out}");
    assert!(out.contains("Archive the digest"), "{out}");
    assert!(
        run.seen.lock().expect("lock").closed.is_empty(),
        "left open for the sheet"
    );
    assert_eq!(run.desk.sheets(), 0, "quire-do is not Confirm1");
}

#[tokio::test]
async fn a_refusal_that_needs_the_cloud_is_a_policy_refusal() {
    let refused = AnswerBody::Refused(RefusalWire::NeedsCloud(DataClass::Prompt));
    let run = ask(
        vec![view(AnswerPhase::Done, refused)],
        false,
        Stdout::Tty,
        JsonFlag::Auto,
    )
    .await;
    assert_eq!(run.report.exit, Exit::Refused, "{:?}", run.report);
    assert!(run.report.stdout.contains("needs a cloud model"));
}

#[tokio::test]
async fn a_companion_that_is_not_there_is_unavailable() {
    let run = ask(vec![], true, Stdout::Tty, JsonFlag::Auto).await;
    assert_eq!(run.report.exit, Exit::Unavailable);
    assert!(
        run.report
            .stderr
            .starts_with("quire-do: the companion is not running")
    );
    assert!(run.report.stdout.is_empty());
}

#[tokio::test]
async fn a_failed_answer_ends_app_failed_and_a_pipe_gets_json() {
    let run = ask(
        vec![view(
            AnswerPhase::Failed,
            text(&["The model did not answer."]),
        )],
        false,
        Stdout::Pipe,
        JsonFlag::Auto,
    )
    .await;
    assert_eq!(run.report.exit, Exit::AppFailed);
    let json: serde_json::Value = serde_json::from_str(&run.report.stdout).expect("json");
    assert_eq!(json["exit"], 5);
    assert_eq!(json["answer"]["phase"]["kind"], "failed");
}

#[test]
fn ask_is_a_command_with_its_words_joined_and_nothing_else_is_taken() {
    let words = |w: &[&str]| w.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let parsed =
        docket_cli::parse(&words(&["ask", "what's", "on", "my", "calendar"])).expect("ask");
    assert_eq!(
        parsed.command,
        docket_cli::Command::Ask {
            text: "what's on my calendar".into(),
            space: prov::SpaceId::desktop(),
        }
    );
    let work = docket_cli::parse(&words(&["ask", "--space", "work", "hi"])).expect("ask");
    assert_eq!(
        work.command,
        docket_cli::Command::Ask {
            text: "hi".into(),
            space: prov::SpaceId::parse("work").expect("space"),
        }
    );
    assert!(docket_cli::parse(&words(&["ask", "--space", "No Good!", "hi"])).is_err());
    assert!(docket_cli::parse(&words(&["ask"])).is_err());
}
