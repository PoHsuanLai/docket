//! `$XDG_CONFIG_HOME/quire/intentd.toml`: which bus names play which role, the reviewer models
//! and the proposed values. The serde form is the file; every key is written.

use action_review::ReviewerSet;
use docket_core::{AgentConfig, CallerRole};
use porter_core::AppName;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const SHIPPED: &str = include_str!("../../../dist/intentd.toml");

/// Why the file is not a configuration.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// The text is not TOML in the expected shape.
    #[error("intentd.toml: {0}")]
    Toml(String),
    /// The second opinion is the deliberate model's family.
    #[error("the second opinion must come from another model family")]
    SameFamily,
}

/// The daemon's configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntentdConfig {
    /// The bus names that play each role; one name may play several. A name that is in none is
    /// a plain app (its own actions, plus search, preview and suggest).
    pub roles: BTreeMap<CallerRole, Vec<AppName>>,
    /// The reviewer models. Absent, the second opinion is missing and high-impact allows fall
    /// back to asking (one resident model, QUESTIONS M2).
    pub reviewers: Option<ReviewerSet>,
    /// The proposed values, as the settings give them.
    pub agent: AgentConfig,
}

impl IntentdConfig {
    /// Reads and checks the file: the reviewer set, if there is one, has a second opinion of
    /// another family.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let config: IntentdConfig =
            toml::from_str(text).map_err(|e| ConfigError::Toml(e.to_string()))?;
        if let Some(set) = &config.reviewers {
            set.check().map_err(|_| ConfigError::SameFamily)?;
        }
        Ok(config)
    }

    /// The configuration the package ships (`dist/intentd.toml`): the role names of the quire
    /// desktop, no reviewer set, the proposed values. What a machine with no file of its own
    /// runs with.
    pub fn shipped() -> Result<Self, ConfigError> {
        Self::parse(SHIPPED)
    }

    /// A configuration in which no name plays any role.
    pub fn empty() -> Self {
        Self {
            roles: BTreeMap::new(),
            reviewers: None,
            agent: AgentConfig::default(),
        }
    }

    /// The roles of a bus name: every role it is listed under; empty for a plain app.
    pub fn roles_of(&self, name: &AppName) -> BTreeSet<CallerRole> {
        self.roles
            .iter()
            .filter(|(_, names)| names.contains(name))
            .map(|(role, _)| *role)
            .collect()
    }
}
