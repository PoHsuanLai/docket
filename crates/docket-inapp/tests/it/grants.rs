//! The consent file, past the rules every store shares (`docket_fake::check_grant_file_contract`):
//! a damaged file is a typed
//! error the app decides about, a failed write is kept for the app to ask for, and the file is
//! the one intentd's `FileGrants` reads. Every file is in a scratch directory the test makes.

use docket_core::{ActionGrant, ActionGrantKey, GrantCaller, GrantTarget};
use docket_fake::check_grant_file_contract;
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

#[test]
fn the_consent_file_follows_the_shared_store_rules() {
    let dir = tempfile::tempdir().expect("scratch");
    check_grant_file_contract(
        dir.path(),
        |path| FileGrantStore::open(path).expect("open"),
        "grants.json.standing",
    );
}

#[test]
fn a_read_grant_survives_a_restart_and_a_damaged_standing_file_keeps_its_fault() {
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().join("app/grants.json");
    let store = FileGrantStore::open(path.clone()).expect("open");
    let (a, read) = (standing_grant("/w/a"), read_grant());
    store.add_standing(a.clone());
    store.add_standing(read.clone());
    assert_eq!(
        FileGrantStore::open(path.clone()).expect("open").standing(),
        vec![a, read],
        "a read grant survives a restart like the other standing grants"
    );
    std::fs::write(path.with_file_name("grants.json.standing"), "{").expect("damage");
    let reader = FileGrantStore::open(path).expect("open");
    reader.add_standing(standing_grant("/w/c"));
    assert!(matches!(
        reader.take_fault(),
        Some(GrantFileError::Corrupt { .. })
    ));
}
