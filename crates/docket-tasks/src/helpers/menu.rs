//! A helper's planner is offered the actions of the apps on its card. The person set which apps it
//! may use and how far: the apps bound what it is offered (a job once waited out a sheet's two
//! minutes to load a skill it never needed), while going further in one of them is offered and
//! asks the person on a sheet that names the helper, so a job beyond its reach says so rather
//! than ending as if done. Of the companion's own actions it keeps handing a job on, which the
//! router gates by its own rules, and loading a skill when one fits its apps.

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
/// offered everything as before): an action of one of the reach's apps, at any effect.
pub(crate) fn in_menu(card: &ActionCard, reach: Option<&TaskPolicy>) -> bool {
    let Some(reach) = reach else {
        return true;
    };
    if card.action.app.as_str() == COMPANION_APP {
        return matches!(card.action.name.as_str(), POST_JOB | SKILL_LOAD);
    }
    reach.actions.iter().any(|m| match m {
        ActionMatch::One(a) => a.app == card.action.app,
        ActionMatch::AppUpTo(app, _) => *app == card.action.app,
    })
}

/// Whether `skill` is offered: everything it uses is on the menu `cards`.
pub(crate) fn skill_in_menu(skill: &Skill, cards: &[ActionCard]) -> bool {
    skill
        .uses
        .iter()
        .all(|used| cards.iter().any(|c| &c.action == used))
}
