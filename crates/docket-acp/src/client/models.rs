//! Which model the agent runs, and which it offers. The offer is read from the reply to
//! `session/new`: a session option that selects a model, or the older model list some agents
//! still send. The model the person configured is set once, before the first turn, and the
//! reply is awaited; a model the agent does not offer is a refusal that lists the ones it does.

use crate::out;
use agent_client_protocol_schema::rpc::RequestId;
use agent_client_protocol_schema::v1::{
    AGENT_METHOD_NAMES, NewSessionResponse, SessionConfigKind, SessionConfigOptionCategory,
    SessionConfigSelectOptions, SessionConfigValueId, SessionId, SetSessionConfigOptionRequest,
};
use docket_session::{Choice, Choices};
use serde_json::{Value, json};

/// The model the person chose, by the id the agent gives it. Written in `agents.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelId(String);

/// Why a model id was not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a model id is 1 to 128 characters without spaces or control characters")]
pub struct ModelIdRefused;

impl ModelId {
    /// A model id: printable, no whitespace.
    pub fn parse(text: &str) -> Result<Self, ModelIdRefused> {
        let fine = !text.is_empty()
            && text.chars().count() <= 128
            && !text.chars().any(|c| c.is_whitespace() || c.is_control());
        fine.then(|| Self(text.to_owned())).ok_or(ModelIdRefused)
    }

    /// The id as the agent knows it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// How the agent takes a model.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Way {
    /// A session option with this id (`session/set_config_option`).
    Option(String),
    /// The older model list (`session/set_model`).
    Older,
}

/// The models a session offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Models {
    way: Way,
    /// The one in use now, if the agent says.
    pub current: Option<String>,
    /// Every one on offer.
    pub available: Vec<Choice>,
}

impl Models {
    /// What the agent offered in its reply to `session/new`; none when it offers no choice.
    pub fn of_reply(reply: &Value, made: &NewSessionResponse) -> Option<Self> {
        Self::from_option(made).or_else(|| Self::from_older(reply))
    }

    fn from_option(made: &NewSessionResponse) -> Option<Self> {
        let option = made.config_options.iter().flatten().find(|o| {
            matches!(o.category, Some(SessionConfigOptionCategory::Model))
                && matches!(o.kind, SessionConfigKind::Select(_))
        })?;
        let SessionConfigKind::Select(select) = &option.kind else {
            return None;
        };
        let flat: Vec<_> = match &select.options {
            SessionConfigSelectOptions::Ungrouped(list) => list.iter().collect(),
            SessionConfigSelectOptions::Grouped(groups) => {
                groups.iter().flat_map(|g| g.options.iter()).collect()
            }
            _ => return None,
        };
        Some(Self {
            way: Way::Option(option.id.0.to_string()),
            current: Some(select.current_value.0.to_string()),
            available: flat
                .iter()
                .map(|o| Choice {
                    id: o.value.0.to_string(),
                    name: o.name.clone(),
                })
                .collect(),
        })
    }

    fn from_older(reply: &Value) -> Option<Self> {
        let state = reply.get("models")?;
        let list = state.get("availableModels")?.as_array()?;
        let available: Vec<Choice> = list
            .iter()
            .filter_map(|m| {
                let id = m.get("modelId")?.as_str()?;
                let name = m.get("name").and_then(Value::as_str).unwrap_or(id);
                Some(Choice {
                    id: id.to_owned(),
                    name: name.to_owned(),
                })
            })
            .collect();
        Some(Self {
            way: Way::Older,
            current: state
                .get("currentModelId")
                .and_then(Value::as_str)
                .map(str::to_owned),
            available,
        })
    }

    /// Whether `wanted` is on offer.
    pub fn offers(&self, wanted: &ModelId) -> bool {
        self.available.iter().any(|c| c.id == wanted.as_str())
    }

    /// The offer as a refusal lists it.
    pub fn choices(&self) -> Choices {
        Choices(self.available.clone())
    }

    /// The request that switches the session to `wanted`.
    pub fn switch(&self, id: &RequestId, session: &SessionId, wanted: &ModelId) -> String {
        match &self.way {
            Way::Option(option) => {
                let request = SetSessionConfigOptionRequest::new(
                    session.clone(),
                    option.clone(),
                    SessionConfigValueId::new(wanted.as_str()),
                );
                out::ask(id, AGENT_METHOD_NAMES.session_set_config_option, &request)
            }
            Way::Older => out::ask(
                id,
                "session/set_model",
                &json!({"sessionId": session, "modelId": wanted.as_str()}),
            ),
        }
    }
}
