//! Consent keyed by Space: the file read keeps what porter reads and drops what it does not,
//! a Space that is gone ends its grants, and another app's own Space is never open.

use crate::support::*;
use docket_core::*;
use porter_core::DesktopSpace;
use porter_core::consent::{Decision, Grant, GrantScope, Usage};
use porter_core::{DataClass, GrantId};
use prov::{SpaceId, SpaceScope};

fn grant(id: &str, caller: GrantCaller, scope: SpaceScope) -> ActionGrant {
    Grant {
        id: GrantId::parse(id).expect("id"),
        key: ActionGrantKey {
            caller,
            owner: app("org.quire.Mail"),
            target: GrantTarget::App,
            class: DataClass::Mail,
            usage: Usage::Interactive,
            space: scope,
        },
        decision: Decision::Allow,
        scope: GrantScope::Always,
        at: at(1),
    }
}

fn only(id: &str) -> SpaceScope {
    SpaceScope::Only(space(id))
}

fn known(ids: &[&str]) -> KnownSpaces {
    KnownSpaces::of(
        ids.iter()
            .map(|id| DesktopSpace::parse(id).expect("desktop space")),
    )
}

fn ids(read: &Reconciled) -> Vec<&str> {
    read.kept.iter().map(|g| g.id.as_str()).collect()
}

#[test]
fn an_app_space_is_open_to_its_owner_alone() {
    let mine = app("org.quire.Mail");
    let theirs = app("org.quire.Photos");
    let own = only("app:org.quire.Mail:3");
    assert_eq!(space_access(&own, &mine), SpaceAccess::Open);
    assert_eq!(
        space_access(&own, &theirs),
        SpaceAccess::Refused(SpaceRefusal {
            space: space("app:org.quire.Mail:3"),
            owner: mine.clone(),
        })
    );
    for open in [SpaceScope::Any, only("work"), only("desktop")] {
        assert_eq!(space_access(&open, &theirs), SpaceAccess::Open, "{open}");
    }
    assert!(
        SpaceRefusal {
            space: space("app:org.quire.Mail:3"),
            owner: mine,
        }
        .to_string()
        .contains("another app")
    );
}

#[test]
fn a_grant_for_an_app_over_another_apps_space_is_ended_on_reading() {
    let photos = GrantCaller::App(app("org.quire.Photos"));
    let held = vec![
        grant("g-1", photos.clone(), only("app:org.quire.Photos:1")),
        grant("g-2", photos.clone(), only("app:org.quire.Mail:1")),
        grant("g-3", GrantCaller::Companion, only("app:org.quire.Mail:1")),
    ];
    let read = reconcile(held, None);
    assert_eq!(ids(&read), ["g-1", "g-3"]);
    assert_eq!(
        read.ended,
        [Ended {
            why: GrantEnd::NotTheirSpace(space("app:org.quire.Mail:1")),
            count: 1
        }]
    );
}

#[test]
fn a_space_the_registry_lacks_ends_its_grants_and_only_those() {
    let held = vec![
        grant("g-1", GrantCaller::Companion, only("work")),
        grant("g-2", GrantCaller::Companion, only("home")),
        grant("g-3", GrantCaller::Companion, only("desktop")),
        grant("g-4", GrantCaller::Companion, only("app:org.quire.Mail:2")),
        grant("g-5", GrantCaller::Companion, SpaceScope::Any),
    ];
    let read = reconcile(held, Some(&known(&["work"])));
    assert_eq!(ids(&read), ["g-1", "g-3", "g-4", "g-5"]);
    assert_eq!(
        read.ended,
        [Ended {
            why: GrantEnd::SpaceGone(space("home")),
            count: 1
        }]
    );
}

#[test]
fn removing_a_space_drops_what_is_scoped_to_it_and_never_widens_the_rest() {
    let held = vec![
        grant("g-1", GrantCaller::Companion, only("work")),
        grant("g-2", GrantCaller::Cua, only("work")),
        grant("g-3", GrantCaller::Companion, only("home")),
        grant("g-4", GrantCaller::Companion, SpaceScope::Any),
    ];
    let read = without_space(held, &space("work"));
    assert_eq!(ids(&read), ["g-3", "g-4"]);
    assert_eq!(read.ended[0].count, 2);
    assert!(read.kept.iter().all(|g| g.key.space != only("work")));
}

fn text(grants: &[ActionGrant]) -> String {
    serde_json::to_string(grants).expect("json")
}

#[test]
fn an_old_file_is_read_by_porters_rules_and_what_it_cannot_place_is_dropped_never_widened() {
    let held = [
        grant("g-1", GrantCaller::Companion, only("work")),
        grant("g-2", GrantCaller::Companion, only("placeholder-a")),
        grant("g-3", GrantCaller::Companion, only("placeholder-b")),
        grant("g-4", GrantCaller::Companion, SpaceScope::Any),
    ];
    // Write the file as an older build did: the Space was any text.
    let file = text(&held)
        .replace("placeholder-a", "Not A Slug")
        .replace("placeholder-b", "also/bad");
    let read = decode_grants(&file).expect("the file reads");
    assert_eq!(ids(&read), ["g-1", "g-4"]);
    assert_eq!(
        read.ended,
        [Ended {
            why: GrantEnd::SpaceUnreadable,
            count: 2
        }]
    );
    assert!(
        read.kept
            .iter()
            .all(|g| g.key.space != SpaceScope::Any || g.id.as_str() == "g-4"),
        "a dropped grant never becomes one for every Space"
    );
}

#[test]
fn a_file_that_is_wrong_in_any_other_way_is_a_fault_not_a_migration() {
    assert!(decode_grants("{}").is_err());
    assert!(decode_grants(r#"[{"id":"g-1"}]"#).is_err());
    let mut broken =
        serde_json::to_value(grant("g-1", GrantCaller::Companion, only("work"))).expect("json");
    broken["decision"] = serde_json::json!("maybe");
    assert!(decode_grants(&format!("[{broken}]")).is_err());
}

#[test]
fn a_dropped_entry_is_audited_in_its_own_words() {
    let record = AuditRecord::GrantsEnded {
        at: at(5),
        ended: Ended {
            why: GrantEnd::SpaceGone(SpaceId::parse("home").expect("space")),
            count: 2,
        },
    };
    let json = serde_json::to_string(&record).expect("json");
    assert_eq!(
        json,
        r#"{"kind":"grants_ended","v":{"at":5,"ended":{"why":{"kind":"space_gone","v":"home"},"count":2}}}"#
    );
    assert_eq!(
        serde_json::from_str::<AuditRecord>(&json).expect("back"),
        record
    );
}
