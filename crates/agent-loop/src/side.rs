//! Side conversations: the person opens a worker's or a run's row and types to it. Each turn is
//! recorded as the person's own trusted words; when the row closes, or two minutes pass without
//! a turn, a side episode is written from those turns and the subagent's typed steps since. A
//! goal change narrows the subagent's task policy through the normal writer path (widening
//! confirms).

use almanac_core::{Episode, EpisodeKind, EpisodeOutcome, ResultLine};
use docket_core::{IdleRules, LedgerStep, TaskLedger, UserTurn, close};
use prov::{AgentRef, SpaceId, TaskId, UnixSeconds};
use serde::{Deserialize, Serialize};

/// One open side conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SideConv {
    /// The subagent.
    pub with: AgentRef,
    /// Its Space.
    pub space: SpaceId,
    /// When the person first spoke to it.
    pub opened: UnixSeconds,
    /// When they last did.
    pub last: UnixSeconds,
    /// What they said, verbatim.
    pub turns: Vec<UserTurn>,
}

/// The open side conversations, one per subagent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SideTable {
    /// The conversations.
    pub open: Vec<SideConv>,
}

/// What happens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SideInput {
    /// The person said something to a subagent.
    Told {
        /// The subagent.
        to: AgentRef,
        /// Its Space.
        space: SpaceId,
        /// What they said.
        turn: UserTurn,
    },
    /// The person closed the row.
    RowClosed(AgentRef),
    /// Time passed.
    Tick(UnixSeconds),
}

/// Why a side conversation ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEnd {
    /// The row was closed.
    RowClosed,
    /// Two minutes without a turn.
    Idle,
}

/// What companiond does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SideEffect {
    /// Narrow the subagent's policy from this turn through the policy writer (widening asks).
    Rederive {
        /// The subagent.
        agent: AgentRef,
        /// The turn.
        turn: UserTurn,
    },
    /// Write the side episode.
    WriteEpisode {
        /// The conversation.
        conv: SideConv,
        /// Why it ended.
        end: SideEnd,
    },
}

/// One transition. A subagent has one open conversation; a turn to it extends that one.
pub fn side_step(
    table: SideTable,
    input: SideInput,
    rules: &IdleRules,
) -> (SideTable, Vec<SideEffect>) {
    let mut open = table.open;
    let mut effects = Vec::new();
    match input {
        SideInput::Told { to, space, turn } => {
            effects.push(SideEffect::Rederive {
                agent: to.clone(),
                turn: turn.clone(),
            });
            let at = turn.at;
            match open.iter_mut().find(|c| c.with == to) {
                Some(conv) => {
                    conv.last = at;
                    conv.turns.push(turn);
                }
                None => open.push(SideConv {
                    with: to,
                    space,
                    opened: at,
                    last: at,
                    turns: vec![turn],
                }),
            }
        }
        SideInput::RowClosed(agent) => {
            if let Some(i) = open.iter().position(|c| c.with == agent) {
                effects.push(SideEffect::WriteEpisode {
                    conv: open.remove(i),
                    end: SideEnd::RowClosed,
                });
            }
        }
        SideInput::Tick(now) => {
            let (idle, live): (Vec<_>, Vec<_>) = open
                .into_iter()
                .partition(|c| now.0 - c.last.0 >= i64::from(rules.side_close.0));
            open = live;
            effects.extend(idle.into_iter().map(|conv| SideEffect::WriteEpisode {
                conv,
                end: SideEnd::Idle,
            }));
        }
    }
    (SideTable { open }, effects)
}

/// The id a side episode gets: the subagent's task or run, and when the conversation opened.
fn side_id(conv: &SideConv) -> String {
    let who = match &conv.with {
        AgentRef::Worker { task } => task.as_str().to_owned(),
        AgentRef::Cua { run } => run.as_str().to_owned(),
        AgentRef::Companion => "companion".to_owned(),
        AgentRef::User => "user".to_owned(),
    };
    format!("side-{who}-{}", conv.opened.0)
}

/// The side episode of a finished conversation: the person's turns verbatim and the subagent's
/// typed steps since, as a trusted skeleton with no narrative. `None` when the id would not be
/// a valid episode id.
pub fn side_episode(
    conv: &SideConv,
    steps: Vec<LedgerStep>,
    results: Vec<ResultLine>,
    ended: UnixSeconds,
) -> Option<Episode> {
    let ledger = TaskLedger {
        task: TaskId::parse(&side_id(conv)).ok()?,
        agent: conv.with.clone(),
        parent: None,
        space: conv.space.clone(),
        started: conv.opened,
        asked: conv.turns.clone(),
        steps,
        touched: vec![],
        results,
    };
    close(&ledger, EpisodeKind::Side, ended, EpisodeOutcome::Done)
}
