//! What a preset (`profile` in `agents.toml`) adds to a run to confine what a program brings of
//! its own: environment variables, and extra `_meta` for `session/new`. The router never knows
//! which program it is; `docket_acp::client` only carries the `_meta` the launcher hands back.
//!
//! Claude Code reads no managed-settings file outside its hosted mode (its
//! `CLAUDE_CODE_MANAGED_SETTINGS_PATH` is ignored), so the settings go the way its ACP adapter
//! takes them: `_meta.claudeCode.options.settings` of `session/new`, an object the adapter hands
//! the SDK as its `settings` option, the flag tier, which outranks the user's, the project's and
//! the local settings. The object is sent inline: docket writes `session/new`, so the agent has
//! no file to write and no path to shadow, and nothing is bound into the sandbox for it.
//!
//! The settings turn off the account's connectors, skills and plugins and allow the desktop's
//! tool server (and no other) to be called without Claude Code's own prompt, so the router's sheet
//! is the only one. Keys that only apply in managed settings (`allowManagedMcpServersOnly`,
//! `allowedMcpServers`, `allowManagedHooksOnly`) do nothing at the flag tier and are not sent.
//! A `permissions.allow` the person's own settings hold is unioned with ours; that is theirs.

use crate::config::Profile;
use docket_acp::client::{SERVER_NAME, SessionMeta};
use serde_json::{Value, json};

impl Profile {
    /// The variables that go with the preset. They are applied after the entry's `set`.
    pub fn env(self) -> Vec<(&'static str, &'static str)> {
        match self {
            Profile::ClaudeCode => vec![
                ("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "1"),
                ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
                ("ENABLE_CLAUDEAI_MCP_SERVERS", "false"),
                ("DISABLE_AUTOUPDATER", "1"),
            ],
        }
    }

    /// The `_meta` of `session/new` the preset adds.
    pub fn session_meta(self) -> SessionMeta {
        match self {
            Profile::ClaudeCode => claude_code_meta(),
        }
    }
}

/// The allow rule for every tool of the desktop's server and no other server.
pub fn allow_rule() -> String {
    format!("mcp__{SERVER_NAME}")
}

fn claude_code_meta() -> SessionMeta {
    let settings = json!({
        "disableClaudeAiConnectors": true,
        "syncClaudeAiSkills": false,
        "syncClaudeAiPlugins": false,
        "permissions": { "allow": [allow_rule()] },
    });
    let Value::Object(meta) = json!({ "claudeCode": { "options": { "settings": settings } } })
    else {
        unreachable!("a JSON object literal is an object")
    };
    meta
}
