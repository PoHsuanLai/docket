//! The mailbox: what one helper hands to another, or to all, as handles and never as words to
//! obey. A post reaches the receiving planner only in its untrusted section, the way an app's
//! reply does; the router stamps its label with the join of everything the sender had read.

use super::job::JobId;
use prov::{EntityId, HelperId, Label};
use serde::{Deserialize, Serialize};

/// One post.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PostId(pub u64);

/// Who a post is for.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum PostTo {
    /// One helper.
    Helper(HelperId),
    /// Every helper, at the start of its next job.
    Group,
}

/// What a post carries: a handle to a thing, never text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum PostPart {
    /// The answer of a finished job.
    Result(JobId),
    /// A thing an app holds.
    Thing(EntityId),
}

/// A post.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Post {
    /// Its id.
    pub id: PostId,
    /// Who sent it. The sender is always a helper: a message cannot be the person's.
    pub from: HelperId,
    /// Who it is for.
    pub to: PostTo,
    /// What it carries.
    pub parts: Vec<PostPart>,
    /// What the sender had read: the join of its labels.
    pub label: Label,
    /// The helpers it has reached.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seen_by: Vec<HelperId>,
}

impl Post {
    /// Whether `helper` should be handed it: it is for them (or for all), they did not send it
    /// and have not had it.
    pub fn waits_for(&self, helper: &HelperId) -> bool {
        let for_them = match &self.to {
            PostTo::Helper(to) => to == helper,
            PostTo::Group => true,
        };
        for_them && &self.from != helper && !self.seen_by.contains(helper)
    }
}
