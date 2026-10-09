//! Routing over the allowed places: the floor first, then closest first.

use super::model::{CloudAllowed, Floor, Place, PlaceKind, Places, Toggle};

/// What routing decided for one task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routed {
    /// Run it here.
    Chosen(Place),
    /// Run it here after asking the person (a cloud account set to ask each time).
    Ask(Place),
    /// A place within the floor could do it but is not allowed: say so and offer to allow one
    /// of this kind. Never a silent cloud fallback.
    NoAllowedPlace {
        /// The closest kind of place that could do it.
        would_need: PlaceKind,
    },
    /// Nothing within the floor can do it; allowing a place would not help.
    Blocked,
}

enum Say {
    Yes,
    Ask,
    No,
}

fn say(place: &Place) -> Say {
    match place {
        Place::ThisComputer { .. } => Say::Yes,
        Place::OwnComputer(o) => match o.allowed {
            Toggle::On => Say::Yes,
            Toggle::Off => Say::No,
        },
        Place::Cloud(c) => match c.allowed {
            CloudAllowed::On => Say::Yes,
            CloudAllowed::AskEachTime => Say::Ask,
            CloudAllowed::Off => Say::No,
        },
    }
}

/// Picks the place for a task. `floor` removes places first and no setting restores one;
/// `can_do` says whether a place is good enough for the task. Then the closest allowed place
/// wins: this computer, your computers, cloud accounts.
pub fn route(floor: Floor, places: &Places, can_do: impl Fn(&Place) -> bool) -> Routed {
    let fits: Vec<Place> = places
        .all()
        .into_iter()
        .filter(|p| floor.admits(p.kind()) && can_do(p))
        .collect();
    let allowed = fits.iter().find_map(|p| match say(p) {
        Say::Yes => Some(Routed::Chosen(p.clone())),
        Say::Ask => Some(Routed::Ask(p.clone())),
        Say::No => None,
    });
    allowed.unwrap_or_else(|| {
        fits.first()
            .map_or(Routed::Blocked, |p| Routed::NoAllowedPlace {
                would_need: p.kind(),
            })
    })
}
