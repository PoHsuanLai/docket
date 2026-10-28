//! What the person is told about a helper, once, in plain words. No sentence here names how the
//! machinery works: a helper has jobs, a model and a pause, nothing more.

use super::card::{HelperName, PauseWhy};
use super::job::JobFault;
use super::rules::JobRefusal;
use serde::{Deserialize, Serialize};

/// Something worth telling the person about a helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Notice {
    /// It stopped taking jobs.
    Paused(PauseWhy),
    /// It has a job and no model.
    WaitsForModel,
    /// It has a job and its agent app is not signed in.
    WaitsForSignIn,
    /// A job of its ended without finishing and is open again.
    Failed(JobFault),
    /// A job for it was refused.
    Refused(JobRefusal),
}

impl Notice {
    /// The sentence, for the helper `name`.
    pub fn plain(&self, name: &HelperName) -> String {
        let name = name.as_str();
        match self {
            Notice::Paused(PauseWhy::ByPerson) => {
                format!("{name} is paused. It takes no jobs until you resume it.")
            }
            Notice::Paused(PauseWhy::Denied3) => format!(
                "{name} is paused: three of its requests in a row were refused. Resume it when you want it to try again."
            ),
            Notice::Paused(PauseWhy::Failed3) => format!(
                "{name} is paused: three of its jobs in a row did not finish. Resume it when you want it to try again."
            ),
            Notice::WaitsForModel => {
                format!("{name} needs a model before it can start. Choose one for it.")
            }
            Notice::WaitsForSignIn => {
                format!(
                    "{name} needs you to sign in before it can start. Open its card to sign in."
                )
            }
            Notice::Failed(fault) => format!("{name} stopped: {}.", fault.plain()),
            Notice::Refused(refusal) => refusal.plain(),
        }
    }
}

/// What the person is told when a write is refused because another helper's job owns the thing.
pub fn owned_sentence(owner: &HelperName) -> String {
    format!("{} is working on this.", owner.as_str())
}
