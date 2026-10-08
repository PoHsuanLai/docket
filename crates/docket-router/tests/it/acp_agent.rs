//! An external coding agent's calls through its host: the host opens the agent's session, records
//! the person's turns, and makes each call the agent asks of it as a router call of the
//! `org.quire.AcpAgent` pseudo-app. The gate is the one every agent meets: the policy point, the
//! reviewers, taint, the breaker, the budgets and the audit; a standing grant belongs to the
//! program and is held in docket's store.

use crate::support::*;
use docket_core::*;
use docket_fake::{
    BoxFut, FakeSeams, HostedApp, ReviewMode, ScriptedConfirmer, ScriptedReviewer, ScriptedWriter,
};
use docket_router::Router;
use prov::{Actor, AgentRef, ClientName, DataClass, Effect, Label, Labelled, SessionId, Source};
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

const HOST: &str = "org.quire.AcpAgent";
const CLAUDE: &str = "claude-code";

fn host() -> CallerId {
    caller(HOST, CallerRole::AcpAgent)
}

fn program(name: &str) -> ProgramName {
    ProgramName::parse(name).expect("program")
}

fn always() -> ConfirmAnswer {
    ConfirmAnswer::Allowed {
        scope: GrantScope::Always,
        receipt: receipt(),
    }
}

fn once() -> ConfirmAnswer {
    ConfirmAnswer::Allowed {
        scope: GrantScope::Once,
        receipt: receipt(),
    }
}

/// The host's side of the pseudo-app: it records what the router handed it to perform, and serves
/// a file's text labelled as a file's.
#[derive(Debug, Default)]
struct Performer {
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

struct World {
    router: Router<FakeSeams>,
    host: Arc<Performer>,
}

fn world() -> World {
    let router = router();
    let host = Arc::new(Performer::default());
    router.seams.link.host(
        porter_core::AppName::parse(HOST).expect("app"),
        host.clone(),
    );
    World { router, host }
}

impl World {
    fn performed(&self) -> Vec<String> {
        self.host.performed.lock().expect("lock").clone()
    }

    fn records(&self) -> Vec<AuditRecord> {
        self.router.seams.sink.records()
    }

    async fn open(&self, name: &str, sheets: SheetSurface) -> SessionId {
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

    async fn call(
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

    fn sheets(&self) -> Vec<ConfirmRequest> {
        self.router.seams.confirmer.requests()
    }

    fn used(&self) -> usize {
        self.records()
            .iter()
            .filter(|r| matches!(r, AuditRecord::StandingUsed { .. }))
            .count()
    }
}

/// A policy that covers the whole pseudo-app.
fn wide(task: &prov::TaskId) -> TaskPolicy {
    let mut policy = wide_policy(task, "work");
    policy.actions = BTreeSet::from([ActionMatch::AppUpTo(
        acp_agent_app().expect("app"),
        Effect::Destructive,
    )]);
    policy.kinds = BTreeSet::new();
    policy
}

fn acp(name: &str, paths: &[&str], args: Vec<(&str, Value)>) -> CallRequest {
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

fn read(path: &str) -> CallRequest {
    acp(FILES_READ, &[path], vec![])
}

fn write(path: &str) -> CallRequest {
    acp(
        FILES_WRITE,
        &[path],
        vec![
            ("lines", Value::Integer(3)),
            ("stage", Value::Text("stage-1".into())),
        ],
    )
}

fn run(line: &str) -> CallRequest {
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
        ],
    )
}

fn permission(kind: &str, paths: &[&str]) -> CallRequest {
    acp(&format!("acpagent.{kind}"), paths, vec![])
}

fn scope_under(name: &str, dir: &str) -> StandingScope {
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
    let agent = ClientName::parse("acp:claude-code").expect("client");
    assert_eq!(actors, [Actor::Mcp { client: agent }]);
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
async fn after_a_file_is_served_a_command_asks_and_offers_no_always() {
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
    w.call(&session, run("cargo test"))
        .await
        .expect("tainted: asks, then runs");
    assert_eq!(
        w.sheets()[1].always,
        AlwaysOffer::Withheld(Withheld::UntrustedIntoSink)
    );
}

#[tokio::test]
async fn an_always_on_a_write_is_a_grant_for_the_program_used_audited_listed_and_revoked() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![always()]);
    let claude = w.open(CLAUDE, SheetSurface::Desktop).await;
    // Taint the session so the write asks.
    w.call(&claude, read("/home/u/proj/a.rs"))
        .await
        .expect("read");
    w.call(&claude, write("/home/u/proj/src/a.rs"))
        .await
        .expect("asked, always");
    let held = w.router.standing_grants();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].caller, GrantCaller::AcpAgent(program(CLAUDE)));
    assert_eq!(held[0].scope, scope_under(FILES_WRITE, "/home/u/proj/src"));
    // A matching later call runs on the grant: no sheet, audited.
    w.call(&claude, write("/home/u/proj/src/b.rs"))
        .await
        .expect("on the grant");
    assert_eq!(w.sheets().len(), 1);
    assert_eq!(w.used(), 1);
    // A sibling directory asks again.
    w.call(&claude, write("/home/u/proj/other/c.rs"))
        .await
        .expect_err("asks");
    assert_eq!(w.sheets().len(), 2);
    // Another program has no such grant.
    let gemini = w.open("gemini-cli", SheetSurface::Desktop).await;
    w.call(&gemini, read("/home/u/proj/a.rs"))
        .await
        .expect("read");
    w.call(&gemini, write("/home/u/proj/src/a.rs"))
        .await
        .expect_err("asks");
    assert_eq!(w.sheets().len(), 3);
    assert_eq!(w.used(), 1);
    // Settings lists it over Control and revokes it.
    let listed = ask(&w.router, &control(), IntentsRequest::ControlStandingGrants).await;
    assert_eq!(listed, IntentsReply::StandingGrants(held.clone()));
    let gone = ask(
        &w.router,
        &control(),
        IntentsRequest::ControlStandingRevoke(held[0].id.clone()),
    )
    .await;
    assert_eq!(gone, IntentsReply::Done);
    let again = w
        .call(&claude, write("/home/u/proj/src/d.rs"))
        .await
        .expect_err("asks again");
    assert_eq!(w.sheets().len(), 4, "{again:?}");
}

#[tokio::test]
async fn a_reviewer_deny_refuses_a_call_a_grant_let_through() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![always()]);
    let claude = w.open(CLAUDE, SheetSurface::Desktop).await;
    w.call(&claude, read("/home/u/proj/a.rs"))
        .await
        .expect("read");
    w.call(&claude, write("/home/u/proj/src/a.rs"))
        .await
        .expect("always");
    w.router.seams.reviewer = ScriptedReviewer::queued(
        vec![Ok(action_review::ReviewVerdict::Deny {
            why: action_review::ReviewReason {
                code: ReasonCode::OutsideRequest,
                text: ReasonText("no".into()),
            },
        })],
        ReviewMode::AlwaysAllow,
    );
    let before = w.performed().len();
    let refused = w
        .call(&claude, write("/home/u/proj/src/b.rs"))
        .await
        .expect_err("the reviewer said no");
    assert_eq!(refused, CallRefusal::Denied(DenyCode::NotAllowed));
    assert_eq!(w.performed().len(), before, "nothing was performed");
}

#[tokio::test]
async fn three_refusals_pause_the_agents_session_until_the_person_speaks() {
    let w = world();
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    for line in ["make a", "make b", "make c"] {
        let refused = w.call(&session, run(line)).await.expect_err("dismissed");
        assert!(
            matches!(refused, CallRefusal::Unconfirmed(_)),
            "{refused:?}"
        );
    }
    let paused = w
        .call(&session, read("/home/u/proj/a.rs"))
        .await
        .expect_err("paused");
    assert_eq!(paused, CallRefusal::Paused(BreakerTrip::Probing));
    say_as(&w.router, &host(), &session, "carry on").await;
    w.call(&session, read("/home/u/proj/a.rs"))
        .await
        .expect("the person spoke");
}

#[tokio::test]
async fn a_permission_the_person_allowed_covers_the_matching_write_once() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![once()]);
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    w.call(&session, read("/home/u/proj/a.rs"))
        .await
        .expect("read");
    w.call(&session, permission("edit", &["/home/u/proj/a.rs"]))
        .await
        .expect("asked once, allowed");
    assert_eq!(w.sheets().len(), 1);
    w.call(&session, write("/home/u/proj/a.rs"))
        .await
        .expect("on the yes");
    assert_eq!(w.sheets().len(), 1, "not asked twice");
    assert!(w.records().iter().any(|r| matches!(
        r,
        AuditRecord::ApprovalUsed { action, .. } if action.name.as_str() == FILES_WRITE
    )));
    // Once: the next write of the same file asks.
    w.call(&session, write("/home/u/proj/a.rs"))
        .await
        .expect_err("asks");
    assert_eq!(w.sheets().len(), 2);
}

#[tokio::test]
async fn a_permission_covers_only_what_it_named_and_ends_when_the_person_speaks() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![once()]);
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    w.call(&session, read("/home/u/proj/a.rs"))
        .await
        .expect("read");
    w.call(&session, permission("edit", &["/home/u/proj/a.rs"]))
        .await
        .expect("allowed");
    w.call(&session, write("/home/u/proj/b.rs"))
        .await
        .expect_err("another file asks");
    assert_eq!(w.sheets().len(), 2);
    say_as(&w.router, &host(), &session, "and now the other one").await;
    let again = w
        .call(&session, write("/home/u/proj/a.rs"))
        .await
        .expect_err("a new turn: asks");
    assert_eq!(w.sheets().len(), 3, "{again:?}");
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
