//! What the daemon reads and writes on disk: the person's consent file, the installed manifests
//! and the environment that says where they are.

use docket_core::{ActionGrant, ActionGrantKey, GrantCaller, GrantTarget};
use docket_router::GrantStore;
use intentd::{FileGrants, Setup, load_manifests};
use porter_core::consent::{Decision, Grant, GrantScope, Usage};
use porter_core::{AppName, DataClass, GrantId};
use prov::{ActionName, SpaceScope, UnixSeconds};
use std::path::{Path, PathBuf};

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
fn grants_survive_a_restart_and_a_second_decision_for_the_same_key_replaces_the_first() {
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().join("quire/intents/grants.json");
    let store = FileGrants::at(path.clone());
    assert!(store.grants().is_empty(), "no file is no grants");
    store.record(grant("g-1", "mail.thread.archive", GrantScope::Once));
    store.record(grant("g-2", "mail.draft.create", GrantScope::Always));
    let reopened = FileGrants::at(path.clone());
    assert_eq!(reopened.grants().len(), 2);
    reopened.record(grant("g-3", "mail.thread.archive", GrantScope::Always));
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

#[test]
fn a_damaged_file_is_no_grants_and_never_a_panic() {
    let dir = tempfile::tempdir().expect("scratch");
    let path = dir.path().join("grants.json");
    for text in ["", "{", "{\"not\": \"a list\"}", "[{\"id\": 3}]"] {
        std::fs::write(&path, text).expect("write");
        let store = FileGrants::at(path.clone());
        assert!(store.grants().is_empty(), "{text:?}");
    }
    // A damaged file does not stop a new decision from being kept.
    let store = FileGrants::at(path);
    store.record(grant("g-1", "mail.thread.archive", GrantScope::Always));
    assert_eq!(store.grants().len(), 1);
}

fn write(dir: &Path, name: &str, text: &str) {
    let folder = dir.join("quire/intents");
    std::fs::create_dir_all(&folder).expect("dir");
    std::fs::write(folder.join(name), text).expect("write");
}

#[test]
fn manifests_are_read_from_every_data_dir_and_an_earlier_dir_wins() {
    let mine = tempfile::tempdir().expect("scratch");
    let system = tempfile::tempdir().expect("scratch");
    let mail = docket_fake::MAIL_MANIFEST;
    write(mine.path(), "org.quire.Mail.toml", mail);
    write(system.path(), "org.quire.Mail.toml", mail);
    write(
        system.path(),
        "org.quire.Files.toml",
        docket_fake::FILES_MANIFEST,
    );
    write(system.path(), "broken.toml", "this is not a manifest");
    write(system.path(), "grants.json", "[]");
    let loaded = load_manifests(&[mine.path().to_path_buf(), system.path().to_path_buf()]);
    let apps: Vec<String> = loaded
        .manifests
        .iter()
        .map(|m| m.manifest().app.to_string())
        .collect();
    assert_eq!(apps, ["org.quire.Mail", "org.quire.Files"]);
    assert_eq!(loaded.skipped.len(), 1, "{:?}", loaded.skipped);
    assert!(loaded.skipped[0].0.ends_with("broken.toml"));
    assert!(
        load_manifests(&[PathBuf::from("/no/such/dir")])
            .manifests
            .is_empty(),
        "a data dir that is not there holds nothing"
    );
}

#[test]
fn the_environment_says_where_everything_is() {
    let home = tempfile::tempdir().expect("scratch");
    let config = home.path().join("config");
    let data = home.path().join("data");
    write(&data, "org.quire.Mail.toml", docket_fake::MAIL_MANIFEST);
    let env = |key: &str| match key {
        "XDG_CONFIG_HOME" => Some(config.display().to_string()),
        "XDG_DATA_HOME" => Some(data.display().to_string()),
        "XDG_DATA_DIRS" => Some(String::new()),
        "XDG_CONFIG_DIRS" => Some(String::new()),
        "XDG_SESSION_ID" => Some("c3".into()),
        "HOME" => Some(home.path().display().to_string()),
        _ => None,
    };
    // No file of its own: the shipped configuration.
    let setup = Setup::from_env(&env).expect("setup");
    assert_eq!(
        setup.config,
        intentd::IntentdConfig::shipped().expect("shipped")
    );
    assert_eq!(setup.manifests.manifests.len(), 1);
    assert_eq!(setup.grants, data.join("quire/intents/grants.json"));
    assert_eq!(setup.session.as_deref(), Some("c3"));
    // A file of its own replaces the shipped one whole.
    let folder = config.join("quire");
    std::fs::create_dir_all(&folder).expect("dir");
    let shipped = include_str!("../../../dist/intentd.toml");
    std::fs::write(
        folder.join("intentd.toml"),
        shipped.replace("org.quire.Do", "org.quire.Mine"),
    )
    .expect("write");
    let own = Setup::from_env(&env).expect("setup");
    assert!(
        own.config
            .roles
            .values()
            .flatten()
            .any(|n| n.as_str() == "org.quire.Mine")
    );
    // A file that is not a configuration stops the daemon: it is not guessed at.
    std::fs::write(folder.join("intentd.toml"), "roles = 3").expect("write");
    assert!(Setup::from_env(&env).is_err());
}

#[test]
fn the_shipped_configuration_gives_a_terminal_the_cli_role_and_sill_the_rest() {
    let config = intentd::IntentdConfig::shipped().expect("shipped");
    let roles = |name: &str| {
        config
            .roles_of(&AppName::parse(name).expect("app"))
            .into_iter()
            .collect::<Vec<_>>()
    };
    use docket_core::CallerRole as R;
    assert_eq!(roles("org.quire.Do"), [R::Cli]);
    assert_eq!(
        roles("org.quire.Shell"),
        [R::Launcher, R::Confirm, R::Control]
    );
    assert_eq!(roles("org.quire.Companion1"), [R::Companion]);
    assert!(roles("org.quire.Mail").is_empty(), "an app is a plain app");
}
