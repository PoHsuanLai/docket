//! Which actions an agent may be offered: a choice from the catalogue by app, by action name and
//! by the greatest effect, never a closure. An action not chosen is not declared to the model,
//! and a call the model names for one is read as "no such tool" before it reaches the router.

use docket_planner::{Catalogue, CatalogueTool};
use prov::Effect;

/// Something a choice named that the catalogue does not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// No offered action belongs to this app.
    App(String),
    /// No offered action has this name.
    Action(String),
}

/// A set of actions, narrowed step by step from a catalogue. Each narrowing keeps a subset of
/// what was there, so a later step can never bring back what an earlier one dropped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actions {
    tools: Vec<CatalogueTool>,
    missing: Vec<Missing>,
}

impl From<&Catalogue> for Actions {
    /// Every action of the catalogue; narrow it from here.
    fn from(catalogue: &Catalogue) -> Self {
        Self {
            tools: catalogue.tools().to_vec(),
            missing: Vec::new(),
        }
    }
}

impl Actions {
    /// No action at all: an agent that only reads, asks and answers.
    pub fn none() -> Self {
        Self {
            tools: Vec::new(),
            missing: Vec::new(),
        }
    }

    /// Only the actions of `app` (its manifest name, such as `org.quire.Mail`).
    pub fn app(self, app: &str) -> Self {
        let tools: Vec<_> = self
            .tools
            .iter()
            .filter(|t| t.app.as_str() == app)
            .cloned()
            .collect();
        let miss = tools.is_empty().then(|| Missing::App(app.to_owned()));
        self.then(tools, miss)
    }

    /// Only the actions with these names (such as `mail.thread.archive`). A name that is not in
    /// the set is a fault of the build, not a silent omission.
    pub fn named(self, names: &[&str]) -> Self {
        let tools = self
            .tools
            .iter()
            .filter(|t| names.contains(&t.decl.name.as_str()))
            .cloned()
            .collect();
        let missing = names
            .iter()
            .filter(|n| self.tools.iter().all(|t| t.decl.name.as_str() != **n))
            .map(|n| Missing::Action((*n).to_owned()))
            .collect::<Vec<_>>();
        Self {
            tools,
            missing: [self.missing, missing].concat(),
        }
    }

    /// Only the actions whose effect is at most `effect` (`Read` < `UndoableWrite` < `Outbound`
    /// < `Destructive`).
    pub fn up_to(self, effect: Effect) -> Self {
        let tools = self
            .tools
            .iter()
            .filter(|t| t.decl.effect <= effect)
            .cloned()
            .collect();
        self.then(tools, None)
    }

    /// Only the actions that change nothing.
    pub fn reads_only(self) -> Self {
        self.up_to(Effect::Read)
    }

    fn then(self, tools: Vec<CatalogueTool>, miss: Option<Missing>) -> Self {
        Self {
            tools,
            missing: self.missing.into_iter().chain(miss).collect(),
        }
    }

    /// What the choices named that were not there.
    pub fn missing(&self) -> &[Missing] {
        &self.missing
    }

    /// The chosen actions as the catalogue the planner is built over.
    pub fn catalogue(&self) -> Catalogue {
        Catalogue::from_tools(self.tools.clone())
    }
}
