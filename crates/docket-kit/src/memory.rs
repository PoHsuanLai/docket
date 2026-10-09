//! What an agent remembers, chosen section by section. Every section is off until it is asked
//! for, and each is read through the router (`Session.Recall`), which labels it: an untrusted
//! text is a handle by the time it is here. The budgets are the assembler's
//! (`AgentConfig::assembler`), overridden on the builder.

use almanac_core::{BodyMode, InjectQuery, RecallOver, RecentQuery, TrustFilter, UserText};
use docket_client::{Intents, Transport as IntentsTransport};
use docket_core::{AssemblerBudget, EpisodeLine, PrimerText, ProfileLine, RecalledLine, Seconds};
use docket_tasks::{episodes_of, hits_of, primer_of, profile_of};
use porter_core::{Count, UnixSeconds};
use prov::{SessionId, SpaceId};

/// How many hits the recall asks for before the token budget cuts them.
pub const RECALL_HITS: Count = Count(8);
/// How many recent episodes at most.
pub const EPISODE_LIMIT: Count = Count(20);

/// Whose memory a recall looks in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecallScope {
    /// The Space the ask is made in.
    AskedSpace,
    /// This Space (the router still narrows it to what the session may read).
    Space(SpaceId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Recall {
    Off,
    On(RecallScope, TrustFilter),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Episodes {
    Off,
    Within(Seconds),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Off,
    On,
}

/// The memory sections of one agent. Start from [`Memory::none`] and add what it needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Memory {
    recall: Recall,
    episodes: Episodes,
    profile: Section,
    primer: Section,
}

/// The sections read for one turn.
#[derive(Debug, Default)]
pub(crate) struct Filled {
    pub profile: Vec<ProfileLine>,
    pub primer: Option<PrimerText>,
    pub episodes: Vec<EpisodeLine>,
    pub recalled: Vec<RecalledLine>,
}

/// What a read needs to know about the ask.
pub(crate) struct Where<'a> {
    pub session: &'a SessionId,
    pub space: &'a SpaceId,
    pub now: UnixSeconds,
    pub words: &'a str,
}

impl Memory {
    /// Nothing is read from memory: the agent works from its role, its words and its steps.
    pub fn none() -> Self {
        Self {
            recall: Recall::Off,
            episodes: Episodes::Off,
            profile: Section::Off,
            primer: Section::Off,
        }
    }

    /// Recall what bears on the words of the ask, from `scope`, for these provenances.
    /// `TrustFilter::TrustedOnly` is the assembler's own choice.
    pub fn recall(self, scope: RecallScope, trust: TrustFilter) -> Self {
        Self {
            recall: Recall::On(scope, trust),
            ..self
        }
    }

    /// Show the episodes that ended in the Space within `window` before the ask.
    pub fn episodes(self, window: Seconds) -> Self {
        Self {
            episodes: Episodes::Within(window),
            ..self
        }
    }

    /// Show the facts the person stated about themselves.
    pub fn profile(self) -> Self {
        Self {
            profile: Section::On,
            ..self
        }
    }

    /// Show the Space's primer.
    pub fn primer(self) -> Self {
        Self {
            primer: Section::On,
            ..self
        }
    }

    /// Reads the chosen sections; a router that does not answer leaves its section empty.
    pub(crate) async fn fill<I: IntentsTransport>(
        &self,
        intents: &Intents<I>,
        budget: &AssemblerBudget,
        at: &Where<'_>,
    ) -> Filled {
        let session = at.session;
        let profile = match self.profile {
            Section::On => profile_of(intents, session).await,
            Section::Off => Vec::new(),
        };
        let primer = match self.primer {
            Section::On => primer_of(intents, session).await,
            Section::Off => None,
        };
        let episodes = match self.episodes {
            Episodes::Within(window) => {
                let query = RecentQuery {
                    since: UnixSeconds(at.now.0.saturating_sub(i64::from(window.0))),
                    kinds: Vec::new(),
                    trust: TrustFilter::Any,
                    limit: EPISODE_LIMIT,
                    bodies: BodyMode::Json,
                };
                episodes_of(intents, session, query).await
            }
            Episodes::Off => Vec::new(),
        };
        let recalled = match &self.recall {
            Recall::On(scope, trust) if !at.words.is_empty() => {
                let space = match scope {
                    RecallScope::AskedSpace => at.space.clone(),
                    RecallScope::Space(space) => space.clone(),
                };
                let query = InjectQuery {
                    space,
                    text: UserText::new(at.words.to_owned()),
                    budget: budget.recall,
                    k: RECALL_HITS,
                    over: RecallOver::Both,
                    trust: *trust,
                };
                hits_of(intents, session, query).await
            }
            Recall::On(..) | Recall::Off => Vec::new(),
        };
        Filled {
            profile,
            primer,
            episodes,
            recalled,
        }
    }
}
