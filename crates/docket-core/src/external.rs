//! An external agent a session runs: a configured program behind the ACP client edge. The host
//! process that launched it opens the session and says which program it is; the router never
//! takes the name from the agent, which is a model's.

use crate::grant::ProgramName;
use serde::{Deserialize, Serialize};

/// Where the person answers a sheet for a call in an external agent's session.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SheetSurface {
    /// The desktop's sheet (sill), like any other call: the normal confirm path.
    #[default]
    Desktop,
    /// The host that launched the agent, which shows the sheet itself. Only a development
    /// fallback (`docket-agent --tty`): the router sends it nothing unless the host asked.
    Host,
}

/// The external agent a session is for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalAgent {
    /// The configured program (`agents.toml`), as the host that launched it names it.
    pub program: ProgramName,
    /// Where its sheets are answered.
    #[serde(default)]
    pub sheets: SheetSurface,
    /// What the person calls it (`label` in `agents.toml`), as the host that launched it says.
    /// Never read from the agent, which is a model's: it is shown in the audit and the journal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<prov::AgentLabel>,
}
