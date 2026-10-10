//! What the daemon reads and writes on disk: the person's consent file, the installed manifests
//! and the environment that says where they are.

use docket_core::{ActionGrant, ActionGrantKey, GrantCaller, GrantTarget};
use docket_fake::check_grant_file_contract;
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
    let shipped = include_str!("../../../../dist/intentd.toml");
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
        roles("org.quire.Temor"),
        [R::Cli],
        "a terminal in an app scope"
    );
    assert_eq!(
        roles("org.quire.Shell"),
        [R::Launcher, R::Confirm, R::Control]
    );
    assert_eq!(roles("org.quire.Companion1"), [R::Companion]);
    assert!(roles("org.quire.Mail").is_empty(), "an app is a plain app");
}

// ---- shipped default grants ----

const SHIPPED_DEFAULTS: &str = include_str!("../../../../dist/intents/default-grants.json");

fn shell_key(usage: Usage) -> ActionGrantKey {
    ActionGrantKey {
        caller: GrantCaller::Companion,
        owner: AppName::parse("org.quire.Shell").expect("app"),
        target: GrantTarget::App,
        class: DataClass::AppOwn,
        usage,
        space: SpaceScope::Any,
    }
}

fn verdict(store: &FileGrants, key: &ActionGrantKey) -> porter_core::consent::Verdict {
    porter_core::consent::decide(&store.grants(), key)
}

fn granted(v: &porter_core::consent::Verdict) -> bool {
    matches!(v, porter_core::consent::Verdict::Granted { .. })
}

/// A data directory holding `text` as the shipped defaults, and a store over it.
fn with_defaults(text: &str) -> (tempfile::TempDir, FileGrants) {
    let data = tempfile::tempdir().expect("scratch");
    write(data.path(), intentd::DEFAULT_GRANTS_FILE, text);
    let store = FileGrants::at(data.path().join("quire/intents/grants.json"))
        .with_defaults(&[data.path().to_path_buf()]);
    (data, store)
}

#[test]
fn the_shipped_defaults_allow_the_shell_in_both_usages_and_nothing_else() {
    let (_data, store) = with_defaults(SHIPPED_DEFAULTS);
    for usage in [Usage::Interactive, Usage::Background] {
        assert!(granted(&verdict(&store, &shell_key(usage))), "{usage:?}");
    }
    assert_eq!(store.grants().len(), 2);
    let other_app = ActionGrantKey {
        owner: AppName::parse("dev.notes").expect("app"),
        ..shell_key(Usage::Interactive)
    };
    let other_class = ActionGrantKey {
        class: DataClass::Mail,
        ..shell_key(Usage::Interactive)
    };
    let other_caller = ActionGrantKey {
        caller: GrantCaller::Cua,
        ..shell_key(Usage::Interactive)
    };
    for key in [other_app, other_class, other_caller] {
        assert_eq!(
            verdict(&store, &key),
            porter_core::consent::Verdict::Ask,
            "{key:?}"
        );
    }
}

#[test]
fn without_a_defaults_file_nothing_is_granted() {
    let data = tempfile::tempdir().expect("scratch");
    let store =
        FileGrants::at(data.path().join("grants.json")).with_defaults(&[data.path().to_path_buf()]);
    assert!(store.grants().is_empty());
}

#[test]
fn a_denial_the_person_records_wins_over_a_default_and_the_shipped_file_stays_as_it_was() {
    let (data, store) = with_defaults(SHIPPED_DEFAULTS);
    let defaults = store.grants();
    for (n, default) in defaults.iter().enumerate() {
        let id = GrantId::parse(&format!("g-revoked-{n}")).expect("id");
        store.record(intentd::revoking(default, id, UnixSeconds(100)));
    }
    for usage in [Usage::Interactive, Usage::Background] {
        assert_eq!(
            verdict(&store, &shell_key(usage)),
            porter_core::consent::Verdict::Denied,
            "{usage:?}"
        );
    }
    let kept = std::fs::read_to_string(
        data.path()
            .join("quire/intents")
            .join(intentd::DEFAULT_GRANTS_FILE),
    )
    .expect("read");
    assert_eq!(kept, SHIPPED_DEFAULTS, "the shipped file is never written");
    // The revocation survives a restart, and an allowance given later brings the default's
    // effect back for that key.
    let reopened = FileGrants::at(data.path().join("quire/intents/grants.json"))
        .with_defaults(&[data.path().to_path_buf()]);
    assert_eq!(
        verdict(&reopened, &shell_key(Usage::Interactive)),
        porter_core::consent::Verdict::Denied
    );
    let mut again = grant("g-again", "shell.unused", GrantScope::Always);
    again.key = shell_key(Usage::Interactive);
    again.at = UnixSeconds(200);
    reopened.record(again);
    assert!(granted(&verdict(&reopened, &shell_key(Usage::Interactive))));
}

#[test]
fn a_default_broader_than_app_own_allowances_is_skipped() {
    let mut list: Vec<ActionGrant> = serde_json::from_str(SHIPPED_DEFAULTS).expect("shipped");
    let mut mail = list[0].clone();
    mail.key.class = DataClass::Mail;
    let mut once = list[0].clone();
    once.scope = GrantScope::Once;
    once.key.usage = Usage::Background;
    let mut deny = list[0].clone();
    deny.decision = Decision::Deny;
    list.truncate(1);
    list.extend([mail, once, deny]);
    let (_data, store) = with_defaults(&serde_json::to_string(&list).expect("json"));
    let all = store.grants();
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0].key.class, DataClass::AppOwn);
    assert_eq!(all[0].at, UnixSeconds(0), "a default is dated the epoch");
}

#[test]
fn a_damaged_defaults_file_grants_nothing() {
    for text in ["", "{", "{\"not\": \"a list\"}"] {
        let (_data, store) = with_defaults(text);
        assert!(store.grants().is_empty(), "{text:?}");
    }
}

#[test]
fn the_consent_file_follows_the_shared_store_rules() {
    let dir = tempfile::tempdir().expect("scratch");
    check_grant_file_contract(
        dir.path(),
        |path| FileGrants::at(path.to_owned()),
        "standing.json",
    );
}
