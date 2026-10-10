//! Why an agent could not be built or an ask could not start.

use crate::actions::Missing;
use docket_client::ClientError;
use docket_planner::RoleFault;

/// Why a builder did not make an agent.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum BuildFault {
    /// The role text is not usable.
    #[error("role: {0}")]
    Role(#[from] RoleFault),
    /// A choice of actions named something the set did not hold.
    #[error("the actions chosen name something that is not there: {0:?}")]
    Missing(Missing),
}

/// Why an ask did not run at all. Once it runs, every way it can end is a [`crate::Ended`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum KitFault {
    /// The router did not open the session or record the turn.
    #[error("the router: {0}")]
    Router(ClientError),
}
