//! Person-facing text of the agent rows: plain words, no protocol names, no developer terms. The
//! Settings app draws these strings as they are.

use docket_core::Rewind;

use super::model::{AgentRow, Availability, Install, ModelState, Offers, SignInState, Source};

impl AgentRow {
    /// Whether it is signed in: "Signed in", "Needs signing in", "Not started yet".
    pub fn sign_in_line(&self) -> &'static str {
        match self.sign_in {
            SignInState::SignedIn => "Signed in",
            SignInState::NeedsSignIn { .. } => "Needs signing in",
            SignInState::NotSeenYet => "Not started yet",
            SignInState::Unknown => "Couldn't check signing in",
        }
    }

    /// The ways of signing in the agent listed, when it needs one.
    pub fn ways_line(&self) -> Option<String> {
        match &self.sign_in {
            SignInState::NeedsSignIn { ways } if !ways.is_empty() => {
                let names: Vec<&str> = ways.iter().map(|w| w.name.as_str()).collect();
                Some(format!("Ways to sign in: {}", names.join(", ")))
            }
            _ => None,
        }
    }

    /// The model: "Model: Sonnet 5.5", "Model: sonnet-5-5 (no longer offered)".
    pub fn model_line(&self) -> String {
        match &self.model {
            ModelState::AgentsOwn => "Model: the agent's own choice".to_owned(),
            ModelState::Chosen { id, availability } => match availability {
                Availability::Offered(name) => format!("Model: {name}"),
                Availability::NoLongerOffered => format!("Model: {id} (no longer offered)"),
                Availability::NotCheckedYet => format!("Model: {id} (not checked yet)"),
            },
        }
    }

    /// Who keeps the history of the files it changes: "Restore points: kept by docket".
    pub fn rewind_line(&self) -> &'static str {
        match self.rewind {
            Rewind::Docket => "Restore points: kept by docket",
            Rewind::Agent => "Restore points: kept by the agent",
        }
    }

    /// What it offers: "Not started yet", "No choice of model", "3 models to choose from".
    pub fn offers_line(&self) -> String {
        match &self.offers {
            Offers::NotSeenYet => "Not started yet".to_owned(),
            Offers::Seen { models, .. } => match models.len() {
                0 => "No choice of model".to_owned(),
                1 => "1 model to choose from".to_owned(),
                n => format!("{n} models to choose from"),
            },
        }
    }

    /// Whether its program is there: "Installed", "Not installed yet (version 1.3.0)".
    pub fn install_line(&self) -> String {
        match &self.source {
            Source::Own => "Installed".to_owned(),
            Source::Registry {
                installed: Install::Installed,
                ..
            } => "Installed".to_owned(),
            Source::Registry {
                version,
                installed: Install::NotInstalled,
                ..
            } => format!("Not installed yet (version {version})"),
        }
    }
}
