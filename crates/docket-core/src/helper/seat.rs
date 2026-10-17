//! What the router is told when a helper's job opens its session: which helper it works for, the
//! ceiling its policy is cut to, and the things it alone may write.

use crate::helper::HelperName;
use crate::task_policy::TaskPolicy;
use prov::{EntityId, HelperId};
use serde::{Deserialize, Serialize};

/// A helper's job's place in the router. Only the companion's own role may say it; from anyone
/// else the router ignores it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperSeat {
    /// The helper.
    pub helper: HelperId,
    /// What the person calls it: its sheets name it.
    pub name: HelperName,
    /// Its reach: the session's task policy is the intersection of what its words derive and this.
    pub reach: TaskPolicy,
    /// The things the job owns: a write to one from any other session is refused while this
    /// session lives.
    pub owns: Vec<EntityId>,
}
