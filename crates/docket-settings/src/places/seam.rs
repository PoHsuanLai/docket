//! The seam to inferd: what docket needs to list, as data. Porter implements it over its
//! listing call; tests implement it over a fixed list.

use super::known::Known;
use super::model::{ModelChoice, PlaceName};

/// What inferd tells docket about places and the models each offers.
pub trait PlaceSource {
    /// The own computers and cloud accounts inferd knows.
    fn known(&self) -> Known;
    /// The models `place` offers, for the advanced picker; data only, empty when unknown.
    fn models(&self, place: &PlaceName) -> Vec<ModelChoice>;
}
