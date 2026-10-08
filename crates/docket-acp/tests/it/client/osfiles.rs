use super::agent::{Act, call};
use super::rig::{Real, Setup, abs, once, read, run_turn, started_over, write};
use docket_acp::client::{FileFault, Files, OsFiles};
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

    let mut rig = started_over::<Real>(
        OsFiles,
        app.to_str().expect("utf8"),
        Setup {
            turns: vec![vec![
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
            ]],
            // The file served taints the session, so each write asks.
            answers: vec![once(), once()],
            ..Setup::default()
        },
    )
    .await;
    run_turn(&mut rig, "go").await;
    let view = &rig.agent;

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
    let undo = rig.performer.undo_notes();
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

/// Why: a write the check refuses after the file was opened must not leave the empty file it
/// created behind, and must not touch a file that was already there.
#[test]
fn a_refused_write_leaves_no_new_file_and_spares_an_old_one() {
    let root = tempfile::tempdir().expect("scratch");
    let inside = root.path().join("inside");
    let elsewhere = root.path().join("elsewhere");
    std::fs::create_dir_all(&inside).expect("inside");
    std::fs::create_dir_all(&elsewhere).expect("elsewhere");
    let within = abs(inside.to_str().expect("utf8"));
    let fresh = elsewhere.join("fresh.txt");
    let old = elsewhere.join("old.txt");
    std::fs::write(&old, "kept").expect("old");

    let mut files = OsFiles;
    let outside = |path: &std::path::Path| abs(path.to_str().expect("utf8"));
    assert!(files.write(&outside(&fresh), &within, "evil").is_err());
    assert!(!fresh.exists(), "the empty file is not left behind");
    assert!(files.write(&outside(&old), &within, "evil").is_err());
    assert_eq!(std::fs::read_to_string(&old).expect("old"), "kept");
}
