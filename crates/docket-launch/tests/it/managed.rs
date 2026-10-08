//! The preset that confines an agent's own extras: the `session/new` meta and the variables it
//! adds, and that nothing of it is a file the agent could reach.

use super::support::{LOGIN, confined, plan, rig};
use docket_acp::client::{AgentChild, Spawn};
use docket_launch::fake::Mood;
use docket_shell::AgentRun;
use serde_json::json;

fn env_of<'a>(run: &'a AgentRun, name: &str) -> Option<&'a str> {
    run.env
        .iter()
        .find(|v| v.name == name)
        .map(|v| v.value.as_str())
}

#[tokio::test]
async fn the_preset_hands_back_the_settings_as_session_meta_with_exactly_the_desktops_server_allowed()
 {
    let mut rig = rig(&confined("/home/me/.claude", ""), Mood::Working, true);
    let spawned = rig
        .spawn
        .spawn(&plan("claude-code", "s-1"))
        .await
        .expect("spawned");
    let settings = json!({
        "disableClaudeAiConnectors": true,
        "syncClaudeAiSkills": false,
        "syncClaudeAiPlugins": false,
        "permissions": {"allow": ["mcp__quire"]},
    });
    let meta = spawned.meta.clone().expect("meta");
    assert_eq!(
        serde_json::Value::Object(meta),
        json!({"claudeCode": {"options": {"settings": settings}}})
    );
    let mut child = spawned.child;
    child.close().await;
}

#[tokio::test]
async fn the_preset_sets_the_isolation_variables_and_not_the_ignored_managed_path() {
    let mut rig = rig(&confined("/home/me/.claude", ""), Mood::Working, true);
    let _spawned = rig
        .spawn
        .spawn(&plan("claude-code", "s-1"))
        .await
        .expect("spawned");
    let run = &rig.procs.runs()[0];
    assert_eq!(env_of(run, "ENABLE_CLAUDEAI_MCP_SERVERS"), Some("false"));
    assert_eq!(env_of(run, "CLAUDE_CODE_DISABLE_CLAUDE_MDS"), Some("1"));
    assert_eq!(
        env_of(run, "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"),
        Some("1")
    );
    assert_eq!(env_of(run, "DISABLE_AUTOUPDATER"), Some("1"));
    assert!(env_of(run, "CLAUDE_CODE_MANAGED_SETTINGS_PATH").is_none());
}

#[tokio::test]
async fn nothing_of_the_preset_is_a_file_or_a_bind_the_agent_could_reach() {
    let mut rig = rig(&confined("/home/me/.claude", ""), Mood::Working, true);
    let _spawned = rig
        .spawn
        .spawn(&plan("claude-code", "s-1"))
        .await
        .expect("spawned");
    let run = &rig.procs.runs()[0];
    // The settings travel in the host's own `session/new`: no file under the run directory, and
    // no bind that is not the entry's.
    let left: Vec<_> = std::fs::read_dir(rig.dir.path()).expect("dir").collect();
    assert!(left.is_empty(), "{left:?}");
    assert!(
        run.binds
            .iter()
            .all(|b| !b.path.as_str().contains("managed"))
    );
}

#[tokio::test]
async fn the_persons_set_cannot_turn_the_variables_back() {
    let set = "ENABLE_CLAUDEAI_MCP_SERVERS = \"true\"\nDISABLE_AUTOUPDATER = \"0\"";
    let mut rig = rig(&confined("/home/me/.claude", set), Mood::Working, true);
    let _spawned = rig
        .spawn
        .spawn(&plan("claude-code", "s-1"))
        .await
        .expect("spawned");
    let run = &rig.procs.runs()[0];
    assert_eq!(env_of(run, "ENABLE_CLAUDEAI_MCP_SERVERS"), Some("false"));
    assert_eq!(env_of(run, "DISABLE_AUTOUPDATER"), Some("1"));
}

#[tokio::test]
async fn an_entry_without_the_preset_has_no_meta_and_no_variables() {
    let mut rig = rig(LOGIN, Mood::Working, true);
    let spawned = rig
        .spawn
        .spawn(&plan("claude-code", "s-1"))
        .await
        .expect("spawned");
    assert!(spawned.meta.is_none());
    let run = &rig.procs.runs()[0];
    assert!(env_of(run, "ENABLE_CLAUDEAI_MCP_SERVERS").is_none());
}

#[test]
fn the_preset_reads_from_agents_toml_and_an_unknown_one_is_refused() {
    use docket_launch::{AgentsFile, Profile};
    let file = AgentsFile::parse(&confined("/home/me/.claude", "")).expect("file");
    let entry = file
        .get(&docket_session::ProgramName::parse("claude-code").expect("name"))
        .expect("entry");
    assert_eq!(entry.profile, Some(Profile::ClaudeCode));
    let bad = confined("/home/me/.claude", "")
        .replace("profile = \"claude-code\"", "profile = \"other\"");
    assert!(AgentsFile::parse(&bad).is_err());
}

#[tokio::test]
async fn the_sign_in_the_entry_names_is_handed_to_the_client_and_none_otherwise() {
    let signed = format!("{LOGIN}sign_in = \"oauth-personal\"\n");
    let mut first = rig(&signed, Mood::Working, true);
    let spawned = first
        .spawn
        .spawn(&plan("claude-code", "s-1"))
        .await
        .expect("spawned");
    assert_eq!(
        spawned.sign_in.as_ref().map(|s| s.as_str()),
        Some("oauth-personal")
    );
    let mut bare = rig(LOGIN, Mood::Working, true);
    let spawned = bare
        .spawn
        .spawn(&plan("claude-code", "s-1"))
        .await
        .expect("spawned");
    assert!(spawned.sign_in.is_none());
}

// ---- the agy preset: a settings file shown over the agent's own, in the sandbox only ----

fn agy_entry(state: &str, sign_in: &str) -> String {
    format!(
        r#"
[[agent]]
program = "agy"
command = "/home/u/.local/share/agy/agy_acp_server.par"
route = "login"
network = "host"
state = ["{state}"]
home = "/home/u"
profile = "agy"
{sign_in}
"#
    )
}

const AGY_AT: &str = "/home/u/.gemini/antigravity-acp/settings.json";

#[tokio::test]
async fn the_agy_preset_shows_a_file_with_exactly_the_allow_rule_and_the_auth_type() {
    let toml = agy_entry("/home/u/.gemini", "sign_in = \"oauth-personal\"");
    let mut rig = rig(&toml, Mood::Working, true);
    let spawned = rig.spawn.spawn(&plan("agy", "s-1")).await.expect("spawned");
    assert!(spawned.meta.is_none(), "agy takes no session/new meta");
    let run = &rig.procs.runs()[0];
    assert_eq!(run.overlays.len(), 1);
    assert_eq!(run.overlays[0].to.as_str(), AGY_AT);
    let text = std::fs::read_to_string(run.overlays[0].from.as_str()).expect("file");
    let value: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(
        value,
        json!({
            "permissions": {"allow": ["mcp(quire/*)"]},
            "auth": {"type": "oauth-personal"},
        })
    );
    let mut child = spawned.child;
    child.close().await;
}

#[tokio::test]
async fn without_a_sign_in_the_file_has_no_auth_key() {
    let mut rig = rig(&agy_entry("/home/u/.gemini", ""), Mood::Working, true);
    let _spawned = rig.spawn.spawn(&plan("agy", "s-1")).await.expect("spawned");
    let run = &rig.procs.runs()[0];
    let text = std::fs::read_to_string(run.overlays[0].from.as_str()).expect("file");
    let value: serde_json::Value = serde_json::from_str(&text).expect("json");
    assert_eq!(value, json!({"permissions": {"allow": ["mcp(quire/*)"]}}));
}

#[tokio::test]
async fn an_existing_state_file_is_shadowed_not_written_and_the_run_file_goes_with_the_session() {
    let state = tempfile::tempdir().expect("state");
    let theirs = state.path().join("antigravity-acp");
    std::fs::create_dir_all(&theirs).expect("dir");
    let file = theirs.join("settings.json");
    let mine = r#"{"permissions":{"allow":["command(git)"]}}"#;
    std::fs::write(&file, mine).expect("their file");
    let toml = agy_entry(state.path().to_str().expect("utf8"), "");
    let mut rig = rig(&toml, Mood::Working, true);
    let spawned = rig.spawn.spawn(&plan("agy", "s-1")).await.expect("spawned");
    let run = &rig.procs.runs()[0];
    let from = run.overlays[0].from.as_str().to_owned();
    // The file is under the run directory, never in the person's state, which is as it was.
    assert!(from.starts_with(rig.dir.path().to_str().expect("utf8")));
    assert_eq!(std::fs::read_to_string(&file).expect("read"), mine);
    let names: Vec<_> = std::fs::read_dir(&theirs).expect("dir").collect();
    assert_eq!(names.len(), 1);
    // Nothing else is made visible for it: the binds are the entry's own.
    assert!(run.binds.iter().all(|b| b.path.as_str() != from));
    let mut child = spawned.child;
    child.close().await;
    assert!(!std::path::Path::new(&from).exists());
    assert!(
        std::fs::read_dir(rig.dir.path())
            .expect("dir")
            .next()
            .is_none()
    );
}

#[tokio::test]
async fn the_overlay_lands_under_the_default_home_when_the_entry_names_none() {
    let toml = agy_entry("/home/u/.gemini", "").replace("home = \"/home/u\"\n", "");
    let mut rig = rig(&toml, Mood::Working, true);
    let _spawned = rig.spawn.spawn(&plan("agy", "s-1")).await.expect("spawned");
    let to = rig.procs.runs()[0].overlays[0].to.as_str().to_owned();
    assert!(
        to.starts_with(docket_shell::SANDBOX_HOME)
            && to.ends_with(".gemini/antigravity-acp/settings.json")
    );
}

#[tokio::test]
async fn the_other_entries_have_no_overlay_and_write_nothing() {
    for toml in [LOGIN.to_owned(), confined("/home/me/.claude", "")] {
        let mut rig = rig(&toml, Mood::Working, true);
        let _spawned = rig
            .spawn
            .spawn(&plan("claude-code", "s-1"))
            .await
            .expect("spawned");
        assert!(rig.procs.runs()[0].overlays.is_empty());
        assert!(
            std::fs::read_dir(rig.dir.path())
                .expect("dir")
                .next()
                .is_none()
        );
    }
}

#[test]
fn agy_reads_from_agents_toml() {
    use docket_launch::{AgentsFile, Profile};
    let file = AgentsFile::parse(&agy_entry("/home/u/.gemini", "")).expect("file");
    let entry = file
        .get(&docket_session::ProgramName::parse("agy").expect("name"))
        .expect("entry");
    assert_eq!(entry.profile, Some(Profile::Agy));
    assert!(Profile::Agy.env().is_empty());
}
