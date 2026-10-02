//! A child task's policy and the roster of tasks.

use docket_core::*;
use docket_router::*;
use porter_core::{AppName, Count};
use prov::{AgentRef, Effect, EntityKind, ReportStatus, SessionId, SpaceId, TaskId, UnixSeconds};
use std::collections::BTreeSet;

fn space(s: &str) -> SpaceId {
    SpaceId::parse(s).expect("space")
}
fn app(s: &str) -> AppName {
    AppName::parse(s).expect("app")
}
fn task(s: &str) -> TaskId {
    TaskId::parse(s).expect("task")
}
fn kind(s: &str) -> EntityKind {
    EntityKind::parse(s).expect("kind")
}

fn policy(task_id: &str, in_space: &str) -> TaskPolicy {
    TaskPolicy {
        task: task(task_id),
        space: space(in_space),
        from: vec![TurnId(1)],
        actions: BTreeSet::from([ActionMatch::AppUpTo(
            app("org.quire.Mail"),
            Effect::Outbound,
        )]),
        kinds: BTreeSet::from([kind("mail.thread"), kind("mail.contact")]),
        ceiling: Effect::Outbound,
        max_count: Count(10),
        recipients: vec![TrustedPattern::Domain("example.com".into())],
        destinations: vec![],
        paths: vec![],
        expires: UnixSeconds(3600),
        rationale: LabelText::parse("tidy the inbox").expect("words"),
        state: TaskPolicyState::Active,
    }
}

fn edit(f: impl FnOnce(&mut TaskPolicy)) -> TaskPolicy {
    let mut p = policy("t-child", "work");
    f(&mut p);
    p
}

#[test]
fn a_child_policy_is_never_wider_than_its_parents() {
    let parent = policy("t-parent", "work");
    let cases: Vec<(&str, TaskPolicy)> = vec![
        ("the same", edit(|_| {})),
        (
            "a higher ceiling and count",
            edit(|p| {
                p.ceiling = Effect::Destructive;
                p.max_count = Count(500);
            }),
        ),
        (
            "an app the parent lacks",
            edit(|p| {
                p.actions.insert(ActionMatch::AppUpTo(
                    app("org.quire.Files"),
                    Effect::Destructive,
                ));
            }),
        ),
        (
            "a kind the parent lacks",
            edit(|p| {
                p.kinds.insert(kind("files.file"));
            }),
        ),
        ("a later expiry", edit(|p| p.expires = UnixSeconds(99_999))),
        (
            "a recipient the parent never trusted",
            edit(|p| {
                p.recipients
                    .push(TrustedPattern::Domain("evil.test".into()))
            }),
        ),
        (
            "a wider domain than the parent's",
            edit(|p| p.recipients = vec![TrustedPattern::Domain("com".into())]),
        ),
        ("another Space", {
            let mut p = edit(|_| {});
            p.space = space("home");
            p
        }),
        (
            "nothing at all",
            edit(|p| {
                p.actions.clear();
                p.kinds.clear();
            }),
        ),
    ];
    for (name, requested) in cases {
        let child = child_policy(&parent, &requested);
        assert!(
            matches!(
                compare(&child, &parent),
                PolicyChange::Narrows | PolicyChange::Same
            ),
            "case: {name}: {:?}",
            compare(&child, &parent)
        );
        assert_eq!(
            child.task, requested.task,
            "case: {name}: the child's own task"
        );
        assert_eq!(
            child.rationale, requested.rationale,
            "case: {name}: its own rationale"
        );
    }
}

#[test]
fn a_child_keeps_what_both_allow() {
    let parent = policy("t-parent", "work");
    let requested = edit(|p| {
        p.ceiling = Effect::UndoableWrite;
        p.max_count = Count(3);
        p.kinds = BTreeSet::from([kind("mail.thread")]);
        p.recipients = vec![TrustedPattern::Domain("mail.example.com".into())];
    });
    let child = child_policy(&parent, &requested);
    assert_eq!(child.ceiling, Effect::UndoableWrite);
    assert_eq!(child.max_count, Count(3));
    assert_eq!(child.kinds, BTreeSet::from([kind("mail.thread")]));
    assert_eq!(
        child.recipients,
        vec![TrustedPattern::Domain("mail.example.com".into())],
        "a narrower trusted domain survives"
    );
    assert_eq!(child.expires, UnixSeconds(3600));
    assert_eq!(child.state, TaskPolicyState::Active);
}

#[test]
fn a_child_of_another_space_covers_nothing() {
    let mut requested = edit(|_| {});
    requested.space = space("home");
    let child = child_policy(&policy("t-parent", "work"), &requested);
    assert!(child.actions.is_empty() && child.kinds.is_empty());
}

fn record(
    t: &str,
    agent: AgentRef,
    in_space: &str,
    state: TaskState,
    goal: Reveal<String>,
) -> TaskRecord {
    TaskRecord {
        task: task(t),
        session: SessionId::parse(&format!("s-{t}")).expect("session"),
        agent: agent.clone(),
        parent: None,
        space: space(in_space),
        state,
        goal,
        last: None,
        ledger: TaskLedger {
            task: task(t),
            agent,
            parent: None,
            space: space(in_space),
            started: UnixSeconds(0),
            asked: vec![],
            steps: vec![],
            touched: vec![],
            results: vec![],
        },
    }
}

#[test]
fn the_roster_shows_a_goal_in_this_space_and_presence_in_another() {
    let mut here = record(
        "t-1",
        AgentRef::Worker { task: task("t-1") },
        "work",
        TaskState::Working,
        Reveal::Handle(Handle(7)),
    );
    here.ledger.asked.push(UserTurn {
        id: TurnId(1),
        text: "skip the newsletters".into(),
        at: UnixSeconds(1),
        from: TurnSource::Launcher,
        via: TurnVia::Typed,
    });
    let there = record(
        "t-2",
        AgentRef::Worker { task: task("t-2") },
        "home",
        TaskState::Ended(ReportStatus::Done),
        Reveal::Plain("book the flights".into()),
    );
    let mut table = TaskTable::new();
    table.insert(here);
    table.insert(there);
    let roster = roster_of(&table, &space("work"));
    assert_eq!(roster.entries.len(), 2);
    let RosterDetail::Full(full) = &roster.entries[0].detail else {
        panic!("a task in this Space shows its line")
    };
    assert_eq!(
        full.goal,
        Reveal::Handle(Handle(7)),
        "a planner's goal stays a handle"
    );
    assert_eq!(
        full.told.as_ref().map(LeadText::as_str),
        Some("skip the newsletters")
    );
    assert_eq!(roster.entries[0].state, RosterState::Working);
    assert_eq!(roster.entries[1].detail, RosterDetail::PresenceOnly);
    assert_eq!(roster.entries[1].state, RosterState::Done);
    let json = serde_json::to_string(&roster.entries[1]).expect("json");
    assert!(!json.contains("flights"), "{json}");
}

#[test]
fn a_task_state_has_one_roster_word() {
    let cases = [
        (TaskState::Starting, RosterState::Starting),
        (TaskState::Working, RosterState::Working),
        (TaskState::NeedsYou, RosterState::NeedsYou),
        (TaskState::Paused, RosterState::Paused),
        (TaskState::Ended(ReportStatus::Done), RosterState::Done),
        (TaskState::Ended(ReportStatus::Failed), RosterState::Failed),
        (
            TaskState::Ended(ReportStatus::Cancelled),
            RosterState::Cancelled,
        ),
        (
            TaskState::Ended(ReportStatus::Progress),
            RosterState::Working,
        ),
    ];
    for (state, want) in cases {
        assert_eq!(state.roster_state(), want, "{state:?}");
    }
}
