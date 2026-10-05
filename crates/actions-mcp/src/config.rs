//! `$XDG_CONFIG_HOME/quire/actions-mcp.toml`: the name the edge's client goes by. Every key is
//! optional. Whether the edge is on is not here: it is the setting `agent.mcp.expose` in docket's
//! own settings file (`settings.rs`), off when nothing says otherwise (QUESTIONS S7).

use crate::expose::McpExpose;
use crate::settings;
use prov::ClientName;
use serde::{Deserialize, Serialize};

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
    pub expose: McpExpose,
    /// The name every argument and call of this process is labelled with.
    #[serde(default = "client")]
    pub client: ClientName,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            expose: McpExpose::Off,
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
    /// then `$XDG_CONFIG_DIRS`), else the shipped one; the switch from the settings file.
    pub fn from_env(env: &impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let found = settings::config_dirs(env)
            .into_iter()
            .map(|d| d.join("quire").join("actions-mcp.toml"))
            .find_map(|path| std::fs::read_to_string(path).ok());
        let config = match found {
            Some(text) => Self::parse(&text)?,
            None => Self::shipped()?,
        };
        Ok(Self {
            expose: settings::from_env(env),
            ..config
        })
    }

    /// The same configuration under another client name.
    pub fn named(self, client: ClientName) -> Self {
        Self { client, ..self }
    }
}
