//! The durable file helpers: a write replaces the file whole or not at all, and a missing file
//! is told apart from an unreadable one. Every file is in a scratch directory.

use docket_store::{read_optional, write_atomic};
use std::path::Path;

#[test]
fn a_write_creates_the_directory_and_replaces_the_file_whole() {
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().join("a/b/grants.json");
    write_atomic(&path, b"first").expect("create");
    write_atomic(&path, b"second").expect("replace");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "second");
    let beside = std::fs::read_dir(path.parent().expect("dir"))
        .expect("list")
        .count();
    assert_eq!(beside, 1, "no temporary file stays beside the target");
}

#[test]
fn a_failed_write_keeps_what_was_there() {
    let dir = tempfile::tempdir().expect("scratch");
    // The target is a directory: the rename cannot replace it.
    let target = dir.path().join("target");
    std::fs::create_dir(&target).expect("dir");
    assert!(write_atomic(&target, b"new").is_err());
    assert!(target.is_dir());
}

#[test]
fn a_path_without_a_file_name_is_refused() {
    assert!(write_atomic(Path::new("/"), b"x").is_err());
}

#[test]
fn a_missing_file_is_none_and_an_unreadable_one_is_an_error() {
    let dir = tempfile::tempdir().expect("scratch");
    assert!(matches!(read_optional(&dir.path().join("none")), Ok(None)));
    std::fs::write(dir.path().join("here"), "text").expect("write");
    assert!(matches!(
        read_optional(&dir.path().join("here")),
        Ok(Some(text)) if text == "text"
    ));
    // A directory is there but cannot be read as text: an error, not "nothing".
    assert!(read_optional(dir.path()).is_err());
}
