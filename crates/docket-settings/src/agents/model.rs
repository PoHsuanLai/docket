//! The rows' types. An invalid state has no value: an agent that never started has no offers, a
//! model is chosen or it is the agent's own, and only one that needs signing in lists ways.

use docket_agents::offered::Named;
use docket_core::Rewind;

/// Where an agent's program came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Installed from the agent registry, at the pinned version.
    Registry {
        /// The registry's id.
        id: String,
        /// The pinned version.
        version: String,
        /// Whether that version is there.
        installed: Install,
    },
    /// A program the person named by its path.
    Own,
}

/// Whether the pinned version is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Install {
    /// It is.
    Installed,
    /// It is not (never installed, or removed).
    NotInstalled,
}

/// What the agent last offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offers {
    /// The agent never started: nothing is known.
    NotSeenYet,
    /// The models it listed at its last start (none when it offers no choice), and the one in use.
    Seen {
        /// The models on offer.
        models: Vec<Named>,
        /// The one the session ran, when the agent said.
        in_use: Option<String>,
    },
}

/// Whether the model chosen is still one the agent offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    /// Offered, under this name.
    Offered(String),
    /// The agent listed models and this is not one of them.
    NoLongerOffered,
    /// The agent never listed any.
    NotCheckedYet,
}

/// The model the person chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelState {
    /// None: the agent runs its own.
    AgentsOwn,
    /// One, by the id `agents.toml` writes.
    Chosen {
        /// The id.
        id: String,
        /// Whether the agent still offers it.
        availability: Availability,
    },
}

/// Whether the agent was signed in at its last start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignInState {
    /// The agent never started.
    NotSeenYet,
    /// The session opened.
    SignedIn,
    /// The agent asked for a sign-in first; the ways it listed.
    NeedsSignIn {
        /// The ways of signing in.
        ways: Vec<Named>,
    },
    /// The last start did not get as far as sign-in.
    Unknown,
}

/// One agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRow {
    /// The name `agents.toml` lists it under.
    pub program: String,
    /// What the person sees: the label, else the name.
    pub label: String,
    /// Where it came from.
    pub source: Source,
    /// The model chosen.
    pub model: ModelState,
    /// What it offered.
    pub offers: Offers,
    /// Whether it was signed in.
    pub sign_in: SignInState,
    /// The way of signing in the person chose, by id.
    pub way: Option<String>,
    /// Who keeps the history of the files it changes: what `checkpoints` says, else `Agent` for
    /// the `claude-code` profile and `Docket` for any other.
    pub rewind: Rewind,
}

/// Every agent in `agents.toml`, in the order listed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Agents {
    /// The rows.
    pub rows: Vec<AgentRow>,
}

impl Agents {
    /// The row of `program`.
    pub fn get(&self, program: &str) -> Option<&AgentRow> {
        self.rows.iter().find(|r| r.program == program)
    }
}
