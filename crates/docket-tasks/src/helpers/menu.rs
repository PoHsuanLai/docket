//! A helper's planner is offered only what its reach covers. The person set on the card which
//! apps it may use and how far; a card beyond that only ends in a question while no one may be
//! there to answer it (a job once waited out a sheet's two minutes to load a skill it never
//! needed). Of the companion's own actions it keeps handing a job on, which the router gates by
//! its own rules, and loading a skill when one fits inside the reach.

use crate::helpers::desk_calls::POST_JOB;
use crate::read::COMPANION_APP;
use docket_core::{ActionCard, ActionMatch, TaskPolicy};
use docket_skills::Skill;
use prov::HelperId;

/// Loads a skill's body into the planner's view.
pub(crate) const SKILL_LOAD: &str = "companion.skill.load";

/// The helper a companion runs a job for, and the reach the person set on its card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsHelper {
    /// The helper.
    pub id: HelperId,
    /// What it may use, and how far.
    pub reach: TaskPolicy,
}

/// Whether the planner is offered `card` under `reach` (none: the front conversation, which is
/// offered everything as before).
pub(crate) fn in_menu(card: &ActionCard, reach: Option<&TaskPolicy>) -> bool {
    let Some(reach) = reach else {
        return true;
    };
    if card.action.app.as_str() == COMPANION_APP {
        return matches!(card.action.name.as_str(), POST_JOB | SKILL_LOAD);
    }
    card.effect <= reach.ceiling
        && reach.actions.iter().any(|m| match m {
            ActionMatch::One(a) => *a == card.action,
            ActionMatch::AppUpTo(app, cap) => *app == card.action.app && card.effect <= *cap,
        })
}

/// Whether `skill` is offered: everything it uses is on the menu `cards`.
pub(crate) fn skill_in_menu(skill: &Skill, cards: &[ActionCard]) -> bool {
    skill
        .uses
        .iter()
        .all(|used| cards.iter().any(|c| &c.action == used))
}
