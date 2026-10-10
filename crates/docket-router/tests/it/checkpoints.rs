//! Restore points through the router: a turn saves one before the host is told, the list and the
//! plan are the person's, and a restore saves a safety point, writes the files back and refuses
//! a plan that is not the one the person saw. An agent can do none of it.

use crate::acp_agent::{host, program};
use crate::support::*;
use docket_checkpoint::{CheckpointStore, DropAsk, Tracking, WorkRoot};
use docket_core::*;
use docket_fake::FakeSeams;
use docket_router::{CHECKPOINTS_APP, CHECKPOINTS_RESTORE, Clock, Router};
use porter_core::Count;
use prov::{ActionName, Actor, AgentRef, Labelled, SessionId};
use std::collections::BTreeMap;

const WORK: &str = "/work/app";

fn root() -> WorkRoot {
    WorkRoot::of(&Workspace::parse(WORK).expect("workspace")).expect("root")
}

fn file(name: &str) -> WorkPath {
    WorkPath::parse(name).expect("path")
}

fn put(router: &Router<FakeSeams>, name: &str, content: &str) {
    router
        .seams
        .checkpoints
        .write(&root(), &file(name), content, Tracking::Followed);
}

fn read(router: &Router<FakeSeams>, name: &str) -> Option<String> {
    router.seams.checkpoints.read(&root(), &file(name))
}

async fn open_in(
    router: &Router<FakeSeams>,
    who: &CallerId,
    cwd: Option<&str>,
    external: Option<ExternalAgent>,
) -> SessionId {
    let reply = ask(
        router,
        who,
        IntentsRequest::SessionOpen(SessionOpen {
            space: space("work"),
            agent: AgentRef::Companion,
            parent: None,
            cwd: cwd.map(|c| Workspace::parse(c).expect("cwd")),
            started_from: None,
            external,
        }),
    )
    .await;
    match reply {
        IntentsReply::SessionOpened(opened) => opened.session,
        other => panic!("open: {other:?}"),
    }
}

async fn listed(router: &Router<FakeSeams>, session: &SessionId) -> CheckpointList {
    let reply = ask(
        router,
        &launcher(),
        IntentsRequest::CheckpointList {
            session: session.clone(),
        },
    )
    .await;
    match reply {
        IntentsReply::Checkpoints(list) => *list,
        other => panic!("list: {other:?}"),
    }
}

async fn planned(router: &Router<FakeSeams>, session: &SessionId, id: u32) -> RestorePlan {
    let reply = ask(
        router,
        &launcher(),
        IntentsRequest::CheckpointPlan {
            session: session.clone(),
            id: CheckpointId(id),
        },
    )
    .await;
    match reply {
        IntentsReply::CheckpointPlan(Ok(plan)) => plan,
        other => panic!("plan: {other:?}"),
    }
}

fn restore_call(session: &SessionId, point: i64, plan: &str) -> Invocation {
    let arg = |value: Value| Labelled {
        value,
        label: trusted(),
    };
    let args: BTreeMap<ParamName, Labelled<Value>> = BTreeMap::from([
        (param("session"), arg(Value::Text(session.as_str().into()))),
        (param("point"), arg(Value::Integer(point))),
        (param("plan"), arg(Value::Text(plan.into()))),
    ]);
    Invocation {
        call: CallId(1),
        action: ActionName::parse(CHECKPOINTS_RESTORE).expect("action"),
        target: TargetValue::Nothing,
        args,
        actor: Actor::User {
            via: app("org.quire.Shell"),
        },
        origin: Origin::Launcher,
        space: space("work"),
    }
}

fn running_words() -> AppRefusal {
    AppRefusal::Failed(FailText("Wait for this turn to finish".to_owned()))
}

/// The host of `session` says `turn` is over.
async fn ended(router: &Router<FakeSeams>, session: &SessionId, turn: TurnId) {
    let request = IntentsRequest::SessionTurnEnded {
        session: session.clone(),
        turn,
        how: TurnEnd::Answered,
    };
    assert_eq!(ask(router, &launcher(), request).await, IntentsReply::Done);
}

fn stale_words() -> AppRefusal {
    AppRefusal::Failed(FailText(
        "Files changed while you were deciding. Look again.".to_owned(),
    ))
}

#[tokio::test]
async fn a_turn_saves_a_point_before_it_is_recorded_and_the_list_shows_it() {
    let router = router();
    put(&router, "a.txt", "one");
    let session = open_in(&router, &companion(), Some(WORK), None).await;
    let first = say(&router, &session, "first").await;
    put(&router, "a.txt", "two");
    let second = say(&router, &session, "second").await;

    let list = listed(&router, &session).await;
    let saved = |id: u32, turn: TurnId| CheckpointRow::Saved {
        id: CheckpointId(id),
        turn,
        at: router.seams.clock.now(),
        state: SavedState::Available,
    };
    assert_eq!(list.rows, [saved(1, first), saved(2, second)]);
    // The point of the first turn holds the file as it was then.
    let plan = planned(&router, &session, 1).await;
    assert_eq!(plan.changed, [file("a.txt")]);
}

#[tokio::test]
async fn no_workspace_writes_no_entry_and_an_agent_that_keeps_its_own_history_says_so() {
    let router = router();
    let plain = open_in(&router, &companion(), None, None).await;
    say(&router, &plain, "hello").await;
    assert!(listed(&router, &plain).await.rows.is_empty());

    let external = ExternalAgent {
        program: program("claude-code"),
        sheets: SheetSurface::Desktop,
        label: None,
        rewind: Rewind::Agent,
    };
    let agent = open_in(&router, &host(), Some(WORK), Some(external)).await;
    let turn = say_as(&router, &host(), &agent, "go").await;
    let rows = listed(&router, &agent).await.rows;
    assert!(matches!(
        rows.as_slice(),
        [CheckpointRow::NotSaved { turn: t, why: SkipReason::AgentKeepsOwn, .. }] if *t == turn
    ));
    assert_eq!(
        router.seams.checkpoints.held(&root(), &agent).await,
        Ok(Vec::new())
    );
}

#[tokio::test]
async fn a_folder_that_cannot_keep_points_is_listed_as_not_saved_and_the_turn_still_counts() {
    let router = router();
    router.seams.checkpoints.lack_history(&root());
    let session = open_in(&router, &companion(), Some(WORK), None).await;
    let turn = say(&router, &session, "go").await;
    let rows = listed(&router, &session).await.rows;
    assert!(matches!(
        rows.as_slice(),
        [CheckpointRow::NotSaved { turn: t, why: SkipReason::NoHistory, .. }] if *t == turn
    ));
}

#[tokio::test]
async fn the_newest_points_are_kept_and_an_older_one_shows_as_cleared() {
    let router = router();
    put(&router, "a.txt", "one");
    let session = open_in(&router, &companion(), Some(WORK), None).await;
    for n in 0..22 {
        say(&router, &session, &format!("turn {n}")).await;
    }
    let held = router
        .seams
        .checkpoints
        .held(&root(), &session)
        .await
        .expect("held");
    assert_eq!(held.len(), 20);
    let states: Vec<SavedState> = listed(&router, &session)
        .await
        .rows
        .iter()
        .filter_map(|row| match row {
            CheckpointRow::Saved { state, .. } => Some(*state),
            _ => None,
        })
        .collect();
    assert_eq!(states.len(), 22);
    assert_eq!(states[..2], [SavedState::Gone, SavedState::Gone]);
    assert!(states[2..].iter().all(|s| *s == SavedState::Available));
}

#[tokio::test]
async fn restoring_saves_a_safety_point_writes_the_files_back_and_is_logged() {
    let router = router();
    put(&router, "a.txt", "one");
    let session = open_in(&router, &companion(), Some(WORK), None).await;
    let first = say(&router, &session, "first").await;
    ended(&router, &session, first).await;
    put(&router, "a.txt", "two");
    put(&router, "b.txt", "made since");

    let plan = planned(&router, &session, 1).await;
    assert_eq!(
        (&plan.changed, &plan.removed),
        (&vec![file("a.txt")], &vec![file("b.txt")])
    );
    let call = restore_call(&session, 1, &plan.digest.0);
    let Preview::Facts(lines) = router
        .checkpoints_dry_run(call.clone())
        .await
        .expect("the sheet")
    else {
        panic!("the sheet is facts");
    };
    let titles: Vec<&str> = lines.iter().map(|l| l.label.as_str()).collect();
    assert_eq!(titles[0], "Put back older versions");
    assert!(titles.contains(&"Delete files made since"));

    router.checkpoints_perform(call).await.expect("restore");
    assert_eq!(read(&router, "a.txt").as_deref(), Some("one"));
    assert_eq!(read(&router, "b.txt"), None);
    let rows = listed(&router, &session).await.rows;
    assert!(matches!(
        rows.as_slice(),
        [
            CheckpointRow::Saved {
                id: CheckpointId(1),
                ..
            },
            CheckpointRow::Saved {
                id: CheckpointId(2),
                state: SavedState::Available,
                ..
            },
            CheckpointRow::RestoredTo {
                id: CheckpointId(1),
                ..
            },
        ]
    ));
    // The safety point holds the files as they were before the restore.
    let undo = planned(&router, &session, 2).await;
    assert_eq!(undo.changed, [file("a.txt")]);
    assert_eq!(undo.added, [file("b.txt")]);
}

#[tokio::test]
async fn a_plan_that_is_not_the_one_the_person_saw_is_refused_and_nothing_is_written() {
    let router = router();
    put(&router, "a.txt", "one");
    let session = open_in(&router, &companion(), Some(WORK), None).await;
    let first = say(&router, &session, "first").await;
    ended(&router, &session, first).await;
    put(&router, "a.txt", "two");
    let plan = planned(&router, &session, 1).await;
    put(&router, "a.txt", "three, after the sheet was drawn");
    let call = restore_call(&session, 1, &plan.digest.0);
    assert_eq!(
        router.checkpoints_dry_run(call.clone()).await,
        Err(stale_words())
    );
    assert_eq!(router.checkpoints_perform(call).await, Err(stale_words()));
    assert_eq!(
        read(&router, "a.txt").as_deref(),
        Some("three, after the sheet was drawn")
    );
    // No safety point was kept for a restore that did not happen.
    assert_eq!(listed(&router, &session).await.rows.len(), 1);
    // A point that was cleared is told as cleared.
    let held = router.seams.checkpoints.held(&root(), &session).await;
    let ids: Vec<CheckpointId> = held.expect("held").iter().map(|s| s.id).collect();
    let drop = DropAsk {
        root: root(),
        session: session.clone(),
        ids,
    };
    assert_eq!(
        router.seams.checkpoints.drop_points(drop).await,
        Ok(Count(1))
    );
    let gone = restore_call(&session, 1, &plan.digest.0);
    assert!(router.checkpoints_perform(gone).await.is_err());
}

#[tokio::test]
async fn an_agent_cannot_list_plan_or_restore_and_a_surface_sees_only_its_own_sessions() {
    let router = router();
    let session = open_in(&router, &companion(), Some(WORK), None).await;
    say(&router, &session, "first").await;
    for agent in [
        caller("org.zed.Zed", CallerRole::Mcp),
        caller("org.quire.AcpAgent", CallerRole::AcpAgent),
    ] {
        for request in [
            IntentsRequest::CheckpointList {
                session: session.clone(),
            },
            IntentsRequest::CheckpointPlan {
                session: session.clone(),
                id: CheckpointId(1),
            },
        ] {
            assert_eq!(
                ask(&router, &agent, request).await,
                IntentsReply::Refused(WireRefusal::NotAllowed),
                "{agent:?}"
            );
        }
        let call = restore_call(&session, 1, "0");
        for request in [
            IntentsRequest::Perform {
                call: CallRequest {
                    action: ActionRef {
                        app: app(CHECKPOINTS_APP),
                        name: call.action.clone(),
                    },
                    target: TargetValue::Nothing,
                    args: call.args.clone(),
                    origin: Origin::Mcp,
                },
                session: None,
                parent_window: None,
                activation: None,
            },
            IntentsRequest::DryRun {
                call: CallRequest {
                    action: ActionRef {
                        app: app(CHECKPOINTS_APP),
                        name: call.action.clone(),
                    },
                    target: TargetValue::Nothing,
                    args: call.args.clone(),
                    origin: Origin::Mcp,
                },
                session: None,
            },
        ] {
            assert_eq!(
                ask(&router, &agent, request).await,
                IntentsReply::Refused(WireRefusal::NotAllowed),
                "{agent:?}"
            );
        }
    }
    // An editor that did not open the session is told it does not exist.
    let zed = caller("org.zed.Zed", CallerRole::Editor);
    let reply = ask(
        &router,
        &zed,
        IntentsRequest::CheckpointList {
            session: session.clone(),
        },
    )
    .await;
    assert_eq!(reply, IntentsReply::Refused(WireRefusal::NoSuchSession));
}

/// How a running turn stops being one, or does not.
#[derive(Clone, Copy)]
enum Way {
    /// The host reports that turn.
    Reported,
    /// The host reports a turn that is not the running one.
    ReportedOther,
    /// The session closes.
    Closed,
    /// The turn has run exactly as long as `turn_max_s`.
    AtTheLimit,
    /// The turn has run longer than `turn_max_s`.
    PastTheLimit,
}

/// Whether a restore may go ahead after `Way`.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Then {
    Allowed,
    Refused,
}

#[tokio::test]
async fn a_restore_is_refused_while_a_turn_runs_and_allowed_once_it_has_ended() {
    let rows = [
        (Way::Reported, Then::Allowed),
        (Way::ReportedOther, Then::Refused),
        (Way::Closed, Then::Allowed),
        (Way::AtTheLimit, Then::Refused),
        (Way::PastTheLimit, Then::Allowed),
    ];
    for (index, (way, then)) in rows.into_iter().enumerate() {
        let router = router();
        put(&router, "a.txt", "one");
        let session = open_in(&router, &companion(), Some(WORK), None).await;
        let turn = say(&router, &session, "go").await;
        put(&router, "a.txt", "two");
        let plan = planned(&router, &session, 1).await;
        let call = restore_call(&session, 1, &plan.digest.0);
        assert_eq!(
            router.checkpoints_dry_run(call.clone()).await,
            Err(running_words()),
            "row {index}: the sheet"
        );
        assert_eq!(
            router.checkpoints_perform(call.clone()).await,
            Err(running_words()),
            "row {index}: the restore"
        );
        assert_eq!(
            read(&router, "a.txt").as_deref(),
            Some("two"),
            "row {index}"
        );
        match way {
            Way::Reported => ended(&router, &session, turn).await,
            Way::ReportedOther => ended(&router, &session, TurnId(turn.0 + 100)).await,
            Way::Closed => {
                let close = IntentsRequest::SessionClose {
                    session: session.clone(),
                };
                assert_eq!(ask(&router, &launcher(), close).await, IntentsReply::Done);
            }
            Way::AtTheLimit => router.seams.clock.pass(Seconds(1800)),
            Way::PastTheLimit => router.seams.clock.pass(Seconds(1801)),
        }
        let performed = router.checkpoints_perform(call).await;
        match then {
            Then::Allowed => assert!(performed.is_ok(), "row {index}: {performed:?}"),
            Then::Refused => assert_eq!(performed, Err(running_words()), "row {index}"),
        }
    }
}

#[tokio::test]
async fn the_list_says_running_from_the_turn_until_it_ends() {
    let router = router();
    let session = open_in(&router, &companion(), Some(WORK), None).await;
    assert_eq!(listed(&router, &session).await.turn, TurnState::Idle);
    let turn = say(&router, &session, "go").await;
    assert_eq!(listed(&router, &session).await.turn, TurnState::Running);
    ended(&router, &session, turn).await;
    assert_eq!(listed(&router, &session).await.turn, TurnState::Idle);
}

#[tokio::test]
async fn only_the_opener_of_a_session_ends_its_turn_and_an_agent_cannot() {
    let router = router();
    let external = ExternalAgent {
        program: program("claude-code"),
        sheets: SheetSurface::Desktop,
        label: None,
        rewind: Rewind::Agent,
    };
    let session = open_in(&router, &host(), Some(WORK), Some(external)).await;
    let turn = say_as(&router, &host(), &session, "go").await;
    let end = IntentsRequest::SessionTurnEnded {
        session: session.clone(),
        turn,
        how: TurnEnd::Failed,
    };
    let other = caller("org.zed.Zed", CallerRole::Editor);
    assert_eq!(
        ask(&router, &other, end.clone()).await,
        IntentsReply::Refused(WireRefusal::NotAllowed)
    );
    let mcp = caller("org.zed.Zed", CallerRole::Mcp);
    assert_eq!(
        ask(&router, &mcp, end.clone()).await,
        IntentsReply::Refused(WireRefusal::NotAllowed)
    );
    assert_eq!(listed(&router, &session).await.turn, TurnState::Running);
    assert_eq!(ask(&router, &host(), end).await, IntentsReply::Done);
    assert_eq!(listed(&router, &session).await.turn, TurnState::Idle);
}
