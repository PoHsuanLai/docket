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

fn standing_grant(prefix: &str) -> docket_core::StandingGrant {
    docket_core::StandingGrant::new(
        GrantCaller::AcpAgent(docket_core::ProgramName::parse("claude-code").expect("program")),
        docket_core::StandingScope::Files {
            action: docket_core::ActionRef {
                app: AppName::parse("org.quire.Files").expect("app"),
                name: ActionName::parse("files.file.move").expect("action"),
            },
            under: docket_core::AbsPath::parse(prefix).expect("path"),
        },
        UnixSeconds(1),
    )
}

fn read_grant() -> docket_core::StandingGrant {
    docket_core::StandingGrant::new(
        GrantCaller::AcpAgent(docket_core::ProgramName::parse("claude-code").expect("program")),
        docket_core::StandingScope::Reads {
            action: docket_core::ActionRef {
                app: AppName::parse("org.quire.Mail").expect("app"),
                name: ActionName::parse("mail.thread.search").expect("action"),
            },
        },
        UnixSeconds(1),
    )
}

#[test]
fn standing_grants_survive_a_restart_and_a_revocation_is_seen_by_the_next_reader() {
    use docket_core::Revocation;
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().join("app/grants.json");
    let store = FileGrantStore::open(path.clone()).expect("open");
    assert!(store.standing().is_empty());
    let (a, b) = (standing_grant("/w/a"), standing_grant("/w/b"));
    store.add_standing(a.clone());
    store.add_standing(b.clone());
    store.add_standing(a.clone());
    let read = read_grant();
    store.add_standing(read.clone());
    let reopened = FileGrantStore::open(path.clone()).expect("open");
    assert_eq!(
        reopened.standing(),
        vec![b.clone(), a.clone(), read.clone()],
        "a read grant survives a restart like the other standing grants"
    );
    assert_eq!(reopened.revoke_standing(&a.id), Revocation::Revoked);
    assert_eq!(reopened.revoke_standing(&a.id), Revocation::NotHeld);
    assert_eq!(
        FileGrantStore::open(path.clone()).expect("open").standing(),
        vec![b.clone(), read.clone()],
        "a third process reads the revocation"
    );
    assert!(
        store.grants().is_empty(),
        "class grants are a separate list"
    );
    std::fs::write(path.with_file_name("grants.json.standing"), "{").expect("damage");
    let reader = FileGrantStore::open(path).expect("open");
    assert!(
        reader.standing().is_empty(),
        "a damaged file holds none: the person is asked again"
    );
    // ... and is not written over: a new grant keeps the fault instead.
    reader.add_standing(standing_grant("/w/c"));
    assert!(matches!(
        reader.take_fault(),
        Some(GrantFileError::Corrupt { .. })
    ));
    assert_eq!(
        std::fs::read_to_string(reader.path().with_file_name("grants.json.standing"))
            .expect("read"),
        "{"
    );
}

#[test]
fn a_store_opened_over_a_damaged_file_never_overwrites_it_but_fresh_does() {
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().join("grants.json");
    std::fs::write(&path, "[]").expect("write");
    let store = FileGrantStore::open(&path).expect("open");
    std::fs::write(&path, "{").expect("damage");
    store.record(grant("g-1", "mail.thread.archive", GrantScope::Always));
    assert!(matches!(
        store.take_fault(),
        Some(GrantFileError::Corrupt { .. })
    ));
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "{");
    assert_eq!(store.grants().len(), 1, "this run still has the consent");
}

#[test]
fn a_failed_write_keeps_the_old_file() {
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().join("grants.json");
    let store = FileGrantStore::open(&path).expect("open");
    store.record(grant("g-1", "mail.thread.archive", GrantScope::Always));
    let before = std::fs::read_to_string(&path).expect("read");
    // A directory where the temporary file belongs: the write cannot start.
    std::fs::create_dir(dir.path().join("grants.json.tmp")).expect("blocker");
    store.record(grant("g-2", "mail.draft.create", GrantScope::Always));
    assert!(matches!(
        store.take_fault(),
        Some(GrantFileError::Write { .. })
    ));
    assert_eq!(std::fs::read_to_string(&path).expect("read"), before);
}
