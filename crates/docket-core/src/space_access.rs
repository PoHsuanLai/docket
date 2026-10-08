//! Whose Space it is. An app's own Space (`app:<app>:<n>`) belongs to that app: no other app
//! acts in it, and no grant over it is held for another app. A Space shared across apps, the
//! outside (`desktop`) and "every Space" are open to all.

use crate::grant::{ActionGrantKey, GrantCaller};
use porter_core::AppName;
use prov::{SpaceId, SpaceScope};

/// Why an app may not use a Space: it is another app's own.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("That Space belongs to another app, so only that app can use it.")]
pub struct SpaceRefusal {
    /// The Space.
    pub space: SpaceId,
    /// The app it belongs to.
    pub owner: AppName,
}

/// Whether an app may use a Space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceAccess {
    /// It may.
    Open,
    /// It may not.
    Refused(SpaceRefusal),
}

/// What `app` may do with `scope`: porter's `open_to`, with the reason when it is no.
pub fn space_access(scope: &SpaceScope, app: &AppName) -> SpaceAccess {
    match scope {
        SpaceScope::Only(space) if !scope.open_to(app) => match space.owner() {
            Some(owner) => SpaceAccess::Refused(SpaceRefusal {
                space: space.clone(),
                owner,
            }),
            None => SpaceAccess::Open,
        },
        SpaceScope::Any | SpaceScope::Only(_) => SpaceAccess::Open,
    }
}

impl GrantCaller {
    /// The app behind the caller, when it is one acting on its own. The other callers are the
    /// person's agents and tools, which act where the person is: the call's own connection is
    /// checked when it is made.
    pub fn as_app(&self) -> Option<&AppName> {
        match self {
            GrantCaller::App(app) => Some(app),
            GrantCaller::Companion
            | GrantCaller::Cua
            | GrantCaller::Mcp(_)
            | GrantCaller::Cli
            | GrantCaller::Editor(_)
            | GrantCaller::AcpAgent(_) => None,
        }
    }
}

impl ActionGrantKey {
    /// Whether the app this grant is for may hold a grant over its Space.
    pub fn space_access(&self) -> SpaceAccess {
        match self.caller.as_app() {
            Some(app) => space_access(&self.space, app),
            None => SpaceAccess::Open,
        }
    }
}
