//! Restore points through the real daemons on a private bus: a turn saves one in a scratch
//! folder, the list shows it, the plan names the changed and the made files, the sheet's lines say
//! the same, the person's yes puts the files back, and the list shows the restore. Everything
//! happens in the world's scratch root (its own HOME, its own repository); nothing of the real
//! system is read or written.

use crate::support::*;
use docket_accept::drive::{Launcher, keep_nothing};
use docket_accept::world::{Consent, World};
use docket_core::{
    ActionRef, CallRequest, CheckpointId, CheckpointRow, Origin, Preview, SavedState, SessionOpen,
    TargetValue, TurnEnd, TurnIn, TurnState, TurnVia, Value, WorkPath, Workspace,
};
use porter_core::AppName;
use prov::{ActionName, AgentRef, Label, Labelled, SpaceId};
use std::path::Path;
use std::process::Command;

/// Whether git is there; a run that requires the tools fails without it, any other skips.
fn git_is_here() -> bool {
    let here = Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|out| out.status.success());
    assert!(
        here || std::env::var_os("DOCKET_REQUIRE_TOOLS").is_none(),
        "git is required (DOCKET_REQUIRE_TOOLS is set)"
    );
    here
}

/// `git init` in `dir` with a HOME and a configuration of the world's own.
fn git_init(dir: &Path, home: &Path) {
    let out = Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git init failed");
}

fn file(name: &str) -> WorkPath {
    WorkPath::parse(name).expect("path")
}

fn restore_call(session: &prov::SessionId, point: i64, plan: &str) -> CallRequest {
    let arg = |value: Value| Labelled {
        value,
        label: Label::trusted_user(),
    };
    CallRequest {
        action: ActionRef {
            app: AppName::parse("org.quire.Checkpoints").expect("app"),
            name: ActionName::parse("checkpoints.restore").expect("action"),
        },
        target: TargetValue::Nothing,
        args: [
            ("session", arg(Value::Text(session.as_str().into()))),
            ("point", arg(Value::Integer(point))),
            ("plan", arg(Value::Text(plan.into()))),
        ]
        .into_iter()
        .map(|(name, value)| (docket_core::ParamName::parse(name).expect("param"), value))
        .collect(),
        origin: Origin::Launcher,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_turn_saves_a_point_and_the_person_restores_it_after_the_plan_and_the_sheet() {
    if !git_is_here() {
        return;
    }
    let world = World::start(&binaries(), Consent::Standing, FLOW_A).await;
    let work = world.dir.path().join("project");
    std::fs::create_dir_all(&work).expect("project folder");
    git_init(&work, world.dir.path());
    std::fs::write(work.join("notes.txt"), "first").expect("write");

    let launcher = Launcher::of(&world).await;
    let opened = launcher
        .intents
        .session_open(SessionOpen {
            space: SpaceId::parse("work").expect("space"),
            agent: AgentRef::Companion,
            parent: None,
            cwd: Some(Workspace::parse(&work.to_string_lossy()).expect("workspace")),
            started_from: None,
            external: None,
        })
        .await
        .expect("Session.Open");
    let session = opened.session;
    let turn = launcher
        .intents
        .session_turn(
            session.clone(),
            TurnIn {
                text: "tidy the notes".into(),
                origin: Origin::Launcher,
                keep: keep_nothing(),
                via: TurnVia::Typed,
            },
        )
        .await
        .expect("Session.Turn");

    // The turn saved a point before it was recorded, and the list shows it.
    let list = launcher
        .intents
        .checkpoint_list(session.clone())
        .await
        .expect("Checkpoint.List");
    assert!(
        matches!(
            list.rows.as_slice(),
            [CheckpointRow::Saved {
                id: CheckpointId(1),
                state: SavedState::Available,
                ..
            }]
        ),
        "{:#?}\n{}",
        list.rows,
        world.logs()
    );
    assert_eq!(list.turn, TurnState::Running, "the turn has not ended");

    // The agent changes a file and makes another.
    std::fs::write(work.join("notes.txt"), "second").expect("write");
    std::fs::write(work.join("extra.txt"), "made since").expect("write");
    let plan = launcher
        .intents
        .checkpoint_plan(session.clone(), CheckpointId(1))
        .await
        .expect("Checkpoint.Plan")
        .expect("a plan");
    assert_eq!(plan.changed, [file("notes.txt")]);
    assert_eq!(plan.removed, [file("extra.txt")]);
    assert!(plan.added.is_empty());

    // A restore asked while the turn runs is refused, the sheet as well as the act.
    let call = restore_call(&session, 1, &plan.digest.0);
    let early = launcher.intents.dry_run(call.clone(), None).await;
    assert!(!matches!(early, Ok(Preview::Facts(_))), "{early:?}");
    let early = launcher.intents.perform(call.clone(), None, None).await;
    assert!(!matches!(early, Ok(Ok(_))), "{early:?}");
    assert_eq!(
        std::fs::read_to_string(work.join("notes.txt")).expect("read"),
        "second"
    );

    // The host says the turn is over: the list says so, and the restore can go on.
    launcher
        .intents
        .session_turn_ended(session.clone(), turn, TurnEnd::Answered)
        .await
        .expect("Session.TurnEnded");
    let idle = launcher
        .intents
        .checkpoint_list(session.clone())
        .await
        .expect("Checkpoint.List");
    assert_eq!(idle.turn, TurnState::Idle);

    // The sheet says the same, in the sheet's words.
    let Preview::Facts(lines) = launcher
        .intents
        .dry_run(call.clone(), None)
        .await
        .expect("the sheet")
    else {
        panic!("the sheet is facts");
    };
    let titles: Vec<&str> = lines.iter().map(|line| line.label.as_str()).collect();
    assert_eq!(titles[0], "Put back older versions", "{titles:?}");
    assert!(titles.contains(&"Delete files made since"), "{titles:?}");

    // The person confirms (the shell draws that sheet): the files are put back.
    launcher
        .intents
        .perform(call, None, None)
        .await
        .expect("Run.Perform")
        .expect("restored");
    assert_eq!(
        std::fs::read_to_string(work.join("notes.txt")).expect("read"),
        "first"
    );
    assert!(!work.join("extra.txt").exists());

    // The list shows the restore, and the safety point it saved first.
    let list = launcher
        .intents
        .checkpoint_list(session)
        .await
        .expect("Checkpoint.List");
    assert!(
        matches!(
            list.rows.as_slice(),
            [
                CheckpointRow::Saved {
                    id: CheckpointId(1),
                    ..
                },
                CheckpointRow::Saved {
                    id: CheckpointId(2),
                    ..
                },
                CheckpointRow::RestoredTo {
                    id: CheckpointId(1),
                    ..
                },
            ]
        ),
        "{:#?}\n{}",
        list.rows,
        world.logs()
    );
}
