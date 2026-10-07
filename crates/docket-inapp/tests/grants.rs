//! The consent file: grants survive a restart, one decision per key, a damaged file is a typed
//! error the app decides about, a failed write is kept for the app to ask for, and the file is
//! the one intentd's `FileGrants` reads. Every file is in a scratch directory the test makes.

use docket_core::{ActionGrant, ActionGrantKey, GrantCaller, GrantTarget};
use docket_inapp::{FileGrantStore, GrantFileError};
use docket_router::GrantStore;
use porter_core::consent::{Decision, Grant, GrantScope, Usage};
use porter_core::{AppName, DataClass, GrantId};
use prov::{ActionName, SpaceScope, UnixSeconds};

fn grant(id: &str, action: &str, scope: GrantScope) -> ActionGrant {
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

#[test]
fn grants_survive_a_restart_and_one_decision_per_key() {
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().join("app/grants.json");
    let store = FileGrantStore::open(&path).expect("no file is a fresh store");
    assert!(store.grants().is_empty());
    store.record(grant("g-1", "mail.thread.archive", GrantScope::Once));
    store.record(grant("g-2", "mail.draft.create", GrantScope::Always));
    let reopened = FileGrantStore::open(&path).expect("reopen");
    assert_eq!(reopened.grants().len(), 2);
    reopened.record(grant("g-3", "mail.thread.archive", GrantScope::Always));
    let all = reopened.grants();
    assert_eq!(all.len(), 2, "the same key is one grant: {all:?}");
    assert!(all.iter().any(|g| g.id.as_str() == "g-3"));
    assert_eq!(store.take_fault(), None);
    let names: Vec<_> = std::fs::read_dir(path.parent().expect("dir"))
        .expect("dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert_eq!(names, ["grants.json"], "the temporary file is renamed away");
}

#[test]
fn two_stores_on_one_file_see_each_others_grants() {
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().join("grants.json");
    let (a, b) = (
        FileGrantStore::open(&path).expect("a"),
        FileGrantStore::open(&path).expect("b"),
    );
    a.record(grant("g-1", "mail.thread.archive", GrantScope::Always));
    b.record(grant("g-2", "mail.draft.create", GrantScope::Always));
    assert_eq!(a.grants().len(), 2, "b's grant did not overwrite a's");
    assert_eq!(b.grants().len(), 2);
}

#[test]
fn a_damaged_file_is_a_typed_error_and_fresh_starts_over_it() {
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().join("grants.json");
    for text in ["{", "{\"not\": \"a list\"}", "[{\"id\": 3}]"] {
        std::fs::write(&path, text).expect("write");
        assert!(
            matches!(
                FileGrantStore::open(&path),
                Err(GrantFileError::Corrupt { .. })
            ),
            "{text:?}"
        );
    }
    std::fs::write(&path, "  \n").expect("write");
    assert!(
        FileGrantStore::open(&path)
            .expect("blank")
            .grants()
            .is_empty(),
        "a blank file is no grants"
    );
    std::fs::write(&path, "{").expect("write");
    let fresh = FileGrantStore::fresh(&path);
    assert!(fresh.grants().is_empty());
    fresh.record(grant("g-1", "mail.thread.archive", GrantScope::Always));
    assert_eq!(
        FileGrantStore::open(&path)
            .expect("repaired")
            .grants()
            .len(),
        1,
        "the first grant replaced the damaged file"
    );
}

#[test]
fn a_write_that_fails_is_kept_for_the_app_and_the_grant_still_counts() {
    let dir = tempfile::tempdir().expect("scratch");
    // A file where the directory should be: nothing can be created under it.
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, "").expect("write");
    let store = FileGrantStore::fresh(blocker.join("grants.json"));
    store.record(grant("g-1", "mail.thread.archive", GrantScope::Always));
    assert!(matches!(
        store.take_fault(),
        Some(GrantFileError::Write { .. })
    ));
    assert_eq!(store.take_fault(), None, "asking clears it");
    assert_eq!(store.grants().len(), 1, "this run still has the consent");
}

#[test]
fn the_file_is_a_json_list_of_grants_the_desktop_store_format() {
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().join("grants.json");
    FileGrantStore::open(&path).expect("open").record(grant(
        "g-1",
        "mail.thread.archive",
        GrantScope::Always,
    ));
    let text = std::fs::read_to_string(&path).expect("read");
    let parsed: Vec<ActionGrant> = serde_json::from_str(&text).expect("a list of grants");
    assert_eq!(
        parsed,
        [grant("g-1", "mail.thread.archive", GrantScope::Always)]
    );
}
