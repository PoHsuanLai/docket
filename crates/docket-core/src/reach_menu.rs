//! What a helper is offered of the desktop's actions: one rule for the helper's own planner and
//! for the agent app that is a helper. The person set which apps the helper may use and how far
//! on its card; the apps bound the menu, while going further inside one of them is offered and
//! asks the person on a sheet that names the helper. Of the companion's own actions the helper
//! keeps handing a job on, suggesting a routine and loading a skill; never another app's, never the assistant's own.

use crate::task_policy::{ActionMatch, TaskPolicy};
use docket_vocab::ActionRef;
use prov::AgentRef;

/// The built-in provider whose actions the companion carries out itself.
pub const COMPANION_APP: &str = "org.quire.Companion";

/// Starts a task: a worker that does a goal of its own.
pub const TASK_START: &str = "companion.task.start";

/// Gives a job to a helper by its name.
pub const HAND_ON_JOB: &str = "companion.helper.post_job";

/// Suggests a routine for the person to accept.
pub const PROPOSE_ROUTINE: &str = "companion.routine.propose";

/// Loads a skill's body into the planner's view.
pub const SKILL_LOAD: &str = "companion.skill.load";

impl TaskPolicy {
    /// Whether a helper whose reach this is is offered `action`: an action of one of the reach's
    /// apps at any effect, or one of the companion actions a helper keeps.
    pub fn offers(&self, action: &ActionRef) -> bool {
        if action.app.as_str() == COMPANION_APP {
            return kept_by_helpers(action);
        }
        self.actions.iter().any(|m| match m {
            ActionMatch::One(a) => a.app == action.app,
            ActionMatch::AppUpTo(app, _) => *app == action.app,
        })
    }
}

/// Whether `action` is one of the companion actions every helper keeps, whatever its card names:
/// handing a job on (the job runs under the other helper's own reach), suggesting a routine (the
/// person accepts it on its own sheet) and loading a skill.
pub(crate) fn kept_by_helpers(action: &ActionRef) -> bool {
    action.app.as_str() == COMPANION_APP
        && matches!(
            action.name.as_str(),
            HAND_ON_JOB | PROPOSE_ROUTINE | SKILL_LOAD
        )
}

/// Whether the planner of `agent` is offered `action` at all. A task does its own goal, so a worker
/// is not offered `companion.task.start`: only the person's assistant hands out work, and a
/// worker that did would start the goal it was given again, one run nested in the last.
pub fn offered_to(action: &ActionRef, agent: &AgentRef) -> bool {
    !(matches!(agent, AgentRef::Worker { .. })
        && action.app.as_str() == COMPANION_APP
        && action.name.as_str() == TASK_START)
}
