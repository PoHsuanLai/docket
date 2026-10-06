//! The setting `agent.mcp.expose` (quire design/22 section 3.27) read from docket's own settings
//! file, `$XDG_CONFIG_HOME/docket/settings.toml`, as `[agent.mcp] expose = "on"`. The schema the
//! Settings app draws the row from is `dist/settings/docket.settings.toml`.
//!
//! The reading is `docket-settings`' (lenient as design/22 section 2 asks): a missing file, a
//! missing key or a value that is not a word of `McpExpose` is `Off`, never an error. A switch
//! that cannot be read is a switch that is not on. This crate may not link a file watcher
//! (`scripts/check-boundary.sh`), so the edge reads the file again whenever it is asked whether it
//! is on: a change in the Settings app applies to the next request.

use crate::expose::McpExpose;
use docket_settings::{AgentSettings, Locator, read};
use std::path::PathBuf;

pub use docket_settings::{SCHEMA, SETTINGS_FILE};

/// The key's path in the schema.
pub const SETTINGS_KEY: &str = "agent.mcp.expose";

/// What the settings text says; anything else is `Off`.
pub fn exposed(text: &str) -> McpExpose {
    read(text, AgentSettings::default()).value.expose
}

/// The configuration directories, the person's first: `$XDG_CONFIG_HOME`, then `$XDG_CONFIG_DIRS`.
pub(crate) fn config_dirs(env: &impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    Locator::from_env(env).dirs().to_vec()
}

/// The first settings file of the configuration directories, read; none is `Off`.
pub fn from_env(env: &impl Fn(&str) -> Option<String>) -> McpExpose {
    Locator::from_env(env)
        .read(AgentSettings::default())
        .value
        .expose
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_is_on_only_when_the_file_says_on() {
        let cases = [
            ("", McpExpose::Off),
            ("[agent.mcp]\nexpose = \"on\"\n", McpExpose::On),
            ("[agent.mcp]\nexpose = \"off\"\n", McpExpose::Off),
            ("[agent.mcp]\nexpose = \"maybe\"\n", McpExpose::Off),
            ("[agent.mcp]\nexpose = true\n", McpExpose::Off),
            ("[agent]\nstrictness = \"default\"\n", McpExpose::Off),
            ("[agent.mcp\nexpose = \"on\"", McpExpose::Off),
            (
                "[agent]\nstrictness = \"default\"\n[agent.mcp]\nexpose = \"on\"\n",
                McpExpose::On,
            ),
        ];
        for (text, want) in cases {
            assert_eq!(exposed(text), want, "{text:?}");
        }
    }

    #[test]
    fn the_shipped_schema_names_the_key_the_reader_reads() {
        let schema: toml::Table = SCHEMA.parse().expect("schema is TOML");
        assert_eq!(schema["file"].as_str(), Some(SETTINGS_FILE));
        let keys = schema["key"].as_array().expect("keys");
        assert!(
            keys.iter()
                .any(|k| k["path"].as_str() == Some(SETTINGS_KEY))
        );
    }
}
