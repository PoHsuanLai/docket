//! A native host over the fakes: docket-fake's router in process (the host's one identity plays
//! the editor and the companion), a scripted planner model, a log shared by every router a test
//! builds, and a clock the test never moves.
#![allow(dead_code)]

#[path = "../../../../companiond/tests/it/support/infer.rs"]
pub mod infer;

use docket_client::{InProcess, Intents};
use docket_core::*;
use docket_fake::{
    FakeSeams, FixedClock, MailContact, MailThread, ScriptedReviewer, ScriptedWriter,
    fake_router_on, host_companion,
};
use docket_planner::PlannerModel;
use docket_router::{GrantStore, Router};
use docket_session::fake::MemoryLog;
use docket_tasks::{Companion, NativeHost, Now, Quiet};
use infer::{Say, ScriptedInfer};
use porter_core::{AppId, AppName, Count, Isolation};
use prov::{DataClass, Effect, EntityKind, SessionId, SpaceId, TaskId, UnixSeconds};
use std::collections::BTreeSet;
use std::sync::Arc;

pub const ARCHIVE: &str = "org.quire.Mail-mail.thread.archive";

#[derive(Debug, Clone, Copy)]
pub struct Still;
impl Now for Still {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(1_000)
    }
}

pub type Host = NativeHost<ScriptedInfer, InProcess<FakeSeams>, Still, Quiet, Arc<MemoryLog>>;

pub fn app(s: &str) -> AppName {
    AppName::parse(s).expect("app")
}

pub fn work() -> SpaceId {
    SpaceId::parse("work").expect("space")
}

pub fn thread(key: &str) -> serde_json::Value {
    serde_json::json!({ "app": "org.quire.Mail", "kind": "mail.thread", "key": key })
}

fn policy_for(task: &str) -> TaskPolicy {
    TaskPolicy {
        task: TaskId::parse(task).expect("task"),
        space: work(),
        from: vec![],
        actions: BTreeSet::from([ActionMatch::AppUpTo(
            app("org.quire.Mail"),
            Effect::Destructive,
        )]),
        kinds: BTreeSet::from([EntityKind::parse("mail.thread").expect("kind")]),
        ceiling: Effect::Destructive,
        max_count: Count(100),
        recipients: vec![],
        destinations: vec![],
        paths: vec![],
        expires: UnixSeconds(i64::MAX),
        rationale: LabelText::parse("test policy").expect("words"),
        state: TaskPolicyState::Active,
    }
}

fn router(log: &Arc<MemoryLog>, task: &str, consent: bool) -> Arc<Router<FakeSeams>> {
    let router = fake_router_on(
        AgentConfig::default(),
        ScriptedReviewer::always_allow(),
        ScriptedWriter::returning(Ok(policy_for(task))),
        FixedClock::at(UnixSeconds(0)),
        Arc::clone(log),
    )
    .expect("router");
    for (key, subject) in [("t1", "Invoice"), ("t2", "Digest")] {
        router.seams.link.mail.add_thread(MailThread {
            key: key.into(),
            subject: subject.into(),
            from: "news@example.test".into(),
            body: "This week".into(),
        });
    }
    router.seams.link.mail.add_contact(MailContact {
        key: "c1".into(),
        name: "Accounting".into(),
        address: "accounting@example.test".into(),
    });
    let mut gid = 0u32;
    let classes: &[DataClass] = if consent {
        &[DataClass::Mail, DataClass::Contacts, DataClass::AppOwn]
    } else {
        &[]
    };
    for (n, class) in classes.iter().copied().enumerate() {
        let owner = if class == DataClass::AppOwn {
            "org.quire.Companion"
        } else {
            "org.quire.Mail"
        };
        // An editor's session asks under the editor's own name, so it holds consent in it.
        let callers = [
            GrantCaller::Companion,
            GrantCaller::Editor(prov::ClientName::parse("org.quire.Acp").expect("client")),
        ];
        for (usage, caller) in [
            porter_core::consent::Usage::Interactive,
            porter_core::consent::Usage::Background,
        ]
        .into_iter()
        .flat_map(|u| callers.clone().into_iter().map(move |c| (u, c)))
        {
            router.seams.grants.record(porter_core::consent::Grant {
                id: porter_core::GrantId::parse(&format!("g-{n}-{}", {
                    gid += 1;
                    gid
                }))
                .expect("grant"),
                key: ActionGrantKey {
                    caller,
                    owner: app(owner),
                    target: GrantTarget::App,
                    class,
                    usage,
                    space: prov::SpaceScope::Only(work()),
                },
                decision: porter_core::consent::Decision::Allow,
                scope: porter_core::consent::GrantScope::Always,
                at: UnixSeconds(0),
            });
        }
    }
    let router = Arc::new(router);
    host_companion(&router);
    router
}

pub type Comp = Companion<ScriptedInfer, InProcess<FakeSeams>, Still, Quiet>;

/// A companion whose one identity plays the editor and the companion.
pub fn companion(router: &Arc<Router<FakeSeams>>, infer: &ScriptedInfer) -> Comp {
    let caller = CallerId {
        app: AppId {
            name: app("org.quire.Acp"),
            isolation: Isolation::Unsandboxed,
        },
        roles: BTreeSet::from([CallerRole::Editor, CallerRole::Companion]),
    };
    Companion::new(
        Intents::over(InProcess::new(router.clone(), caller)),
        PlannerModel::new(infer.clone()),
        AgentConfig::default(),
        Still,
        app("org.quire.Shell"),
    )
}

fn host(router: &Arc<Router<FakeSeams>>, infer: &ScriptedInfer, log: &Arc<MemoryLog>) -> Host {
    NativeHost::over(companion(router, infer), Arc::clone(log))
}

pub struct World {
    pub log: Arc<MemoryLog>,
    pub router: Arc<Router<FakeSeams>>,
    pub infer: ScriptedInfer,
    pub host: Host,
}

impl World {
    pub fn new(script: Vec<Say>) -> World {
        let log = Arc::new(MemoryLog::new());
        let router = router(&log, "t-1", true);
        let infer = ScriptedInfer::new(script);
        let host = host(&router, &infer, &log);
        World {
            log,
            router,
            infer,
            host,
        }
    }

    /// A world where the person has given no standing consent, so a write asks on a sheet.
    pub fn asking(script: Vec<Say>) -> World {
        let log = Arc::new(MemoryLog::new());
        let router = router(&log, "t-1", false);
        let infer = ScriptedInfer::new(script);
        let host = host(&router, &infer, &log);
        World {
            log,
            router,
            infer,
            host,
        }
    }

    /// The daemon and the host are gone and new ones start over the same log: the mail app, which
    /// lives in its own process, is as it was.
    pub fn restart(&mut self, script: Vec<Say>) {
        let fresh = router(&self.log, "t-1", true);
        self.router = fresh;
        self.infer = ScriptedInfer::new(script);
        self.host = host(&self.router, &self.infer, &self.log);
    }

    /// How many times the mail app was asked to perform anything.
    pub fn performed(&self) -> usize {
        self.router.seams.link.performed.lock().expect("lock").len()
    }
}

pub fn session(s: &str) -> SessionId {
    SessionId::parse(s).expect("session")
}
