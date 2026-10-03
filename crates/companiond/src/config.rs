//! `$XDG_CONFIG_HOME/quire/companiond.toml`: which bus name speaks for the person, which Spaces a
//! restart reads, and the proposed values. Every key is optional.

use docket_core::AgentConfig;
use porter_core::AppName;
use prov::SpaceId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const SHIPPED: &str = include_str!("../../../dist/companiond.toml");

/// Why the file is not a configuration.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("companiond.toml: {0}")]
pub struct ConfigError(String);

fn shell() -> AppName {
    AppName::parse("org.quire.Shell").expect("`org.quire.Shell` is a valid app name")
}

fn spaces() -> Vec<SpaceId> {
    vec![SpaceId::desktop()]
}

/// The daemon's configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompaniondConfig {
    /// The bus name that speaks for the person.
    #[serde(default = "shell")]
    pub shell: AppName,
    /// The Spaces a restart reads its records from.
    #[serde(default = "spaces")]
    pub spaces: Vec<SpaceId>,
    /// The proposed values.
    #[serde(default)]
    pub agent: AgentConfig,
}

impl Default for CompaniondConfig {
    fn default() -> Self {
        Self {
            shell: shell(),
            spaces: spaces(),
            agent: AgentConfig::default(),
        }
    }
}

impl CompaniondConfig {
    /// Reads the file.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        toml::from_str(text).map_err(|e| ConfigError(e.to_string()))
    }

    /// The configuration the package ships (`dist/companiond.toml`).
    pub fn shipped() -> Result<Self, ConfigError> {
        Self::parse(SHIPPED)
    }

    /// The first `quire/companiond.toml` of the configuration directories (`$XDG_CONFIG_HOME`,
    /// then `$XDG_CONFIG_DIRS`), else the shipped one.
    pub fn from_env(env: &impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let home = env("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env("HOME").unwrap_or_default()).join(".config"));
        let rest = env("XDG_CONFIG_DIRS")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "/etc/xdg".to_owned());
        let found = std::iter::once(home)
            .chain(rest.split(':').map(PathBuf::from))
            .map(|d| d.join("quire").join("companiond.toml"))
            .find_map(|path| std::fs::read_to_string(path).ok());
        match found {
            Some(text) => Self::parse(&text),
            None => Self::shipped(),
        }
    }
}
