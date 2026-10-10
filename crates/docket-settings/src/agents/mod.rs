//! The external coding agents in `agents.toml`, as the Settings app draws them: one row each with
//! its label, where it came from, whether the pinned version is installed, the model chosen and
//! the ones the agent last offered, and whether it was signed in. What the agent offered is the
//! record docket wrote when it last started (`docket_agents::offered`); nothing here starts an
//! agent. Starting one to refresh the record is an explicit [`RefreshRequest`], never a page
//! opening.

mod model;
mod read;
mod refresh;
mod words;
mod write;

pub use model::{AgentRow, Agents, Availability, Install, ModelState, Offers, SignInState, Source};
pub use read::read_agents;
pub use refresh::{REFRESH_PROGRAM, RefreshRequest};
pub use write::{
    AgentChoice, ChoiceFault, Pick, edit_agent_choice, edit_agent_rewind, write_agent_choice,
    write_agent_rewind,
};
