//! `plan_restore`: the split into changed, added and removed, and the digest.

use crate::support::*;
use docket_checkpoint::plan_restore;

#[test]
fn files_are_split_into_changed_added_and_removed() {
    // (name, now, then, changed, added, removed)
    type Files<'a> = &'a [(&'a str, &'a str)];
    type Paths<'a> = &'a [&'a str];
    type Row<'a> = (
        &'a str,
        Files<'a>,
        Files<'a>,
        Paths<'a>,
        Paths<'a>,
        Paths<'a>,
    );
    let rows: [Row; 5] = [
        ("nothing moved", &[("a", "1")], &[("a", "1")], &[], &[], &[]),
        (
            "content differs",
            &[("a", "2"), ("b", "1")],
            &[("a", "1"), ("b", "1")],
            &["a"],
            &[],
            &[],
        ),
        (
            "missing now",
            &[("a", "1")],
            &[("a", "1"), ("dir/b", "9")],
            &[],
            &["dir/b"],
            &[],
        ),
        (
            "made since",
            &[("a", "1"), ("z", "3"), ("c", "4")],
            &[("a", "1")],
            &[],
            &[],
            &["c", "z"],
        ),
        (
            "all three, sorted by path",
            &[("m", "1"), ("k", "2"), ("x", "3")],
            &[("m", "9"), ("a", "4"), ("b", "5")],
            &["m"],
            &["a", "b"],
            &["k", "x"],
        ),
    ];
    for (name, now, then, changed, added, removed) in rows {
        let plan = plan_restore(&listing(now), &listing(then));
        assert_eq!(plan.changed, paths(changed), "{name}");
        assert_eq!(plan.added, paths(added), "{name}");
        assert_eq!(plan.removed, paths(removed), "{name}");
        assert_eq!(plan.digest.0.len(), 64, "{name}: SHA-256 as hex");
    }
}

#[test]
fn the_digest_is_stable_under_order_and_differs_on_any_change() {
    let base = plan_restore(
        &listing(&[("a", "1"), ("b", "2"), ("c", "3")]),
        &listing(&[("a", "9"), ("d", "4")]),
    );
    let reordered = plan_restore(
        &listing(&[("c", "3"), ("a", "1"), ("b", "2")]),
        &listing(&[("d", "4"), ("a", "9")]),
    );
    assert_eq!(base, reordered);

    let now = &[("a", "1"), ("b", "2"), ("c", "3")][..];
    let then = &[("a", "9"), ("d", "4")][..];
    // Each variant changes one thing a person would care about.
    type Files<'a> = &'a [(&'a str, &'a str)];
    let variants: [(&str, Files, Files); 6] = [
        (
            "another file is changed",
            &[("a", "1"), ("b", "2"), ("c", "3"), ("e", "5")],
            then,
        ),
        ("a file made since is gone", &[("a", "1"), ("b", "2")], then),
        ("the saved content differs", now, &[("a", "8"), ("d", "4")]),
        (
            "the current content differs",
            &[("a", "7"), ("b", "2"), ("c", "3")],
            then,
        ),
        (
            "a file is moved",
            &[("a", "1"), ("b", "2"), ("c", "3")],
            &[("a", "9"), ("e", "4")],
        ),
        ("nothing to do", &[("a", "1")], &[("a", "1")]),
    ];
    for (name, now, then) in variants {
        let other = plan_restore(&listing(now), &listing(then));
        assert_ne!(other.digest, base.digest, "{name}");
    }
}

#[test]
fn paths_that_concatenate_alike_do_not_share_a_digest() {
    let one = plan_restore(&listing(&[]), &listing(&[("a", "x"), ("bc", "y")]));
    let two = plan_restore(&listing(&[]), &listing(&[("ab", "x"), ("c", "y")]));
    assert_ne!(one.digest, two.digest);
}
