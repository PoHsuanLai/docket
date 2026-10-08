//! A policy's `paths` bound the files a call targets: whole components, outside is outside,
//! an empty list leaves files alone, and the lists compare and intersect without a hole.

use crate::support::{action, app};
use crate::task_policy::{declared, edit, file, labels, policy};
use docket_core::*;
use prov::Effect;

fn files_call(paths: &[&str]) -> CallRequest {
    CallRequest {
        action: ActionRef {
            app: app("org.quire.Mail"),
            name: action("mail.thread.archive"),
        },
        target: TargetValue::Files(paths.iter().map(|p| file(p)).collect()),
        args: Default::default(),
        origin: Origin::Companion,
    }
}

fn under(path: &str) -> TrustedPattern {
    TrustedPattern::Under(file(path))
}

fn outside(path: &str) -> Coverage {
    Coverage::Outside(Widening::Pattern(
        docket_core::ArgSink::Path,
        TrustedPattern::Exact(Value::File(file(path))),
    ))
}

fn covered(policy: &TaskPolicy, call: &CallRequest) -> Coverage {
    let decl = declared(call, Effect::UndoableWrite);
    covers(policy, &decl, call, &labels(vec![]))
}

#[test]
fn covers_checks_every_file_target_against_the_paths() {
    let tests_only = edit(|p| p.paths = vec![under("/home/u/proj/tests")]);
    let cases = [
        (
            "inside",
            &tests_only,
            files_call(&["/home/u/proj/tests/a.rs"]),
            Coverage::Inside,
        ),
        (
            "the folder itself",
            &tests_only,
            files_call(&["/home/u/proj/tests"]),
            Coverage::Inside,
        ),
        (
            "a sibling with the same prefix",
            &tests_only,
            files_call(&["/home/u/proj/tests-old/a.rs"]),
            outside("/home/u/proj/tests-old/a.rs"),
        ),
        (
            "outside",
            &tests_only,
            files_call(&["/home/u/proj/src/a.rs"]),
            outside("/home/u/proj/src/a.rs"),
        ),
        (
            "one of two is outside",
            &tests_only,
            files_call(&["/home/u/proj/tests/a.rs", "/home/u/proj/src/b.rs"]),
            outside("/home/u/proj/src/b.rs"),
        ),
        (
            "a climb out of the folder",
            &tests_only,
            files_call(&["/home/u/proj/tests/../src/a.rs"]),
            outside("/home/u/proj/tests/../src/a.rs"),
        ),
    ];
    for (name, policy, call, want) in cases {
        assert_eq!(covered(policy, &call), want, "case: {name}");
    }
    assert_eq!(
        covered(&policy(), &files_call(&["/home/u/proj/src/a.rs"])),
        Coverage::Inside,
        "empty paths leave files as they were"
    );
}

#[test]
fn added_paths_widen_and_removed_paths_narrow() {
    let one = edit(|p| p.paths = vec![under("/home/u/proj/tests")]);
    let two = edit(|p| p.paths = vec![under("/home/u/proj/tests"), under("/home/u/proj/docs")]);
    assert_eq!(
        compare(&two, &one),
        PolicyChange::Widens(vec![Widening::Pattern(
            ArgSink::Path,
            under("/home/u/proj/docs")
        )])
    );
    assert_eq!(compare(&one, &two), PolicyChange::Narrows);
}

#[test]
fn dropping_every_path_widens_because_files_are_then_unbounded() {
    let one = edit(|p| p.paths = vec![under("/home/u/proj/tests")]);
    assert!(matches!(
        compare(&policy(), &one),
        PolicyChange::Widens(w) if w == vec![Widening::Pattern(ArgSink::Path, under("/"))]
    ));
}

#[test]
fn a_child_of_a_bounded_policy_stays_bounded() {
    let bounded = edit(|p| p.paths = vec![under("/home/u/proj/tests")]);
    for (a, b) in [(&bounded, &policy()), (&policy(), &bounded)] {
        assert_eq!(intersection(a, b).paths, bounded.paths);
    }
    let narrower = edit(|p| p.paths = vec![under("/home/u/proj/tests/unit")]);
    assert_eq!(
        intersection(&bounded, &narrower).paths,
        narrower.paths,
        "two lists keep what each covers of the other"
    );
}
