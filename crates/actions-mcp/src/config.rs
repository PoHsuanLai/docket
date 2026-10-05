//! `$XDG_CONFIG_HOME/quire/actions-mcp.toml`: whether the edge is on and the name its client goes
//! by. Off by default (QUESTIONS S7): a file that does not exist is an edge that lists nothing.
//! Every key is optional. The switch is the setting `mcp.enabled` once quire's settings carry it;
//! until then this file is where the person (or the package) writes it.

use crate::access::McpAccess;
use prov::ClientName;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const SHIPPED: &str = include_str!("../../../dist/actions-mcp.toml");

/// Why the file is not a configuration.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("actions-mcp.toml: {0}")]
pub struct ConfigError(String);

fn client() -> ClientName {
    ClientName::parse("mcp-client").expect("`mcp-client` is a valid client name")
}

/// The edge's configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpConfig {
    /// Whether external clients may use the registry's offered actions.
    #[serde(default)]
    pub access: McpAccess,
    /// The name every argument and call of this process is labelled with.
    #[serde(default = "client")]
    pub client: ClientName,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            access: McpAccess::Off,
            client: client(),
        }
    }
}

impl McpConfig {
    /// Reads the file.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        toml::from_str(text).map_err(|e| ConfigError(e.to_string()))
    }

    /// The configuration the package ships (`dist/actions-mcp.toml`): off.
    pub fn shipped() -> Result<Self, ConfigError> {
        Self::parse(SHIPPED)
    }

    /// The first `quire/actions-mcp.toml` of the configuration directories (`$XDG_CONFIG_HOME`,
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
            .map(|d| d.join("quire").join("actions-mcp.toml"))
            .find_map(|path| std::fs::read_to_string(path).ok());
        match found {
            Some(text) => Self::parse(&text),
            None => Self::shipped(),
        }
    }

    /// The same configuration under another client name.
    pub fn named(self, client: ClientName) -> Self {
        Self { client, ..self }
    }
}
