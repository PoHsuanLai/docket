//! The setting `agent.mcp.expose` (quire design/22 section 3.27) read from docket's own settings
//! file, `$XDG_CONFIG_HOME/docket/settings.toml`, as `[agent.mcp] expose = "on"`. The schema the
//! Settings app draws the row from is `dist/settings/docket.settings.toml`.
//!
//! Lenient as design/22 section 2 asks: a missing file, a missing key or a value that is not a
//! word of `McpExpose` is `Off`, never an error. A switch that cannot be read is a switch that is
//! not on.

use crate::expose::McpExpose;
use std::path::PathBuf;

/// The schema docket ships for its settings (design/22 section 9.2).
pub const SCHEMA: &str = include_str!("../../../dist/settings/docket.settings.toml");

/// The file the settings live in, under the configuration directory.
pub const SETTINGS_FILE: &str = "docket/settings.toml";

/// The key's path in the schema.
pub const SETTINGS_KEY: &str = "agent.mcp.expose";

/// What the settings text says; anything else is `Off`.
pub fn exposed(text: &str) -> McpExpose {
    let word = text.parse::<toml::Table>().ok().and_then(|t| {
        t.get("agent")?
            .get("mcp")?
            .get("expose")?
            .as_str()
            .map(str::to_owned)
    });
    match word.as_deref() {
        Some("on") => McpExpose::On,
        _ => McpExpose::Off,
    }
}

/// The configuration directories, the person's first: `$XDG_CONFIG_HOME`, then `$XDG_CONFIG_DIRS`.
pub(crate) fn config_dirs(env: &impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let home = env("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env("HOME").unwrap_or_default()).join(".config"));
    let rest = env("XDG_CONFIG_DIRS")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/etc/xdg".to_owned());
    std::iter::once(home)
        .chain(rest.split(':').map(PathBuf::from))
        .collect()
}

/// The first settings file of the configuration directories, read; none is `Off`.
pub fn from_env(env: &impl Fn(&str) -> Option<String>) -> McpExpose {
    config_dirs(env)
        .into_iter()
        .map(|d| d.join(SETTINGS_FILE))
        .find_map(|path| std::fs::read_to_string(path).ok())
        .map_or(McpExpose::Off, |text| exposed(&text))
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
    fn the_shipped_schema_describes_the_key_the_reader_reads() {
        let schema: toml::Table = SCHEMA.parse().expect("schema is TOML");
        assert_eq!(schema["file"].as_str(), Some(SETTINGS_FILE));
        let keys = schema["key"].as_array().expect("keys");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0]["path"].as_str(), Some(SETTINGS_KEY));
        assert_eq!(keys[0]["default"].as_str(), Some("off"));
    }
}
