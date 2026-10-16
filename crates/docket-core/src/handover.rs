//! Handing the assistant's turn to an outside agent. When the person chose a coding agent as the
//! assistant's model, companiond does not run its planner for their turn: it asks the agent host
//! (`docket-agent`, the process that plays `org.quire.AcpAgent`) to give the turn to that agent,
//! and shows what the agent said. These are the two bodies of that one call, `AgentHost1.HandOver`.
//!
//! The host, not companiond, opens the agent's session at the router and makes the agent's calls,
//! so everything an agent does is ruled exactly as for `docket-agent` on a terminal.
//!
//! A turn can take longer than the bus lets one call wait, so the call is two: `HandOver` starts
//! the turn and answers its id, and `Wait` is asked again until the turn has ended.

use crate::grant::ProgramName;
use crate::planner::UserTurn;
use prov::SpaceId;
use serde::{Deserialize, Serialize};

/// The turn to hand over: which agent (the name `agents.toml` lists it under), the Space the
/// conversation is in, and the person's words as the shell recorded them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandOver {
    /// The agent.
    pub program: ProgramName,
    /// The conversation's Space.
    pub space: SpaceId,
    /// The person's turn.
    pub turn: UserTurn,
}

/// What the agent made of the turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Handed {
    /// What it said, as the answer's words.
    pub words: String,
    /// What the person calls it ("Claude Code"): the label `agents.toml` gives, else the name.
    pub label: String,
}

/// Why the turn was not handed over, in a form the assistant says in plain words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
#[non_exhaustive]
pub enum HandRefusal {
    /// Outside agents are switched off (`agent.acp.agents`).
    TurnedOff,
    /// No such agent is listed in `agents.toml`.
    NotSetUp,
    /// The agent needs the person to sign in first.
    NeedsSignIn {
        /// What the person calls it.
        label: String,
    },
    /// The host is switching to another agent and starts again; the person asks again.
    Switching,
    /// The agent could not be started or failed on the turn.
    Failed {
        /// Why, in words the person reads.
        message: String,
    },
}

/// The host's answer to a [`HandOver`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum HandReply {
    /// The agent took the turn.
    Handed(Handed),
    /// It did not.
    Refused(HandRefusal),
}

impl HandRefusal {
    /// What the assistant tells the person.
    pub fn sentence(&self) -> String {
        match self {
            HandRefusal::TurnedOff => {
                "Coding agents are turned off. Turn them on in Settings to use one.".to_owned()
            }
            HandRefusal::NotSetUp => {
                "The coding agent you chose is not set up on this computer any more. Choose another in Settings.".to_owned()
            }
            HandRefusal::NeedsSignIn { label } => format!("Sign in to {label} first."),
            HandRefusal::Switching => {
                "Switching to another coding agent. Ask again in a moment.".to_owned()
            }
            HandRefusal::Failed { message } => message.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_refusal_says_itself_in_plain_words() {
        let rows = [
            (HandRefusal::TurnedOff, "turned off"),
            (HandRefusal::NotSetUp, "not set up"),
            (HandRefusal::Switching, "Ask again"),
            (
                HandRefusal::NeedsSignIn {
                    label: "Claude Code".to_owned(),
                },
                "Sign in to Claude Code first.",
            ),
            (
                HandRefusal::Failed {
                    message: "It stopped.".to_owned(),
                },
                "It stopped.",
            ),
        ];
        for (refusal, expected) in rows {
            let text = refusal.sentence();
            assert!(text.contains(expected), "{refusal:?}: {text}");
            for word in ["ACP", "MCP", "protocol", "CLI"] {
                assert!(!text.contains(word), "{refusal:?} says {word}");
            }
        }
    }

    #[test]
    fn a_reply_round_trips_as_json() {
        let reply = HandReply::Refused(HandRefusal::NeedsSignIn {
            label: "Claude Code".to_owned(),
        });
        let text = serde_json::to_string(&reply).expect("json");
        assert_eq!(
            serde_json::from_str::<HandReply>(&text).expect("back"),
            reply
        );
    }
}
