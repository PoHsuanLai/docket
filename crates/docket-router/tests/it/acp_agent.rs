//! An external coding agent's calls through its host: the host opens the agent's session, records
//! the person's turns, and makes each call the agent asks of it as a router call of the
//! `org.quire.AcpAgent` pseudo-app. The gate is the one every agent meets: the policy point, the
//! reviewers, taint, the breaker, the budgets and the audit; a standing grant belongs to the
//! program and is held in docket's store.

use crate::support::*;
use docket_core::*;
use docket_fake::{BoxFut, FakeSeams, HostedApp, ScriptedConfirmer, ScriptedWriter};
use docket_router::Router;
use prov::{Actor, AgentRef, ClientName, DataClass, Effect, Label, Labelled, SessionId, Source};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

pub(crate) const HOST: &str = "org.quire.AcpAgent";
pub(crate) const CLAUDE: &str = "claude-code";

pub(crate) fn host() -> CallerId {
    caller(HOST, CallerRole::AcpAgent)
}

pub(crate) fn program(name: &str) -> ProgramName {
    ProgramName::parse(name).expect("program")
}

pub(crate) fn always() -> ConfirmAnswer {
    ConfirmAnswer::Allowed {
        scope: GrantScope::Always,
        receipt: receipt(),
    }
}

pub(crate) fn once() -> ConfirmAnswer {
    ConfirmAnswer::Allowed {
        scope: GrantScope::Once,
        receipt: receipt(),
    }
}

/// The host's side of the pseudo-app: it records what the router handed it to perform, and serves
/// a file's text labelled as a file's.
#[derive(Debug, Default)]
pub(crate) struct Performer {
    performed: Mutex<Vec<String>>,
}

impl HostedApp for Performer {
    fn perform(&self, inv: Invocation) -> BoxFut<Result<Outcome, AppRefusal>> {
        self.performed
            .lock()
            .expect("lock")
            .push(inv.action.as_str().to_owned());
        let value = (inv.action.as_str() == FILES_READ).then(|| Labelled {
            value: Value::Text("fn main() {}".into()),
            label: Label::untrusted(Source::File, DataClass::Files, inv.space.clone()),
        });
        Box::pin(async move {
            Ok(Outcome {
                value,
                said: None,
                show: Preview::None,
                undo: Undoable::No,
                follow: Follow::Nothing,
            })
        })
    }
}

pub(crate) struct World {
    pub(crate) router: Router<FakeSeams>,
    host: Arc<Performer>,
}

pub(crate) fn world() -> World {
    let router = router();
    let host = Arc::new(Performer::default());
    router.seams.link.host(
        porter_core::AppName::parse(HOST).expect("app"),
        host.clone(),
    );
    World { router, host }
}

impl World {
    pub(crate) fn performed(&self) -> Vec<String> {
        self.host.performed.lock().expect("lock").clone()
    }

    pub(crate) fn records(&self) -> Vec<AuditRecord> {
        self.router.seams.sink.records()
    }

    pub(crate) async fn open(&self, name: &str, sheets: SheetSurface) -> SessionId {
        self.open_labelled(name, sheets, None).await
    }

    /// Opens as the host does when the person's `agents.toml` entry carries a `label`.
    pub(crate) async fn open_labelled(
        &self,
        name: &str,
        sheets: SheetSurface,
        label: Option<prov::AgentLabel>,
    ) -> SessionId {
        let reply = ask(
            &self.router,
            &host(),
            IntentsRequest::SessionOpen(SessionOpen {
                space: space("work"),
                agent: AgentRef::Companion,
                parent: None,
                cwd: Some(Workspace::parse("/home/u/proj").expect("cwd")),
                started_from: None,
                external: Some(ExternalAgent {
                    program: program(name),
                    sheets,
                    label,
                    rewind: Default::default(),
                }),
            }),
        )
        .await;
        match reply {
            IntentsReply::SessionOpened(opened) => {
                say_as(
                    &self.router,
                    &host(),
                    &opened.session,
                    "fix the failing test",
                )
                .await;
                give_policy(&self.router, &opened.session, wide(&opened.task));
                opened.session
            }
            other => panic!("open: {other:?}"),
        }
    }

    pub(crate) async fn call(
        &self,
        session: &SessionId,
        request: CallRequest,
    ) -> Result<Outcome, CallRefusal> {
        match ask(
            &self.router,
            &host(),
            IntentsRequest::Perform {
                activation: None,
                call: request,
                session: Some(session.clone()),
                parent_window: None,
            },
        )
        .await
        {
            IntentsReply::Performed(result) => *result,
            other => panic!("perform: {other:?}"),
        }
    }

    pub(crate) fn sheets(&self) -> Vec<ConfirmRequest> {
        self.router.seams.confirmer.requests()
    }

    pub(crate) fn used(&self) -> usize {
        self.records()
            .iter()
            .filter(|r| matches!(r, AuditRecord::StandingUsed { .. }))
            .count()
    }
}

/// A policy that covers the whole pseudo-app.
pub(crate) fn wide(task: &prov::TaskId) -> TaskPolicy {
    let mut policy = wide_policy(task, "work");
    policy.actions = BTreeSet::from([ActionMatch::AppUpTo(
        acp_agent_app().expect("app"),
        Effect::Destructive,
    )]);
    policy.kinds = BTreeSet::new();
    policy
}

pub(crate) fn acp(name: &str, paths: &[&str], args: Vec<(&str, Value)>) -> CallRequest {
    CallRequest {
        action: acp_agent_action(name).expect("action"),
        target: if paths.is_empty() {
            TargetValue::Nothing
        } else {
            TargetValue::Files(
                paths
                    .iter()
                    .map(|p| FileRef::parse(p).expect("file"))
                    .collect(),
            )
        },
        args: args
            .into_iter()
            .map(|(k, v)| {
                (
                    param(k),
                    Labelled {
                        value: v,
                        label: trusted(),
                    },
                )
            })
            .collect(),
        origin: Origin::Companion,
    }
}

pub(crate) fn read(path: &str) -> CallRequest {
    acp(
        FILES_READ,
        &[path],
        vec![("stage", Value::Text("stage-1".into()))],
    )
}

pub(crate) fn write(path: &str) -> CallRequest {
    acp(
        FILES_WRITE,
        &[path],
        vec![
            ("lines", Value::Integer(3)),
            ("stage", Value::Text("stage-1".into())),
        ],
    )
}

pub(crate) fn run(line: &str) -> CallRequest {
    run_with(line, DERIVES_OWN, NETWORK_CLOSED)
}

/// A command with the two facts the host would send: where its arguments come from, and the
/// sandbox's network.
pub(crate) fn run_with(line: &str, derives: &str, network: &str) -> CallRequest {
    let choice = |id: &str| Value::Choice(ChoiceId::parse(id).expect("choice"));
    acp(
        TERMINAL_RUN,
        &[],
        vec![
            ("command", Value::Text(line.into())),
            (
                "cwd",
                Value::File(FileRef::parse("/home/u/proj").expect("cwd")),
            ),
            ("stage", Value::Text("stage-2".into())),
            (DERIVES_PARAM, choice(derives)),
            (NETWORK_PARAM, choice(network)),
        ],
    )
}

pub(crate) fn permission(kind: &str, paths: &[&str]) -> CallRequest {
    acp(&format!("acpagent.{kind}"), paths, vec![])
}

pub(crate) fn scope_under(name: &str, dir: &str) -> StandingScope {
    StandingScope::Files {
        action: acp_agent_action(name).expect("action"),
        under: AbsPath::parse(dir).expect("dir"),
    }
}

#[tokio::test]
async fn a_call_is_audited_as_the_agent_program_and_a_read_runs_without_a_question() {
    let w = world();
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    let served = w
        .call(&session, read("/home/u/proj/a.rs"))
        .await
        .expect("read");
    assert_eq!(
        served.value.expect("text").value,
        Value::Text("fn main() {}".into())
    );
    assert!(w.sheets().is_empty());
    assert_eq!(w.performed(), [FILES_READ]);
    let actors: Vec<Actor> = w
        .records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Call { actor, action, .. } if action.name.as_str() == FILES_READ => {
                Some(actor)
            }
            _ => None,
        })
        .collect();
    let program = prov::AgentProgram::parse("claude-code").expect("program");
    assert_eq!(
        actors,
        [Actor::Acp {
            program,
            label: None
        }]
    );
}

#[tokio::test]
async fn only_the_host_of_the_session_may_make_the_agents_calls() {
    let w = world();
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    for (who, why) in [
        (companion(), "the companion"),
        (launcher(), "the launcher"),
        (caller("org.quire.Mail", CallerRole::App), "an app"),
        (caller("org.quire.Do", CallerRole::Cli), "a terminal"),
    ] {
        let reply = ask(
            &w.router,
            &who,
            IntentsRequest::Perform {
                activation: None,
                call: read("/home/u/proj/a.rs"),
                session: Some(session.clone()),
                parent_window: None,
            },
        )
        .await;
        match reply {
            IntentsReply::Performed(done) => assert_eq!(
                *done,
                Err(CallRefusal::Denied(DenyCode::NotAllowed)),
                "{why}"
            ),
            IntentsReply::Refused(_) => {}
            other => panic!("{why}: {other:?}"),
        }
    }
    // Another host cannot act in this host's session.
    let other = caller("org.example.OtherHost", CallerRole::AcpAgent);
    let reply = ask(
        &w.router,
        &other,
        IntentsRequest::Perform {
            activation: None,
            call: read("/home/u/proj/a.rs"),
            session: Some(session),
            parent_window: None,
        },
    )
    .await;
    assert_eq!(reply, IntentsReply::Refused(WireRefusal::NotAllowed));
    assert!(w.performed().is_empty());
}

#[tokio::test]
async fn a_session_the_launcher_opened_is_not_an_agents_even_if_it_says_so() {
    let w = world();
    let reply = ask(
        &w.router,
        &launcher(),
        IntentsRequest::SessionOpen(SessionOpen {
            space: space("work"),
            agent: AgentRef::Companion,
            parent: None,
            cwd: None,
            started_from: None,
            external: Some(ExternalAgent {
                program: program(CLAUDE),
                sheets: SheetSurface::Desktop,
                label: None,
                rewind: Default::default(),
            }),
        }),
    )
    .await;
    let IntentsReply::SessionOpened(opened) = reply else {
        panic!("{reply:?}")
    };
    let calls = ask(
        &w.router,
        &host(),
        IntentsRequest::Perform {
            activation: None,
            call: read("/home/u/proj/a.rs"),
            session: Some(opened.session),
            parent_window: None,
        },
    )
    .await;
    assert_eq!(calls, IntentsReply::Refused(WireRefusal::NotAllowed));
    // And the host opens an agent's session or none.
    let bare = ask(
        &w.router,
        &host(),
        IntentsRequest::SessionOpen(SessionOpen {
            space: space("work"),
            agent: AgentRef::Companion,
            parent: None,
            cwd: None,
            started_from: None,
            external: None,
        }),
    )
    .await;
    assert_eq!(bare, IntentsReply::Refused(WireRefusal::NotAllowed));
}

#[tokio::test]
async fn after_a_file_is_served_a_command_derived_from_it_asks_and_offers_no_always() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![once(), once()]);
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    w.call(&session, run("cargo test"))
        .await
        .expect("untainted: asks, then runs");
    assert!(
        matches!(w.sheets()[0].always, AlwaysOffer::Offered(_)),
        "{:?}",
        w.sheets()[0].always
    );
    w.call(&session, read("/home/u/proj/a.rs"))
        .await
        .expect("read");
    w.call(
        &session,
        run_with("cat /home/u/proj/a.rs", DERIVES_READ, NETWORK_CLOSED),
    )
    .await
    .expect("derived: asks, then runs");
    assert_eq!(
        w.sheets()[1].always,
        AlwaysOffer::Withheld(Withheld::UntrustedIntoSink)
    );
}

#[tokio::test]
async fn a_sheet_goes_to_the_desktop_unless_the_host_asked_to_show_it_itself() {
    let w = world();
    let desktop = w.open(CLAUDE, SheetSurface::Desktop).await;
    let hosted = w.open("gemini-cli", SheetSurface::Host).await;
    w.call(&desktop, run("make")).await.expect_err("dismissed");
    w.call(&hosted, run("make")).await.expect_err("dismissed");
    let sheets = w.sheets();
    assert_eq!(sheets[0].editor, None);
    assert_eq!(
        sheets[1].editor,
        Some(EditorRoute {
            client: ClientName::parse(HOST).expect("client"),
            session: hosted,
        })
    );
}

#[tokio::test]
async fn the_policy_comes_from_the_persons_typed_turn_and_the_host_cannot_speak_for_the_agent() {
    let mut w = world();
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    let task = w
        .router
        .state
        .lock()
        .expect("lock")
        .sessions
        .get(&session)
        .expect("session")
        .task
        .clone();
    w.router.seams.writer = ScriptedWriter::returning(Ok(wide(&task)));
    say_as(&w.router, &host(), &session, "also update the docs").await;
    let turns = w.router.state.lock().expect("lock").sessions[&session]
        .turns
        .clone();
    assert_eq!(
        turns.last().map(|t| &t.from),
        Some(&TurnSource::Agent(
            porter_core::AppName::parse(HOST).expect("app")
        ))
    );
    assert_eq!(
        w.router.seams.writer.calls().last().map(|c| c.0.clone()),
        Some(task)
    );
    // Nothing else the agent says is a turn: the host has no note, read or widen member.
    for request in [
        IntentsRequest::SessionHandles {
            session: session.clone(),
        },
        IntentsRequest::SessionTaskPolicy {
            session: session.clone(),
        },
    ] {
        assert_eq!(
            ask(&w.router, &host(), request).await,
            IntentsReply::Refused(WireRefusal::NotAllowed)
        );
    }
}
