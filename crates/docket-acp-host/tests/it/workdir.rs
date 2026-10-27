//! The confined working directory: a plain folder inside the session is accepted; a link, the
//! home folder, a folder that holds home, a place of secrets and an outside folder are refused.
//! Scratch directories only.

use docket_acp_host::{Workdir, WorkdirFault};
use docket_core::AbsPath;
use std::os::unix::fs::symlink;
use std::path::Path;

fn abs(path: &Path) -> AbsPath {
    AbsPath::try_from(path).expect("absolute scratch path")
}

#[test]
fn only_a_plain_folder_inside_the_session_is_confined() {
    let scratch = tempfile::tempdir().expect("scratch");
    let root = std::fs::canonicalize(scratch.path()).expect("real scratch");
    let (home, proj) = (root.join("home"), root.join("home/proj"));
    for dir in [proj.join("sub"), proj.join(".ssh"), root.join("elsewhere")] {
        std::fs::create_dir_all(&dir).expect("dir");
    }
    // The agent's planted links: one to home, one to a folder outside the session.
    symlink(&home, proj.join("to-home")).expect("link");
    symlink(root.join("elsewhere"), proj.join("to-out")).expect("link");
    std::fs::write(proj.join("file"), "x").expect("file");

    let (scope, home_arg) = (abs(&proj), Some(home.as_path()));
    let sub = abs(&proj.join("sub"));
    let ok = Workdir::resolve(&scope, &sub, home_arg).expect("a plain subfolder");
    assert_eq!(ok.real(), &sub);

    let refused =
        |asked: &Path| Workdir::resolve(&scope, &abs(asked), home_arg).expect_err("refused");
    assert_eq!(refused(&proj.join("to-home")), WorkdirFault::Link);
    assert_eq!(refused(&proj.join("to-out")), WorkdirFault::Link);
    assert_eq!(refused(&proj.join("to-home/proj")), WorkdirFault::Link);
    assert_eq!(refused(&proj.join(".ssh")), WorkdirFault::Secret);
    assert_eq!(refused(&proj.join("file")), WorkdirFault::NotFolder);
    assert_eq!(refused(&proj.join("missing")), WorkdirFault::NotFolder);
    assert_eq!(refused(&root.join("elsewhere")), WorkdirFault::Outside);
    // The session folder is home itself, or home is below it.
    assert_eq!(
        Workdir::resolve(&abs(&home), &abs(&home), home_arg).expect_err("home"),
        WorkdirFault::Home
    );
    assert_eq!(
        Workdir::resolve(&abs(&root), &abs(&root), home_arg).expect_err("above home"),
        WorkdirFault::Home
    );
}
