//! What the models behind the policy writer and the reviewers say in a case, word for word. A
//! case that leaves a stage out gets the hijacked judge's answer for it (the writer picks every
//! action up to destructive, the quick judge passes, the larger stages allow), so a case names
//! only the stage it spoils.
//!
//! The words are the model's own reply, raw: a fenced block, a verdict with an extra key, an
//! empty string. They are not checked here; the point is that the code reading them is. A live
//! run plays them through inferd's replay engine (`docket-live`), the gate's deterministic suite
//! through `action_review::parse_verdict`.

use docket_core::Stage;
use serde::{Deserialize, Serialize};

/// The raw replies of the stages a case spoils.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelScript {
    /// The policy writer's reply (a JSON draft, or not).
    #[serde(default)]
    pub writer: Option<String>,
    /// The quick judge's reply: `pass`, `flag`, or something else.
    #[serde(default)]
    pub quick: Option<String>,
    /// The deliberate reviewer's reply.
    #[serde(default)]
    pub deliberate: Option<String>,
    /// The second opinion's reply.
    #[serde(default)]
    pub second: Option<String>,
}

impl ModelScript {
    /// Whether no stage is spoiled.
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }

    /// What the reviewer of `stage` says, if the case spoils it.
    pub fn reviewer(&self, stage: Stage) -> Option<&str> {
        match stage {
            Stage::Quick => self.quick.as_deref(),
            Stage::Deliberate => self.deliberate.as_deref(),
            Stage::SecondOpinion => self.second.as_deref(),
        }
    }
}
