//! Which of the desktop's actions one session's tool edge lists and lets through.

use docket_core::TaskPolicy;
use docket_vocab::ActionRef;

/// What the edge of one session offers its agent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum EdgeMenu {
    /// Every action an agent may be offered (the assistant's coding agent).
    #[default]
    Everything,
    /// What a helper's card allows, as for a native helper: its own apps' actions, handing a job
    /// on, and loading a skill. Never another app's.
    Helper(Box<TaskPolicy>),
}

impl EdgeMenu {
    /// Whether `action` is on the menu.
    pub fn offers(&self, action: &ActionRef) -> bool {
        match self {
            EdgeMenu::Everything => true,
            EdgeMenu::Helper(reach) => reach.offers(action),
        }
    }
}
