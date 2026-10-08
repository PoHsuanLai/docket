//! The preset that confines an agent's own extras: the `session/new` meta and the variables it
//! adds, and that nothing of it is a file the agent could reach.

use super::support::{LOGIN, confined, plan, rig};
use bulkhead::AgentRun;
use docket_acp::client::{AgentChild, Spawn};
use docket_launch::fake::Mood;
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
