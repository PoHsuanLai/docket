//! Consent keyed by Space, on disk and through a whole intentd: another app's own Space is
//! never held, a removed Space drops the grants scoped to it (never moving them), a restart
//! drops the grants of a Space that went while the daemon was away, and an old file's entries
//! that porter cannot read are dropped, never widened. The bus is private and accountd is a
//! fake that serves only `Spaces1`.

use crate::support::bus::PrivateBus;
use crate::support::memoryd::FakeMemoryd;
use crate::support::spaces::FakeSpaces;
use docket_core::{ActionGrant, ActionGrantKey, AgentConfig, GrantCaller, GrantTarget};
use docket_router::GrantStore;
use intentd::{Cadence, FileGrants, IntentdConfig, Setup, start};
use porter_core::consent::{Decision, Grant, GrantScope, Usage};
use porter_core::{AppName, DataClass, GrantId};
use prov::{ActionName, SpaceId, SpaceScope, UnixSeconds};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn app(name: &str) -> AppName {
    AppName::parse(name).expect("app")
}

fn only(id: &str) -> SpaceScope {
    SpaceScope::Only(SpaceId::parse(id).expect("space"))
}

fn grant(id: &str, caller: GrantCaller, space: SpaceScope) -> ActionGrant {
    Grant {
        id: GrantId::parse(id).expect("id"),
        key: ActionGrantKey {
            caller,
            owner: app("org.quire.Mail"),
            target: GrantTarget::Action(ActionName::parse("mail.thread.archive").expect("action")),
            class: DataClass::Mail,
            usage: Usage::Interactive,
            space,
        },
        decision: Decision::Allow,
        scope: GrantScope::Always,
        at: UnixSeconds(1),
    }
}

fn held(store: &FileGrants) -> Vec<String> {
    let mut ids: Vec<String> = store.grants().iter().map(|g| g.id.to_string()).collect();
    ids.sort();
    ids
}

#[test]
fn an_app_cannot_hold_a_grant_over_another_apps_own_space() {
    let dir = tempfile::tempdir().expect("scratch");
    let store = FileGrants::at(dir.path().join("grants.json"));
    let photos = GrantCaller::App(app("org.quire.Photos"));
    store.record(grant("g-1", photos.clone(), only("app:org.quire.Mail:1")));
    store.record(grant("g-2", photos.clone(), only("app:org.quire.Photos:1")));
    store.record(grant("g-3", photos, only("work")));
    assert_eq!(
        held(&store),
        ["g-2", "g-3"],
        "the refused grant was not kept"
    );

    // A file that already holds one (written by hand or by an older build) does not serve it.
    let planted = vec![
        grant(
            "g-4",
            GrantCaller::App(app("org.quire.Photos")),
            only("app:org.quire.Mail:1"),
        ),
        grant("g-5", GrantCaller::Companion, SpaceScope::Any),
    ];
    std::fs::write(
        dir.path().join("grants.json"),
        serde_json::to_string(&planted).expect("json"),
    )
    .expect("plant");
    assert_eq!(held(&store), ["g-5"]);
}

#[test]
fn removing_a_space_drops_its_grants_and_leaves_every_other() {
    let dir = tempfile::tempdir().expect("scratch");
    let store = FileGrants::at(dir.path().join("grants.json"));
    store.record(grant("g-1", GrantCaller::Companion, only("work")));
    store.record(grant("g-2", GrantCaller::Cua, only("home")));
    store.record(grant("g-3", GrantCaller::Cli, SpaceScope::Any));
    let ended = store
        .end_space(&SpaceId::parse("work").expect("space"))
        .expect("settled");
    assert_eq!(ended.len(), 1);
    assert_eq!(ended[0].count, 1);
    assert_eq!(held(&store), ["g-2", "g-3"]);
    assert!(
        store
            .end_space(&SpaceId::parse("work").expect("space"))
            .expect("again")
            .is_empty(),
        "nothing left to end is nothing audited"
    );
}

#[test]
fn a_damaged_file_is_left_alone_when_a_space_goes() {
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().join("grants.json");
    std::fs::write(&path, "not json").expect("plant");
    let store = FileGrants::at(path.clone());
    assert!(
        store
            .end_space(&SpaceId::parse("work").expect("space"))
            .is_err()
    );
    assert_eq!(std::fs::read_to_string(&path).expect("read"), "not json");
}

struct Desk {
    _dir: tempfile::TempDir,
    _bus: PrivateBus,
    memoryd: Arc<FakeMemoryd>,
    spaces: FakeSpaces,
    spaces_connection: zbus::Connection,
    path: PathBuf,
    _connections: Vec<docket_dbus::BusConnection>,
    _intentd: intentd::Running,
}

impl Desk {
    /// A desk whose consent file `prefill` fills, and whose accountd lists `listed`, before
    /// intentd starts.
    async fn start(listed: &[&str], prefill: impl FnOnce(&std::path::Path)) -> Desk {
        let dir = tempfile::tempdir().expect("scratch");
        let bus = PrivateBus::start(dir.path());
        let memoryd = FakeMemoryd::recording();
        let memory_connection = bus.connect().await;
        memoryd.serve(&memory_connection).await;
        let spaces_connection = bus.connect().await;
        let spaces = FakeSpaces::serve(&spaces_connection, listed).await;
        let home = dir.path().display().to_string();
        let env = |key: &str| match key {
            "HOME" => Some(home.clone()),
            "XDG_DATA_HOME" => Some(dir.path().join("data").display().to_string()),
            "XDG_DATA_DIRS" => Some(dir.path().join("none").display().to_string()),
            "XDG_CONFIG_HOME" => Some(dir.path().join("config").display().to_string()),
            "XDG_CONFIG_DIRS" => Some(dir.path().join("none").display().to_string()),
            _ => None,
        };
        let mut setup = Setup::from_env(&env).expect("setup");
        prefill(&setup.grants);
        setup.config = IntentdConfig {
            roles: BTreeMap::new(),
            reviewers: None,
            agent: AgentConfig::default(),
        };
        setup.audit_every = Duration::from_millis(50);
        setup.signals = Cadence {
            every: Duration::from_millis(30),
            rescan_every: 2,
        };
        let path = setup.grants.clone();
        let daemon_connection = bus.connect().await;
        let intentd = start(&daemon_connection, None, setup)
            .await
            .expect("intentd");
        Desk {
            _dir: dir,
            _bus: bus,
            memoryd,
            spaces,
            spaces_connection,
            path,
            _connections: vec![memory_connection, daemon_connection],
            _intentd: intentd,
        }
    }

    /// The grants held, once `wanted` says the file is as the test expects (or the wait ends).
    async fn held_when(&self, wanted: impl Fn(&[String]) -> bool) -> Vec<String> {
        for _ in 0..100 {
            let now = held(&FileGrants::at(self.path.clone()));
            if wanted(&now) {
                return now;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        held(&FileGrants::at(self.path.clone()))
    }

    /// The consent file's text, once it no longer holds `gone` (or the wait ends).
    async fn file_without(&self, gone: &str) -> String {
        for _ in 0..100 {
            let text = std::fs::read_to_string(&self.path).unwrap_or_default();
            if !text.contains(gone) {
                return text;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        std::fs::read_to_string(&self.path).unwrap_or_default()
    }

    async fn ended_records(&self) -> usize {
        for _ in 0..100 {
            let n = self
                .memoryd
                .stored()
                .iter()
                .filter(|r| r.body.kind().as_str() == "docket.grants_ended")
                .count();
            if n > 0 {
                return n;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        0
    }
}

fn plant(path: &std::path::Path, grants: &[ActionGrant], swaps: &[(&str, &str)]) {
    let mut text = serde_json::to_string(grants).expect("json");
    for (from, to) in swaps {
        text = text.replace(from, to);
    }
    std::fs::create_dir_all(path.parent().expect("dir")).expect("dir");
    std::fs::write(path, text).expect("plant");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_restart_drops_the_grants_of_a_space_that_went_and_audits_it() {
    let desk = Desk::start(&["work"], |path| {
        plant(
            path,
            &[
                grant("g-1", GrantCaller::Companion, only("work")),
                grant("g-2", GrantCaller::Companion, only("home")),
                grant("g-3", GrantCaller::Companion, SpaceScope::Any),
                grant("g-4", GrantCaller::Cua, only("app:org.quire.Mail:2")),
            ],
            &[],
        );
    })
    .await;
    let now = desk.held_when(|ids| !ids.contains(&"g-2".to_owned())).await;
    assert_eq!(now, ["g-1", "g-3", "g-4"]);
    assert!(desk.ended_records().await >= 1, "the drop is audited");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_old_entry_porter_cannot_read_is_dropped_never_widened() {
    let desk = Desk::start(&["work"], |path| {
        plant(
            path,
            &[
                grant("g-1", GrantCaller::Companion, only("work")),
                grant("g-2", GrantCaller::Companion, only("placeholder")),
            ],
            &[("placeholder", "My Old Space")],
        );
    })
    .await;
    assert_eq!(desk.held_when(|ids| ids.len() == 1).await, ["g-1"]);
    let file = desk.file_without("My Old Space").await;
    assert!(
        !file.contains("My Old Space"),
        "the file is rewritten without it"
    );
    assert!(
        !file.contains(r#""kind": "any""#),
        "nothing became every Space"
    );
    assert!(desk.ended_records().await >= 1, "the drop is audited");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn removing_a_space_while_running_drops_its_grants() {
    let desk = Desk::start(&["work", "home"], |path| {
        plant(
            path,
            &[
                grant("g-1", GrantCaller::Companion, only("work")),
                grant("g-2", GrantCaller::Companion, only("home")),
                grant("g-3", GrantCaller::Companion, SpaceScope::Any),
            ],
            &[],
        );
    })
    .await;
    desk.spaces.remove(&desk.spaces_connection, "work").await;
    let now = desk.held_when(|ids| !ids.contains(&"g-1".to_owned())).await;
    assert_eq!(now, ["g-2", "g-3"]);
    assert!(desk.ended_records().await >= 1, "the removal is audited");
}
