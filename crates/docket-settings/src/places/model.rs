//! The setting's types. An invalid state has no value: this computer has no switch, an own
//! computer is on or off, and only a cloud account may be "ask me each time".

/// A place's name as the person sees it ("lab server", an account's address).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PlaceName(pub String);

/// A model a place offers, as data. The default view never shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelChoice(pub String);

/// The three kinds of place, closest first (the order routing prefers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlaceKind {
    /// This computer.
    ThisComputer,
    /// A computer the person owns.
    OwnComputer,
    /// A cloud account.
    Cloud,
}

/// How far a request's data may travel: the farthest kind of place it may reach. A privacy
/// floor, applied before any setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Floor(pub PlaceKind);

impl Floor {
    /// Whether a place of `kind` is within the floor.
    pub fn admits(self, kind: PlaceKind) -> bool {
        kind <= self.0
    }
}

/// An on/off choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toggle {
    /// May be used.
    On,
    /// May not be used.
    Off,
}

/// A cloud account's choice: the only place that may ask each time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudAllowed {
    /// Never used.
    Off,
    /// Used.
    On,
    /// The assistant asks before each use.
    AskEachTime,
}

/// A computer the person added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnComputer {
    /// Its name.
    pub name: PlaceName,
    /// On or off.
    pub allowed: Toggle,
    /// The model the person picked for it, if any.
    pub model: Option<ModelChoice>,
}

/// A cloud account that is signed in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudAccount {
    /// The account's name.
    pub name: PlaceName,
    /// The provider's name, for the confirmation.
    pub provider: String,
    /// Off until the person turns it on.
    pub allowed: CloudAllowed,
    /// The model the person picked for it, if any.
    pub model: Option<ModelChoice>,
}

/// One place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// This computer: always on, so it carries no switch.
    ThisComputer {
        /// The model the person picked, if any.
        model: Option<ModelChoice>,
    },
    /// One of the person's computers.
    OwnComputer(OwnComputer),
    /// A cloud account.
    Cloud(CloudAccount),
}

impl Place {
    /// Its kind.
    pub fn kind(&self) -> PlaceKind {
        match self {
            Place::ThisComputer { .. } => PlaceKind::ThisComputer,
            Place::OwnComputer(_) => PlaceKind::OwnComputer,
            Place::Cloud(_) => PlaceKind::Cloud,
        }
    }
}

/// The whole setting: this computer, then the person's computers, then cloud accounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Places {
    /// The model picked for this computer.
    pub this_computer: Option<ModelChoice>,
    /// The computers added.
    pub own: Vec<OwnComputer>,
    /// The accounts signed in.
    pub cloud: Vec<CloudAccount>,
}

impl Places {
    /// A machine with nothing added: this computer only.
    pub fn none() -> Self {
        Self {
            this_computer: None,
            own: Vec::new(),
            cloud: Vec::new(),
        }
    }

    /// Every place, closest kind first, each kind in the order listed.
    pub fn all(&self) -> Vec<Place> {
        let this = Place::ThisComputer {
            model: self.this_computer.clone(),
        };
        std::iter::once(this)
            .chain(self.own.iter().cloned().map(Place::OwnComputer))
            .chain(self.cloud.iter().cloned().map(Place::Cloud))
            .collect()
    }
}

impl Default for Places {
    fn default() -> Self {
        Self::none()
    }
}
