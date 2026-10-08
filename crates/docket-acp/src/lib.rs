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
//! The protocol's wire types are `agent-client-protocol-schema`'s (Apache-2.0); the transport is
//! ours. `rawInput` and `rawOutput` are never sent; `allow_always` is offered only on a router
//! sheet that offered it.

mod calls;
mod covered;
mod expose;
mod fault;
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
mod turn;
mod wire;

pub use calls::{Outcome, outcome};
pub use covered::Covered;
pub use expose::{Permit, Refusal};
pub use history::rfc3339;
pub use mode::{Mode, Say};
pub use permission::{
    ALLOW_ALWAYS, ALLOW_ONCE, REJECT_ONCE, Verdict, choice, choice_of_reply, options,
    sheet_options, sheet_request, verdict, verdict_of_reply,
};
pub use scope_words::always_words;
pub use server::{Server, Ticks};
#[cfg(feature = "server")]
pub use stdio::{LineWire, SystemTicks};
pub use turn::{Finish, Order, Turn, Wants};
pub use wire::{Incoming, NotJsonRpc, Wire, WireClosed};
