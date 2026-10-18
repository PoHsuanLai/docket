//! What a helper is offered of the desktop's actions: one rule for the helper's own planner and
//! for the agent app that is a helper. The person set which apps the helper may use and how far
//! on its card; the apps bound the menu, while going further inside one of them is offered and
//! asks the person on a sheet that names the helper. Of the companion's own actions the helper
//! keeps handing a job on, suggesting a routine and loading a skill; never another app's, never the assistant's own.

use crate::ids::ActionRef;
use crate::task_policy::{ActionMatch, TaskPolicy};

/// The built-in provider whose actions the companion carries out itself.
pub const COMPANION_APP: &str = "org.quire.Companion";

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
            return matches!(action.name.as_str(), HAND_ON_JOB | PROPOSE_ROUTINE | SKILL_LOAD);
        }
        self.actions.iter().any(|m| match m {
            ActionMatch::One(a) => a.app == action.app,
            ActionMatch::AppUpTo(app, _) => *app == action.app,
        })
    }
}
