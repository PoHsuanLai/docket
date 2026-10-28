//! A helper's planner is offered the actions of the apps on its card. The person set which apps it
//! may use and how far: the apps bound what it is offered (a job once waited out a sheet's two
//! minutes to load a skill it never needed), while going further in one of them is offered and
//! asks the person on a sheet that names the helper, so a job beyond its reach says so rather
//! than ending as if done. Of the companion's own actions it keeps handing a job on and proposing
//! a routine for its own card, which the router gates by its own rules, and loading a skill when
//! one fits its apps.

use docket_core::ActionCard;
use docket_permissions_core::TaskPolicy;
use docket_skills::Skill;
use prov::HelperId;

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
    reach.is_none_or(|reach| reach.offers(&card.action))
}

/// Whether `skill` is offered: everything it uses is on the menu `cards`.
pub(crate) fn skill_in_menu(skill: &Skill, cards: &[ActionCard]) -> bool {
    skill
        .uses
        .iter()
        .all(|used| cards.iter().any(|c| &c.action == used))
}
