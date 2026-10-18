//! What a helper that is an agent app adds to the card: the agent's own ways of signing in, what
//! the shell is asked to do for a way that runs in a terminal, and the agent's words, which are
//! untrusted. Sign-in is the agent's own: we know a helper is signed in only because the agent
//! opened a session, and we never read its login.

use prov::{AgentProgram, HelperId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What the agent app asks to be run, for a way of signing in that needs a terminal: arguments
/// appended to its usual command, and variables set over its usual ones. Both are the agent's
/// own words, passed on as given and never shown.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalLogin {
    /// Arguments to append to the agent's command.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// Variables to set over the agent's own.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// How a way of signing in is carried out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SignInHow {
    /// The agent signs itself in when it is asked to.
    Agent,
    /// The person signs in in a terminal window that runs the agent.
    Terminal(TerminalLogin),
}

/// One way of signing in, as the agent app lists it: its id, and the name it gives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignInWay {
    /// The id the agent knows it by.
    pub id: String,
    /// Its name, in the agent's own words ("Sign in with Claude account").
    pub name: String,
    /// How it is carried out.
    pub how: SignInHow,
}

/// A request to the shell: open a terminal window that runs the agent app's own command with
/// `login` added, inside the helper's sandbox and with the agent's login folder, so the sign-in
/// lands where the agent will read it. When the window closes the shell tells the companion, which
/// starts the helper again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalRequest {
    /// The helper the person is signing in for.
    pub helper: HelperId,
    /// The agent app to run.
    pub program: AgentProgram,
    /// What to add to its command.
    pub login: TerminalLogin,
}

/// What an agent app said in answer to a job. It is the agent model's text: the board labels it
/// untrusted, a planner reads it only as a handle and never as an instruction, and it never
/// becomes another helper's goal.
///
/// It crosses the bus between the board and the host of the agent apps as the plain text it is.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UntrustedWords(String);

impl UntrustedWords {
    /// The words as the agent gave them.
    pub fn new(text: String) -> Self {
        Self(text)
    }

    /// The words. Whoever reads them has to treat them as data.
    pub fn as_untrusted_str(&self) -> &str {
        &self.0
    }

    /// The words, given up.
    pub fn into_inner(self) -> String {
        self.0
    }
}

// The agent's words: Debug shows the length only.
impl std::fmt::Debug for UntrustedWords {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "UntrustedWords(<{} chars>)", self.0.chars().count())
    }
}
