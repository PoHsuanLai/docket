//! The tier rule: a typed action first, a hook (D-Bus, portal, MPRIS) second, computer use last
//! and only where the person turned it on for the app.

use serde::{Deserialize, Serialize};

/// How the companion reaches something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// A typed action from the app's manifest.
    Typed,
    /// A hook the platform offers (D-Bus, a portal, MPRIS).
    Hook,
    /// Pixels: `org.quire.Cua` through `cua.run.start`.
    Cua,
}

/// Whether a tier is there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Offer {
    /// It is.
    Offered,
    /// It is not.
    Absent,
}

/// What reaches an app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Availability {
    /// A typed action covers the goal.
    pub typed: Offer,
    /// A hook covers it.
    pub hook: Offer,
    /// Computer use is on for the app (`cua.enabled`, off by default).
    pub cua: Offer,
}

/// The first tier that is offered, in order; none when nothing reaches the app (the answer is
/// `RefusalWire::NoWay`).
pub fn choose_tier(available: &Availability) -> Option<Tier> {
    [
        (Tier::Typed, available.typed),
        (Tier::Hook, available.hook),
        (Tier::Cua, available.cua),
    ]
    .into_iter()
    .find_map(|(tier, offer)| (offer == Offer::Offered).then_some(tier))
}
