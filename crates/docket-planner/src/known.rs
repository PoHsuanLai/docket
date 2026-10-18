//! Parameters whose valid values this turn the view already tells. Such a parameter is offered
//! to the model as a list of exactly those values, and a call that names another is refused
//! before the router is asked, with the list. Nothing here is specific to one app: an action
//! gets an entry when the view holds the full set of what one of its parameters may name.

use docket_core::{ActionRef, COMPANION_APP, HAND_ON_JOB, OneOf, ParamName, PlannerView};

/// The parameter of giving a job that names the helper to give it to. `after` (whose last answer
/// is handed on) is left free: it may name the helper whose planner this is, which the view does
/// not list, and the desk answers a name it does not know in words.
const HELPER_PARAMS: [&str; 1] = ["helper"];

/// The names of the helpers the view lists, each once, in the order listed.
fn helper_names(view: &PlannerView) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for line in &view.roster.helpers {
        let name = line.name.as_str();
        if !names.iter().any(|n| n == name) {
            names.push(name.to_owned());
        }
    }
    names
}

/// What `action`'s parameters may take for this view. Empty when the view does not hold a full
/// set (no helpers listed), so nothing is narrowed to a list with no member.
pub(crate) fn known_values(action: &ActionRef, view: &PlannerView) -> Vec<OneOf> {
    if action.app.as_str() != COMPANION_APP || action.name.as_str() != HAND_ON_JOB {
        return Vec::new();
    }
    let names = helper_names(view);
    if names.is_empty() {
        return Vec::new();
    }
    HELPER_PARAMS
        .iter()
        .filter_map(|param| ParamName::parse(param).ok())
        .map(|param| OneOf {
            param,
            values: names.clone(),
        })
        .collect()
}
