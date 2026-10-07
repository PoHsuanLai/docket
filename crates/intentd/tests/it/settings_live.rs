//! The person's settings file reaches a router that is already serving: a change in the file
//! applies to the next decision without a restart. The wait is the watch's own event, never a sleep.

use docket_core::{AgentConfig, Strictness};
use docket_fake::fake_router;
use docket_settings::{AgentSettings, Locator};
use intentd::{SettingsWatch, WatchState, apply, apply_next};
use porter_core::Count;
use std::path::Path;

/// What the Settings app does: a temp file, then a rename over the settings file.
fn write_atomically(home: &Path, text: &str) {
    let dir = home.join("docket");
    std::fs::create_dir_all(&dir).unwrap();
    let temp = dir.join("settings.toml.tmp");
    std::fs::write(&temp, text).unwrap();
    std::fs::rename(&temp, dir.join("settings.toml")).unwrap();
}

#[tokio::test]
async fn a_change_in_the_file_applies_to_the_router_without_a_restart() {
    let home = tempfile::tempdir().unwrap();
    let nothing = tempfile::tempdir().unwrap();
    let env = |key: &str| match key {
        "XDG_CONFIG_HOME" => Some(home.path().display().to_string()),
        "XDG_CONFIG_DIRS" => Some(nothing.path().display().to_string()),
        _ => None,
    };
    let router = fake_router(AgentConfig::default()).expect("router");
    let mut watch = SettingsWatch::start(
        Locator::from_env(&env),
        AgentSettings::over(AgentConfig::default()),
    );
    assert_eq!(*watch.state(), WatchState::Live);
    apply(&router, &watch.current());
    assert_eq!(router.agent_config(), AgentConfig::default());

    write_atomically(
        home.path(),
        "[agent]\nstrictness = \"trust_more\"\n[agent.budget]\ncalls = 7\n",
    );
    while router.agent_config().strictness == Strictness::Default {
        let loaded = apply_next(&router, &mut watch)
            .await
            .expect("the watch is live");
        assert_eq!(loaded.fallbacks, vec![]);
    }
    assert_eq!(router.agent_config().strictness, Strictness::TrustMore);
    assert_eq!(router.agent_config().budget.calls, Count(7));

    // A bad value falls back to the base for that key; the good key beside it still applies.
    write_atomically(
        home.path(),
        "[agent]\nstrictness = \"ask_more\"\n[agent.budget]\ncalls = 0\n",
    );
    while router.agent_config().strictness != Strictness::AskMore {
        apply_next(&router, &mut watch)
            .await
            .expect("the watch is live");
    }
    assert_eq!(
        router.agent_config().budget.calls,
        AgentConfig::default().budget.calls,
        "the refused value is the base's, not the previous file's"
    );
}
