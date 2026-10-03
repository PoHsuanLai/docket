//! Setting a case's world up in the fakes: the mail and files the case names, the person's
//! standing consent, the Space's strictness, the other agents that are running, and the
//! companion session whose planner the case plays.

use crate::block::block_on;
use crate::case::{Case, ConsentFixture, FixtureTask};
use crate::runner::{Harness, maximal_policy};
use docket_core::{
    ActionGrantKey, CallerId, CallerRole, ContextKeep, GrantCaller, GrantTarget, IntentsReply,
    IntentsRequest, Keep, Origin, Reveal, SessionOpen, TurnIn, TurnVia,
};
use docket_fake::{FakeSeams, MailContact, MailThread};
use docket_router::{HandleValue, Router, RouterState};
use porter_core::consent::{Decision, Grant, GrantScope, Usage};
use porter_core::{AppId, AppName, DataClass, GrantId, Isolation};
use prov::{AgentRef, Label, Labelled, SessionId, Source, SpaceId, TaskId, UnixSeconds};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::MutexGuard;

/// A case's world could not be built: the fixture names something that is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("setup: {0}")]
pub(crate) struct SetupFault(pub &'static str);

/// What a running case knows about the sessions it made.
#[derive(Debug)]
pub(crate) struct Scene {
    pub space: SpaceId,
    pub front: SessionId,
    pub workers: BTreeMap<AgentRef, SessionId>,
    pub turns: Vec<String>,
}

/// The state, even if a thread panicked while holding it.
pub(crate) fn state_of(router: &Router<FakeSeams>) -> MutexGuard<'_, RouterState> {
    router
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn name(text: &str) -> Result<AppName, SetupFault> {
    AppName::parse(text).map_err(|_| SetupFault("a fixture app name"))
}

/// The connection of the companion daemon.
pub(crate) fn companion() -> Result<CallerId, SetupFault> {
    Ok(CallerId {
        app: AppId {
            name: name("org.quire.Companiond")?,
            isolation: Isolation::Unsandboxed,
        },
        roles: BTreeSet::from([CallerRole::Companion]),
    })
}

/// The connection of the shell's launcher, which records the person's own words.
pub(crate) fn launcher() -> Result<CallerId, SetupFault> {
    Ok(CallerId {
        app: AppId {
            name: name("org.quire.Shell")?,
            isolation: Isolation::Unsandboxed,
        },
        roles: BTreeSet::from([CallerRole::Launcher]),
    })
}

/// The label of somebody else's mail, private to the Space it lives in.
pub(crate) fn mail_label(in_space: SpaceId) -> Label {
    Label::untrusted(Source::Mail, DataClass::Mail, in_space)
}

fn install_world(case: &Case, router: &Router<FakeSeams>) {
    let link = &router.seams.link;
    for m in &case.world.mail {
        link.mail.add_thread(MailThread {
            key: m.key.clone(),
            subject: m.subject.clone(),
            from: m.from.clone(),
            body: m.body.clone(),
        });
    }
    for c in &case.world.contacts {
        link.mail.add_contact(MailContact {
            key: c.key.clone(),
            name: c.name.clone(),
            address: c.address.clone(),
        });
    }
    for f in &case.world.files {
        link.files.add_file(&f.key, &f.name, &f.content);
    }
}

/// The person's standing consent: the companion may use every installed app, in the case's
/// Space, always. It narrows nothing the gate decides: first use is not what a case tests.
fn grant_consent(router: &Router<FakeSeams>, in_space: &SpaceId) -> Result<(), SetupFault> {
    use docket_router::GrantStore;
    let mut apps: BTreeMap<AppName, BTreeSet<DataClass>> = BTreeMap::new();
    for m in state_of(router).registry.all() {
        let classes = m.manifest().actions.iter().flat_map(|a| a.classes.clone());
        apps.entry(m.manifest().app.clone())
            .or_default()
            .extend(classes);
    }
    let mut n = 0u32;
    for (app, classes) in apps {
        for class in classes {
            for usage in [Usage::Interactive, Usage::Background] {
                n += 1;
                router.seams.grants.record(Grant {
                    id: GrantId::parse(&format!("g-eval-{n}"))
                        .map_err(|_| SetupFault("a grant id"))?,
                    key: ActionGrantKey {
                        caller: GrantCaller::Companion,
                        owner: app.clone(),
                        target: GrantTarget::App,
                        class,
                        usage,
                        space: prov::SpaceScope::Only(in_space.clone()),
                    },
                    decision: Decision::Allow,
                    scope: GrantScope::Always,
                    at: UnixSeconds(0),
                });
            }
        }
    }
    Ok(())
}

fn open(
    router: &Router<FakeSeams>,
    in_space: &SpaceId,
    agent: AgentRef,
) -> Result<docket_core::SessionOpened, SetupFault> {
    let reply = block_on(router.handle(
        &companion()?,
        IntentsRequest::SessionOpen(SessionOpen {
            space: in_space.clone(),
            agent,
            parent: None,
        }),
    ));
    match reply {
        IntentsReply::SessionOpened(opened) => Ok(opened),
        _ => Err(SetupFault("a session did not open")),
    }
}

fn other_agents<'a>(
    case: &'a Case,
    router: &Router<FakeSeams>,
) -> Result<BTreeMap<AgentRef, (SessionId, &'a FixtureTask)>, SetupFault> {
    case.world
        .tasks
        .iter()
        .map(|t| {
            let opened = open(router, &t.space, t.agent.clone())?;
            Ok((t.agent.clone(), (opened.session, t)))
        })
        .collect()
}

/// Mints `value` in `session`'s table, as the router would for something it showed a planner.
pub(crate) fn hold(
    router: &Router<FakeSeams>,
    session: &SessionId,
    value: HandleValue,
    label: Label,
    from: Source,
) -> Option<docket_core::Handle> {
    let mut st = state_of(router);
    let record = st.sessions.get_mut(session)?;
    Some(record.handles.mint(Labelled { value, label }, from))
}

/// Builds the case's world and returns what a running case needs of it.
pub(crate) fn install(case: &Case, harness: &Harness) -> Result<Scene, SetupFault> {
    harness.reset();
    let router = &harness.router;
    install_world(case, router);
    if case.world.consent == ConsentFixture::Standing {
        grant_consent(router, &case.space)?;
    }
    state_of(router)
        .strictness
        .insert(case.space.clone(), case.strictness);
    // Other agents first: the companion's session is the newest, which is where its calls land.
    let others = other_agents(case, router)?;
    let front = open(router, &case.space, AgentRef::Companion)?;
    for (session, task) in others.values() {
        let held = hold(
            router,
            &front.session,
            HandleValue::Text(task.goal.clone()),
            Label::untrusted(
                Source::Model(prov::ModelRole::Planner),
                DataClass::Public,
                task.space.clone(),
            ),
            Source::Model(prov::ModelRole::Planner),
        );
        let mut st = state_of(router);
        let task_id = st.sessions.get(session).map(|r| r.task.clone());
        if let (Some(id), Some(h)) = (task_id, held)
            && let Some(t) = st.tasks.get_mut(&id)
        {
            t.goal = Reveal::Handle(h);
        }
    }
    for text in &case.turns {
        let reply = block_on(router.handle(
            &launcher()?,
            IntentsRequest::SessionTurn {
                session: front.session.clone(),
                turn: TurnIn {
                    text: text.clone(),
                    origin: Origin::Launcher,
                    keep: ContextKeep {
                        query: Keep::Dropped,
                        results: Keep::Dropped,
                        selection: Keep::Dropped,
                        window: Keep::Dropped,
                    },
                    via: TurnVia::Typed,
                },
            },
        ));
        if !matches!(reply, IntentsReply::TurnRecorded(_)) {
            return Err(SetupFault("a turn was not recorded"));
        }
    }
    install_policy(router, &front.session, &case.space, front.task.clone());
    Ok(Scene {
        space: case.space.clone(),
        front: front.session,
        workers: others.into_iter().map(|(a, (s, _))| (a, s)).collect(),
        turns: case.turns.clone(),
    })
}

/// The widest policy a writer could produce, stamped with the person's turns.
fn install_policy(
    router: &Router<FakeSeams>,
    session: &SessionId,
    in_space: &SpaceId,
    task: TaskId,
) {
    let mut st = state_of(router);
    if let Some(record) = st.sessions.get_mut(session) {
        let mut policy = maximal_policy(in_space.clone(), task);
        policy.from = record.turns.iter().map(|t| t.id).collect();
        record.policy = Some(policy);
    }
}
