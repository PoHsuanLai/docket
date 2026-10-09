//! Where the built-in assistant may run (owner decision 2026-10-09): three kinds of place, each
//! an on/off choice, instead of a model picker. This module is the typed setting, its lenient
//! reader and writer, the never-widening merge with what the machine knows, and the pure routing
//! over it. The floor (what a request's data may reach) is plumbing, not a setting: it filters
//! first in [`route`] and nothing here can lift it.

mod known;
mod model;
mod read;
mod route;
mod seam;
mod words;
mod write;

pub use known::{Known, KnownPlace};
pub use model::{
    CloudAccount, CloudAllowed, Floor, ModelChoice, OwnComputer, Place, PlaceKind, PlaceName,
    Places, Toggle,
};
pub use read::{PLACES_PATH, PlacesLoaded, read_places};
pub use route::{Routed, route};
pub use seam::PlaceSource;
pub use words::{confirm_turn_on, kind_label, no_place_says};
pub use write::write_places;
