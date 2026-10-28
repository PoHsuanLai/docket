//! What a policy writer hands the router: the policy, and what checking the model's draft had to
//! change in it, so a trace shows a correction instead of hiding it. Also the writer's seam.

use crate::TaskPolicy;
use crate::planner::{ActionCard, UserTurn};
use crate::review::ReviewError;
use prov::{Effect, SpaceId, TaskId};
use serde::{Deserialize, Serialize};
use std::future::Future;

/// One thing the checker changed in a model's draft because the draft contradicted itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Corrected {
    /// The draft's ceiling was below the effect of an action it chose itself, and was raised to
    /// that effect. The actions list is untouched, so nothing more is allowed than was chosen.
    CeilingRaised {
        /// What the draft said.
        from: Effect,
        /// The highest effect among the actions it chose.
        to: Effect,
    },
}

/// A derived policy and the corrections made to the draft it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Derived {
    /// The checked policy.
    pub policy: TaskPolicy,
    /// What was corrected, empty when the draft was consistent.
    pub corrected: Vec<Corrected>,
}

impl Derived {
    /// A policy nothing was corrected in.
    pub fn plain(policy: TaskPolicy) -> Self {
        Self {
            policy,
            corrected: Vec::new(),
        }
    }
}

/// Derives a policy from the person's turns alone. Its input is the router-held turns and the
/// action catalogue; it never sees content, tool output or planner prose.
pub trait PolicyWriter: Send + Sync {
    /// The policy for these turns in this Space, or why there is none. The router stamps the
    /// result's `from` and `expires`; `task` is the task it is for.
    fn derive(
        &self,
        task: &TaskId,
        turns: &[UserTurn],
        catalogue: &[ActionCard],
        space: &SpaceId,
    ) -> impl Future<Output = Result<Derived, ReviewError>> + Send;
}
