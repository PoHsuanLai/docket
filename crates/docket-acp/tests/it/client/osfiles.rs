//! The real file system under a scratch directory: links that lead out of the session's
//! directory are refused, by the path check and again at the open.

use super::agent::{Act, agent, call};
use super::rig::{Real, abs, opening, program, read, session, write};
use crate::support::Fixed;
use docket_acp::Answer;
use docket_acp::client::fake::{FakeAsk, FakeSpawn};
use docket_acp::client::{AcpBackend, FileFault, Files, OsFiles, Parts};
use docket_session::{SessionBackend, StartSession};
use docket_shell::fake::FakeSandbox;
use std::os::unix::fs::symlink;

#[tokio::test]
async fn a_link_inside_the_directory_cannot_lead_a_read_or_a_write_out() {
    let root = tempfile::tempdir().expect("scratch");
    let app = root.path().join("work/app");
    let outside = root.path().join("work/outside");
    std::fs::create_dir_all(&app).expect("app");
    std::fs::create_dir_all(&outside).expect("outside");
    std::fs::write(outside.join("secret.txt"), "TOP SECRET").expect("secret");
    std::fs::write(app.join("real.txt"), "fine").expect("real");
    symlink(outside.join("secret.txt"), app.join("leak")).expect("file link");
    symlink(&outside, app.join("dirlink")).expect("dir link");
    let at = |rel: &str| app.join(rel).to_str().expect("utf8").to_owned();

    let (wire, view) = agent(vec![vec![
        call("real", "fs/read_text_file", read(&at("real.txt"))),
        call("leak", "fs/read_text_file", read(&at("leak"))),
        call(
            "via_dir",
            "fs/read_text_file",
            read(&at("dirlink/secret.txt")),
        ),
        call(
            "put_dir",
            "fs/write_text_file",
            write(&at("dirlink/new.txt"), "evil"),
        ),
        call(
            "put_in",
            "fs/write_text_file",
            write(&at("made.txt"), "one"),
        ),
        call(
            "put_again",
            "fs/write_text_file",
            write(&at("made.txt"), "two"),
        ),
        Act::Stop("end_turn"),
    ]]);
    let (spawn, _seen) = FakeSpawn::new(vec![wire]);
    let (sandbox, _) = FakeSandbox::ready(Vec::new());
    let mut backend: AcpBackend<Real> = AcpBackend::new(Parts {
        program: program(),
        session: session(),
        spawn,
        files: OsFiles,
        ask: FakeAsk::new(vec![Answer::Once, Answer::Once]),
        sandbox,
        ticks: Fixed,
        grants: Vec::new(),
    });
    backend
        .start(StartSession {
            session: session(),
            opening: opening(app.to_str().expect("utf8")),
        })
        .await
        .expect("start");
    backend.turn(super::rig::turn(1, "go")).await.expect("turn");
    while backend.next_event().await.is_some() {}

    assert_eq!(view.reply("real").expect("read")["content"], "fine");
    assert!(view.reply("leak").is_err(), "a link to a file outside");
    assert!(
        view.reply("via_dir").is_err(),
        "a link to a directory outside"
    );
    assert!(view.reply("put_dir").is_err());
    assert!(
        !outside.join("new.txt").exists(),
        "nothing was created outside"
    );
    assert_eq!(
        std::fs::read_to_string(outside.join("secret.txt")).expect("kept"),
        "TOP SECRET"
    );
    assert!(view.reply("put_in").is_ok());
    assert_eq!(
        std::fs::read_to_string(app.join("made.txt")).expect("made"),
        "two"
    );
    // The undo notes kept what each write replaced.
    let undo = backend.undo_notes();
    assert_eq!(undo.len(), 2);
    assert_eq!(undo[0].before, None);
    assert_eq!(undo[1].before.as_deref(), Some("one"));
}

#[test]
fn a_descriptor_that_points_outside_after_the_open_is_caught() {
    // The check at the open: the file's real place must lie inside the directory named.
    let root = tempfile::tempdir().expect("scratch");
    let inside = root.path().join("in");
    let elsewhere = root.path().join("else");
    std::fs::create_dir_all(&inside).expect("in");
    std::fs::create_dir_all(&elsewhere).expect("else");
    std::fs::write(elsewhere.join("f.txt"), "x").expect("f");
    let file = abs(elsewhere.join("f.txt").to_str().expect("utf8"));
    let within = abs(inside.to_str().expect("utf8"));
    assert_eq!(OsFiles.read(&file, &within), Err(FileFault::Moved));
    assert_eq!(OsFiles.write(&file, &within, "y"), Err(FileFault::Moved));
    assert_eq!(
        std::fs::read_to_string(elsewhere.join("f.txt")).expect("kept"),
        "x"
    );
}
