//! The preset that confines an agent's own extras: what docket writes, what the child is told,
//! and that nothing the agent can write reaches the file.

use super::support::{abs, confined, plan, rig};
use docket_acp::client::{AgentChild, Spawn, SpawnFault};
use docket_launch::fake::Mood;
use docket_shell::{Access, AgentRun, agent_bwrap_args};
use std::os::unix::fs::PermissionsExt;

const PATH_VAR: &str = "CLAUDE_CODE_MANAGED_SETTINGS_PATH";

fn env_of<'a>(run: &'a AgentRun, name: &str) -> Option<&'a str> {
    run.env
        .iter()
        .find(|v| v.name == name)
        .map(|v| v.value.as_str())
}

#[tokio::test]
async fn the_preset_writes_a_private_read_only_file_and_points_the_child_at_it() {
    let mut rig = rig(&confined("/home/me/.claude", ""), Mood::Working, true);
    let mut child = rig
        .spawn
        .spawn(&plan("claude-code", "s-1"))
        .await
        .expect("spawned")
        .child;
    let run = &rig.procs.runs()[0];
    // The variable names the directory: Claude Code reads `managed-settings.json` (and a
    // `managed-settings.d` of drop-ins) inside it, and reads nothing when handed the file.
    let named = env_of(run, PATH_VAR).expect("the variable").to_owned();
    let dir = rig.dir.path().join("docket-managed-acp-s-1");
    assert_eq!(named, dir.to_str().expect("utf8"));
    let file = dir.join("managed-settings.json");
    let mode =
        |p: &std::path::Path| std::fs::metadata(p).expect("meta").permissions().mode() & 0o777;
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&file), 0o400);
    let entries: Vec<_> = std::fs::read_dir(&dir).expect("dir").collect();
    assert_eq!(
        entries.len(),
        1,
        "the bound directory holds the settings file only"
    );

    // Connectors, skills and plugins are off, and exactly the desktop's server is pre-allowed.
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).expect("file")).expect("json");
    assert_eq!(json["disableClaudeAiConnectors"], true);
    assert_eq!(json["syncClaudeAiSkills"], false);
    assert_eq!(json["syncClaudeAiPlugins"], false);
    assert_eq!(
        json["permissions"]["allow"],
        serde_json::json!(["mcp__quire"])
    );
    assert_eq!(
        json["allowedMcpServers"],
        serde_json::json!([{"serverName": "quire"}])
    );
    assert_eq!(json["allowManagedMcpServersOnly"], true);
    assert_eq!(json["allowManagedHooksOnly"], true);

    assert_eq!(env_of(run, "ENABLE_CLAUDEAI_MCP_SERVERS"), Some("false"));
    assert_eq!(env_of(run, "CLAUDE_CODE_DISABLE_CLAUDE_MDS"), Some("1"));
    assert_eq!(
        env_of(run, "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC"),
        Some("1")
    );
    assert_eq!(env_of(run, "DISABLE_AUTOUPDATER"), Some("1"));

    child.close().await;
    assert!(!dir.exists(), "the file goes with the session");
}

#[tokio::test]
async fn the_agent_cannot_write_the_settings_because_their_directory_is_bound_read_only_and_last() {
    let mut rig = rig(&confined("/home/me/.claude", ""), Mood::Working, true);
    let _child = rig
        .spawn
        .spawn(&plan("claude-code", "s-1"))
        .await
        .expect("spawned")
        .child;
    let run = &rig.procs.runs()[0];
    let file = env_of(run, PATH_VAR).expect("variable");
    let last = run.binds.last().expect("binds");
    assert_eq!(last.path.as_str(), file);
    assert_eq!(last.access, Access::ReadOnly);
    // No read-write bind (state, cwd) contains it.
    let writable = run.binds.iter().filter(|b| b.access == Access::ReadWrite);
    for bind in writable.chain([&docket_shell::Bind {
        path: run.cwd.clone(),
        access: Access::ReadWrite,
    }]) {
        assert!(!std::path::Path::new(file).starts_with(bind.path.as_str()));
    }
    // In bwrap's words: a `--ro-bind` of the file, and no `--bind` after it.
    let args = agent_bwrap_args(run, &["/home", "/tmp"]);
    let at = args.iter().position(|a| a == file).expect("file in args");
    assert_eq!(args[at - 1], "--ro-bind");
    let later_rw_ancestor = args[at + 1..]
        .windows(2)
        .any(|w| w[0] == "--bind" && std::path::Path::new(file).starts_with(&w[1]));
    assert!(!later_rw_ancestor);
}

#[tokio::test]
async fn a_place_under_the_agents_state_or_cwd_is_refused() {
    // State that contains the run directory would shadow the file with a writable bind.
    let probe = rig(&confined("/x", ""), Mood::Working, true);
    let parent = probe
        .dir
        .path()
        .parent()
        .expect("parent")
        .to_str()
        .expect("utf8")
        .to_owned();
    let mut shadowed = rig(&confined(&parent, ""), Mood::Working, true);
    let fault = shadowed
        .spawn
        .spawn(&plan("claude-code", "s-1"))
        .await
        .expect_err("refused");
    assert_eq!(fault, SpawnFault::Sandbox);
    assert!(shadowed.procs.runs().is_empty());
    // The cwd case is direct: the working directory is the run directory.
    let mut rig = rig(&confined("/home/me/.claude", ""), Mood::Working, true);
    let mut p = plan("claude-code", "s-1");
    p.cwd = abs(rig.dir.path().to_str().expect("utf8"));
    let fault = rig.spawn.spawn(&p).await.expect_err("refused");
    assert_eq!(fault, SpawnFault::Sandbox);
    assert!(rig.procs.runs().is_empty());
}

#[tokio::test]
async fn the_person_cannot_point_the_variable_elsewhere() {
    let set = format!(
        "{PATH_VAR} = \"/home/me/.claude/settings.json\"\nENABLE_CLAUDEAI_MCP_SERVERS = \"true\""
    );
    let mut rig = rig(&confined("/home/me/.claude", &set), Mood::Working, true);
    let _child = rig
        .spawn
        .spawn(&plan("claude-code", "s-1"))
        .await
        .expect("spawned")
        .child;
    let run = &rig.procs.runs()[0];
    assert!(
        env_of(run, PATH_VAR)
            .expect("variable")
            .contains("docket-managed-")
    );
    assert_eq!(env_of(run, "ENABLE_CLAUDEAI_MCP_SERVERS"), Some("false"));
}

#[tokio::test]
async fn an_entry_without_the_preset_gets_no_file_and_no_variable() {
    let mut rig = rig(super::support::LOGIN, Mood::Working, true);
    let _child = rig
        .spawn
        .spawn(&plan("claude-code", "s-1"))
        .await
        .expect("spawned")
        .child;
    let run = &rig.procs.runs()[0];
    assert!(env_of(run, PATH_VAR).is_none());
    assert!(!rig.dir.path().join("docket-managed-acp-s-1").exists());
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
