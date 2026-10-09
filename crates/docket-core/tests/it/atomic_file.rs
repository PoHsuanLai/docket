//! The durable file helpers: a write replaces the file whole or not at all, and a missing file
//! is told apart from an unreadable one. Every file is in a scratch directory.

use docket_core::{AbsPath, PathFault, StandingFileFault, decode_standing};
#[cfg(feature = "atomic-file")]
use docket_core::{read_optional, write_atomic};
use std::path::Path;

#[cfg(feature = "atomic-file")]
#[test]
fn a_write_creates_the_directory_and_replaces_the_file_whole() {
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().join("a/b/grants.json");
    write_atomic(&path, b"first").expect("create");
    write_atomic(&path, b"second").expect("replace");
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "second");
    assert!(!path.with_file_name("grants.json.tmp").exists());
}

#[cfg(feature = "atomic-file")]
#[test]
fn a_failed_write_keeps_the_old_content_and_leaves_no_temporary() {
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().join("grants.json");
    write_atomic(&path, b"old").expect("create");
    // A directory where the temporary file belongs: it cannot be created.
    let temporary = dir.path().join("grants.json.tmp");
    std::fs::create_dir(&temporary).expect("blocker");
    assert!(write_atomic(&path, b"new").is_err());
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "old");
    // A rename that fails (the target is a directory) also keeps the target and cleans up.
    std::fs::remove_dir(&temporary).expect("unblock");
    let target = dir.path().join("target");
    std::fs::create_dir(&target).expect("dir");
    assert!(write_atomic(&target, b"new").is_err());
    assert!(target.is_dir());
    assert!(!dir.path().join("target.tmp").exists());
}

#[cfg(feature = "atomic-file")]
#[test]
fn a_path_without_a_file_name_is_refused() {
    assert!(write_atomic(Path::new("/"), b"x").is_err());
}

#[cfg(feature = "atomic-file")]
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

#[test]
fn a_damaged_standing_text_is_a_typed_fault_not_an_empty_list() {
    for text in ["{", "[1]", "{\"not\": \"a list\"}"] {
        assert!(
            matches!(decode_standing(text), Err(StandingFileFault::Decode(_))),
            "{text:?}"
        );
    }
}

#[test]
fn a_path_that_is_not_utf8_is_a_fault_not_a_lossy_name() {
    use std::os::unix::ffi::OsStrExt;
    let bad = Path::new(std::ffi::OsStr::from_bytes(b"/w/\xff"));
    assert_eq!(AbsPath::try_from(bad), Err(PathFault::NotUtf8));
    assert!(AbsPath::try_from(Path::new("/w/p")).is_ok());
    assert_eq!(
        AbsPath::try_from(Path::new("w/p")),
        Err(PathFault::NotAbsolute)
    );
}
