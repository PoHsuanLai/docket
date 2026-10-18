//! The ACP server edge (design note `acp-sessions.md`, section 3a): an editor drives the
//! companion over the Agent Client Protocol.
//!
//! - `Server`: the protocol over a `Wire`, a `SessionHost` and a `SessionLog`. It holds no
//!   authority; every call a session makes goes through the host's gate.
//! - `Permit`: the proof that `agent.acp.expose` is on. No permit, no server.
//! - `turn`: one prompt turn as a pure step machine. `calls`, `permission`, `mode`, `history`:
//!   the mappings from our step lines, the editor's extra gate, the modes and the stored
//!   sessions.
//!
//! - `Helpers`: the seam through which an editor sees the person's helpers (`session/list` shows
//!   them, `session/load` replays their recent jobs, `session/new` with `_meta.helper` opens a job)
//!   and the board's sentences reach it as plain lines. The editor never answers a helper's sheet
//!   for the helper: its "always" is a grant for that helper alone.
//! - `client` (feature `client`): the other direction (S4), an external agent as a session backend.
//! - `Terminals`: the `terminal/*` client methods over the sandboxed shell tool (`bulkhead`),
//!   ruled by the `Execute` rules. They run in our sandbox, never in an editor's terminal.
//!
//! The protocol's wire types are `agent-client-protocol-schema`'s (Apache-2.0); the transport is
//! ours. `rawInput` and `rawOutput` are never sent; `allow_always` is offered only on a router
//! sheet that offered it.

#[cfg(feature = "client")]
pub mod bulk;
mod calls;
pub mod client;
mod command_deadline;
mod covered;
mod expose;
mod fault;
mod helper_face;
mod helper_replay;
mod helpers;
mod history;
mod mode;
mod out;
mod permission;
mod prompt;
mod scope_words;
mod server;
mod sessions;
mod sheet;
#[cfg(feature = "server")]
mod stdio;
mod terminals;
mod turn;
mod wire;

pub use calls::{Outcome, outcome};
pub use command_deadline::CommandDeadline;
pub use covered::Covered;
pub use expose::{Permit, Refusal};
pub use helpers::{HelperRefusal, HelperRow, Helpers, NoHelpers};
pub use history::rfc3339;
pub use mode::{Mode, Say};
pub use permission::{
    ALLOW_ALWAYS, ALLOW_ONCE, REJECT_ONCE, Verdict, choice, choice_of_reply, options,
    sheet_options, sheet_request, verdict, verdict_of_reply,
};
pub use scope_words::always_words;
pub use server::{Server, Ticks};
#[cfg(feature = "server")]
pub use stdio::{LINE_MAX, LineFault, LineWire, SystemTicks, read_capped};
pub use terminals::Terminals;
pub use turn::{Finish, Order, Turn, Wants};
pub use wire::{Incoming, NotJsonRpc, Wire, WireClosed};
