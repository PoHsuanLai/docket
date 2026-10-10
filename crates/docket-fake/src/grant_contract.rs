//! What every consent-file store must do, as one table a host runs over its own store.
//!
//! Both the daemon's store and an app's in-process store keep the person's consent in a file
//! and share the disk rules in `docket_core::GrantFileError`'s module. A host calls
//! [`check_grant_file_contract`] with a way to open its store at a path and the name of the file
//! its standing grants live in; what is particular to a host (its typed errors, its defaults,
//! what a failed write is kept as) stays in the host's own tests. Test code: it panics on a
//! failed check.

use docket_core::{
    AbsPath, ActionGrant, ActionGrantKey, ActionRef, GrantCaller, GrantTarget, Revocation,
    StandingGrant, StandingScope,
};
use docket_router::GrantStore;
use porter_core::consent::{Decision, Grant, GrantScope, Usage};
use porter_core::{AppName, DataClass, GrantId};
use prov::{ActionName, SpaceScope, UnixSeconds};
use std::path::Path;

/// A class grant for `action`, to be recorded or compared.
pub fn contract_grant(id: &str, action: &str, scope: GrantScope) -> ActionGrant {
    Grant {
        id: GrantId::parse(id).expect("id"),
        key: ActionGrantKey {
            caller: GrantCaller::Companion,
            owner: AppName::parse("org.quire.Mail").expect("app"),
            target: GrantTarget::Action(ActionName::parse(action).expect("action")),
            class: DataClass::Mail,
            usage: Usage::Interactive,
            space: SpaceScope::Any,
        },
        decision: Decision::Allow,
        scope,
        at: UnixSeconds(1),
    }
}

/// A standing grant to move files under `prefix`.
pub fn contract_standing(prefix: &str) -> StandingGrant {
    StandingGrant::new(
        GrantCaller::Companion,
        StandingScope::Files {
            action: ActionRef {
                app: AppName::parse("org.quire.Files").expect("app"),
                name: ActionName::parse("files.file.move").expect("action"),
            },
            under: AbsPath::parse(prefix).expect("path"),
        },
        UnixSeconds(1),
    )
}

/// Runs every shared check in a scratch `dir` the caller made. `open` makes the store for a
/// grants file path; `standing_file` is the name, beside it, of the file standing grants live in.
pub fn check_grant_file_contract<S: GrantStore>(
    dir: &Path,
    open: impl Fn(&Path) -> S,
    standing_file: &str,
) {
    restart_and_replace(&dir.join("restart/quire/grants.json"), &open);
    two_stores_one_file(&dir.join("two/grants.json"), &open);
    damaged_grants_are_never_written_over(&dir.join("damaged/grants.json"), &open);
    standing_survives_and_revokes(
        &dir.join("standing/quire/grants.json"),
        standing_file,
        &open,
    );
    damaged_standing_is_never_written_over(&dir.join("hurt/grants.json"), standing_file, &open);
    unreadable_standing_is_not_empty(&dir.join("blocked/grants.json"), standing_file, &open);
}

fn restart_and_replace<S: GrantStore>(path: &Path, open: &impl Fn(&Path) -> S) {
    let store = open(path);
    assert!(store.grants().is_empty(), "no file is no grants");
    store.record(contract_grant(
        "g-1",
        "mail.thread.archive",
        GrantScope::Once,
    ));
    store.record(contract_grant(
        "g-2",
        "mail.draft.create",
        GrantScope::Always,
    ));
    let reopened = open(path);
    assert_eq!(reopened.grants().len(), 2);
    reopened.record(contract_grant(
        "g-3",
        "mail.thread.archive",
        GrantScope::Always,
    ));
    let all = reopened.grants();
    assert_eq!(all.len(), 2, "the same key is one grant: {all:?}");
    assert!(
        all.iter()
            .any(|g| g.id.as_str() == "g-3" && g.scope == GrantScope::Always)
    );
    assert!(
        !path.with_extension("json.tmp").exists(),
        "the temporary file is renamed away"
    );
}

fn two_stores_one_file<S: GrantStore>(path: &Path, open: &impl Fn(&Path) -> S) {
    let (a, b) = (open(path), open(path));
    a.record(contract_grant(
        "g-1",
        "mail.thread.archive",
        GrantScope::Always,
    ));
    b.record(contract_grant(
        "g-2",
        "mail.draft.create",
        GrantScope::Always,
    ));
    assert_eq!(a.grants().len(), 2, "b's grant did not overwrite a's");
    assert_eq!(b.grants().len(), 2);
}

fn damaged_grants_are_never_written_over<S: GrantStore>(path: &Path, open: &impl Fn(&Path) -> S) {
    std::fs::create_dir_all(path.parent().expect("dir")).expect("dir");
    for text in ["{", "{\"not\": \"a list\"}", "[{\"id\": 3}]"] {
        // The store is open before the file is damaged: a store may refuse to open a damaged
        // file, which its own tests cover.
        std::fs::remove_file(path).ok();
        let store = open(path);
        std::fs::write(path, text).expect("write");
        // A new decision does not replace the file: it may hold every grant the person gave.
        store.record(contract_grant(
            "g-1",
            "mail.thread.archive",
            GrantScope::Always,
        ));
        assert_eq!(std::fs::read_to_string(path).expect("read"), text);
    }
    // A blank file is no grants, and a decision is kept.
    std::fs::write(path, "  \n").expect("write");
    let store = open(path);
    store.record(contract_grant(
        "g-1",
        "mail.thread.archive",
        GrantScope::Always,
    ));
    assert_eq!(store.grants().len(), 1);
}

fn standing_survives_and_revokes<S: GrantStore>(
    path: &Path,
    standing_file: &str,
    open: &impl Fn(&Path) -> S,
) {
    let store = open(path);
    assert!(store.standing().is_empty());
    let (a, b) = (contract_standing("/w/a"), contract_standing("/w/b"));
    store.add_standing(a.clone());
    store.add_standing(b.clone());
    store.add_standing(a.clone());
    let reopened = open(path);
    assert_eq!(reopened.standing(), vec![b.clone(), a.clone()]);
    assert_eq!(reopened.revoke_standing(&a.id), Revocation::Revoked);
    assert_eq!(reopened.revoke_standing(&a.id), Revocation::NotHeld);
    assert_eq!(
        open(path).standing(),
        vec![b],
        "a third process reads the revocation"
    );
    assert!(
        store.grants().is_empty(),
        "class grants are a separate list"
    );
    let folder = path.parent().expect("dir");
    assert!(
        !folder.join(format!("{standing_file}.tmp")).exists(),
        "no temporary file is left"
    );
}

fn damaged_standing_is_never_written_over<S: GrantStore>(
    path: &Path,
    standing_file: &str,
    open: &impl Fn(&Path) -> S,
) {
    let damaged = path.with_file_name(standing_file);
    std::fs::create_dir_all(path.parent().expect("dir")).expect("dir");
    let store = open(path);
    for text in ["{", "[1]"] {
        std::fs::write(&damaged, text).expect("damage");
        assert!(store.standing().is_empty(), "{text:?}");
        let held = contract_standing("/w/a");
        store.add_standing(held.clone());
        assert_eq!(std::fs::read_to_string(&damaged).expect("read"), text);
        assert_eq!(store.revoke_standing(&held.id), Revocation::NotHeld);
        assert_eq!(std::fs::read_to_string(&damaged).expect("read"), text);
    }
}

fn unreadable_standing_is_not_empty<S: GrantStore>(
    path: &Path,
    standing_file: &str,
    open: &impl Fn(&Path) -> S,
) {
    // A directory where the file belongs: it exists and cannot be read as text.
    let blocker = path.with_file_name(standing_file);
    std::fs::create_dir_all(&blocker).expect("blocker");
    let store = open(path);
    store.add_standing(contract_standing("/w/a"));
    assert!(blocker.is_dir(), "left alone");
    assert!(store.standing().is_empty());
}
