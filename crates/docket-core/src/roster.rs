//! The companion's view of who else is working, and of what it remembers: the roster, recent
//! episodes, recalled items and the pinned facts. Built by the router from labelled sources, so
//! untrusted text is already a handle by the time a planner holds one.
//!
//! A line for an agent in another Space shows presence only (QUESTIONS Q8): Spaces are hard
//! walls, and the companion learns that something runs elsewhere, never what.

use crate::context::Reveal;
use crate::ids::Handle;
use crate::planner::StepLine;
use almanac_core::{EpisodeId, EpisodeOutcome, MemoryItem, RecallWhy};
use prov::{AgentRef, SpaceId, UnixSeconds};
use serde::{Deserialize, Serialize};
use std::fmt;

/// How many characters of the person's words a roster line quotes.
pub const TOLD_LEAD_CHARS: usize = 80;

/// The first [`TOLD_LEAD_CHARS`] characters of something the person told an agent.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LeadText(String);

impl LeadText {
    /// The lead of `text`: its first 80 characters, cut on a character boundary.
    pub fn of(text: &str) -> Self {
        Self(text.chars().take(TOLD_LEAD_CHARS).collect())
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// The person's words: Debug shows the length only.
impl fmt::Debug for LeadText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LeadText(<{} chars>)", self.0.chars().count())
    }
}

/// Where an agent stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RosterState {
    /// Being set up.
    Starting,
    /// Working.
    Working,
    /// Waiting for the person.
    NeedsYou,
    /// Paused (a halt, a breaker, the person).
    Paused,
    /// Finished.
    Done,
    /// Could not finish.
    Failed,
    /// Stopped before finishing.
    Cancelled,
}

/// What the roster tells about an agent in the person's own Space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterFull {
    /// What it is doing; the person's words are plain, anything else a handle.
    pub goal: Reveal<String>,
    /// Its last step.
    pub last: Option<StepLine>,
    /// What the person last told it directly, so the front thread knows at once, in trusted
    /// words, that they redirected it.
    pub told: Option<LeadText>,
}

/// How much a roster line tells.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum RosterDetail {
    /// The full line.
    Full(Box<RosterFull>),
    /// Only that it exists and where it stands (another Space).
    PresenceOnly,
}

/// One agent on the roster.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterLine {
    /// Which agent.
    pub agent: AgentRef,
    /// Its Space.
    pub space: SpaceId,
    /// Where it stands.
    pub state: RosterState,
    /// What is told about it.
    pub detail: RosterDetail,
}

/// The agents that are working or just finished.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Roster {
    /// One line each.
    pub entries: Vec<RosterLine>,
}

impl Roster {
    /// The roster as an agent working in `active` may see it: a line from another Space keeps
    /// its agent, Space and state and loses its goal, last step and told line.
    pub fn seen_from(&self, active: &SpaceId) -> Roster {
        let entries = self
            .entries
            .iter()
            .map(|line| {
                if &line.space == active {
                    line.clone()
                } else {
                    RosterLine {
                        detail: RosterDetail::PresenceOnly,
                        ..line.clone()
                    }
                }
            })
            .collect();
        Roster { entries }
    }
}

/// An episode's skeleton as lines the planner reads: trusted material only.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SkeletonText(pub String);

impl fmt::Debug for SkeletonText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SkeletonText(<{} bytes>)", self.0.len())
    }
}

/// One recent episode, as the planner reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeLine {
    /// Which episode.
    pub id: EpisodeId,
    /// Whose.
    pub agent: AgentRef,
    /// Its Space.
    pub space: SpaceId,
    /// When it ended.
    pub ended: UnixSeconds,
    /// How it ended.
    pub outcome: EpisodeOutcome,
    /// What happened, as trusted lines.
    pub skeleton: SkeletonText,
    /// A model-written narrative, by handle: reading it taints the task.
    pub narrative: Option<Handle>,
}

/// One item recall brought up for this turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecalledLine {
    /// The fact or event.
    pub doc: MemoryItem,
    /// When it happened or was written.
    pub at: UnixSeconds,
    /// Its text: plain when trusted, a handle when not.
    pub text: Reveal<String>,
    /// Why it was found.
    pub why: RecallWhy,
}

macro_rules! trusted_text {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "(<{} bytes>)"), self.0.len())
            }
        }
    };
}

trusted_text!(
    /// The Space's primer: at most 200 lines of trusted facts.
    PrimerText
);
trusted_text!(
    /// One pinned fact the person stated themselves at the desktop scope.
    ProfileLine
);
trusted_text!(
    /// The latest day or week digest line.
    RollupLine
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planner::StepLine;

    fn space(s: &str) -> SpaceId {
        SpaceId::parse(s).expect("space")
    }

    fn full(agent: AgentRef, in_space: &str) -> RosterLine {
        RosterLine {
            agent,
            space: space(in_space),
            state: RosterState::Working,
            detail: RosterDetail::Full(Box::new(RosterFull {
                goal: Reveal::Plain("tidy the downloads".into()),
                last: None::<StepLine>,
                told: Some(LeadText::of("skip the newsletters")),
            })),
        }
    }

    #[test]
    fn another_space_shows_presence_only() {
        let run = AgentRef::Cua {
            run: prov::RunId::parse("r-1").expect("run"),
        };
        let roster = Roster {
            entries: vec![full(AgentRef::Companion, "work"), full(run.clone(), "home")],
        };
        let seen = roster.seen_from(&space("work"));
        assert!(matches!(seen.entries[0].detail, RosterDetail::Full(_)));
        assert_eq!(seen.entries[1].detail, RosterDetail::PresenceOnly);
        assert_eq!(seen.entries[1].agent, run);
        assert_eq!(seen.entries[1].state, RosterState::Working);
        let json = serde_json::to_string(&seen.entries[1]).expect("json");
        assert!(
            !json.contains("downloads") && !json.contains("newsletters"),
            "{json}"
        );
    }

    #[test]
    fn the_told_lead_is_eighty_characters_on_a_boundary() {
        let long = "é".repeat(200);
        let lead = LeadText::of(&long);
        assert_eq!(lead.as_str().chars().count(), TOLD_LEAD_CHARS);
        assert_eq!(LeadText::of("short").as_str(), "short");
    }
}
