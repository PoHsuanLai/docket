//! The request to refresh what an agent offers: start it briefly in its sandbox, open a session,
//! read the models and sign-in ways, write the record, close it. docket has no settings bus; the
//! Settings app makes the request by running [`REFRESH_PROGRAM`] with [`RefreshRequest::arguments`],
//! and reads the rows again when it exits (any exit status: a start that needed signing in is
//! still written down). An agent starts only on this request, never when a page opens.

use super::model::AgentRow;

/// The program that does the refresh, found beside the other docket programs.
pub const REFRESH_PROGRAM: &str = "docket-agent";

/// A request to refresh one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshRequest {
    program: String,
}

impl RefreshRequest {
    /// The request for the agent in `row`.
    pub fn of(row: &AgentRow) -> Self {
        Self {
            program: row.program.clone(),
        }
    }

    /// The agent's name in `agents.toml`.
    pub fn program(&self) -> &str {
        &self.program
    }

    /// The arguments [`REFRESH_PROGRAM`] is run with.
    pub fn arguments(&self) -> Vec<String> {
        vec![self.program.clone(), "--refresh".to_owned()]
    }
}
