//! `voiced.toml`: which bus names play which role. `shell` is sill; `app` is not listed: it is
//! whoever owns the bus name a `Route` names, checked with `GetNameOwner`.

use porter_core::AppName;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What a caller is to voiced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceRole {
    /// Opens utterances and speech; sill.
    Shell,
    /// Attaches to an utterance it was routed.
    App,
}

/// Whether the begin and end sounds play (`voice.earcons`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Earcons {
    /// Play them.
    On,
    /// Silent.
    Off,
}

/// Why the file is not a configuration.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// The TOML does not parse into the configuration.
    #[error("voiced.toml: {0}")]
    Parse(String),
    /// No bus name has the shell role.
    #[error("voiced.toml names no shell")]
    NoShell,
}

/// The configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoicedConfig {
    /// The bus names per role.
    pub roles: BTreeMap<VoiceRole, Vec<AppName>>,
    /// The earcons.
    pub earcons: Earcons,
    /// `input = "<node.name>"`: capture from this physical source when it exists, instead of the
    /// person's default source. Absent: follow the default. (Sill's settings could carry it later.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
}

impl VoicedConfig {
    /// Reads `voiced.toml`; a file with no shell is refused, since nothing could then begin.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let config: VoicedConfig =
            toml::from_str(text).map_err(|e| ConfigError::Parse(e.to_string()))?;
        if config
            .roles
            .get(&VoiceRole::Shell)
            .is_none_or(Vec::is_empty)
        {
            return Err(ConfigError::NoShell);
        }
        Ok(config)
    }

    /// The role a bus name plays, ignoring `App` (which a route decides).
    pub fn role_of(&self, name: &AppName) -> Option<VoiceRole> {
        self.roles
            .iter()
            .find(|(_, names)| names.contains(name))
            .map(|(role, _)| *role)
    }
}
