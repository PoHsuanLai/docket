//! What starts a routine: a time, an event, or the end of another job.

use super::schedule::Schedule;
use crate::helper::JobId;
use almanac_core::KindPattern;
use prov::SpaceId;
use serde::{Deserialize, Serialize};

/// Which almanac events start a routine: a kind (new mail, a file in a Space) in a Space. The
/// event's content never reaches the routine's goal; the run is handed the event as a handle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventMatch {
    /// The kind of event.
    pub kind: KindPattern,
    /// The Space it happens in; none means the helper's own.
    pub space: Option<SpaceId>,
    /// What this is, in the person's words ("New mail in Inbox"): what is shown for it.
    pub words: String,
}

impl EventMatch {
    /// Whether an event of `kind` in `space` is one this match names.
    pub fn covers(&self, kind: &str, space: &SpaceId) -> bool {
        self.kind.covers(kind) && self.space.as_ref().is_none_or(|own| own == space)
    }
}

/// The job whose end starts a routine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobRef(pub JobId);

/// What starts a routine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Trigger {
    /// A time rule.
    At(Schedule),
    /// An almanac event.
    On(EventMatch),
    /// Another job ending done.
    AfterJob(JobRef),
}

impl Trigger {
    /// As a person says it.
    pub fn plain(&self) -> String {
        match self {
            Trigger::At(schedule) => schedule.plain(),
            Trigger::On(event) => event.words.clone(),
            Trigger::AfterJob(_) => "When an earlier job is done".to_owned(),
        }
    }
}
