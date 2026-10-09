//! What the machine knows about (inferd's listing, as data) merged into the setting without ever
//! widening it: a place the setting has no row for starts Off.

use super::model::{CloudAccount, CloudAllowed, OwnComputer, PlaceName, Places, Toggle};

/// One place inferd lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnownPlace {
    /// A computer the person owns that inferd can reach.
    OwnComputer(PlaceName),
    /// A cloud account that is signed in, with its provider's name.
    Cloud(PlaceName, String),
}

/// What inferd lists.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Known(pub Vec<KnownPlace>);

impl Places {
    /// The setting with a row for every known place. A row the person set keeps its choice; a new
    /// one is Off, even for a computer inferd already used (the person has not said yes here).
    /// A setting row for a place no longer known is kept, so signing back in restores it.
    pub fn merged(mut self, known: &Known) -> Self {
        for place in &known.0 {
            match place {
                KnownPlace::OwnComputer(name) if !self.own.iter().any(|o| &o.name == name) => {
                    self.own.push(OwnComputer {
                        name: name.clone(),
                        allowed: Toggle::Off,
                        model: None,
                    });
                }
                KnownPlace::Cloud(name, provider)
                    if !self.cloud.iter().any(|c| &c.name == name) =>
                {
                    self.cloud.push(CloudAccount {
                        name: name.clone(),
                        provider: provider.clone(),
                        allowed: CloudAllowed::Off,
                        model: None,
                    });
                }
                _ => {}
            }
        }
        self
    }

    /// The setting of a machine that has only the older switches (`ai.local_only`, the floors):
    /// nothing in them says yes to a particular computer or account, so every known place is Off.
    /// The text is not read at all; that is the point.
    pub fn migrated(_older: &str, known: &Known) -> Self {
        Self::none().merged(known)
    }
}
