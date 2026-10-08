//! Shared by the acceptance scenarios: where the daemon binaries are, the cassettes the model
//! plays, and a failure that prints the daemons' logs.
#![allow(dead_code)]

use docket_accept::world::{Binaries, Cassette, World};

pub fn binaries() -> Binaries {
    Binaries {
        intentd: env!("CARGO_BIN_EXE_accept-intentd").into(),
        companiond: env!("CARGO_BIN_EXE_accept-companiond").into(),
        readerd: env!("CARGO_BIN_EXE_accept-readerd").into(),
        memoryd: env!("ACCEPT_MEMORYD").into(),
        inferd: env!("ACCEPT_INFERD").into(),
        quire_do: env!("CARGO_BIN_EXE_accept-quire-do").into(),
    }
}

/// Flow (a), the person allows: search, contact search, forward, the closing words.
pub const FLOW_A: Cassette = Cassette(include_str!(
    "../../../../dev/accept/cassettes/flow-a.jsonl"
));
/// Flow (a) by the handles the planner is shown for what the searches found: the cassette's
/// forward entry needs `#1 mail.thread, #2 mail.thread` and `#3 mail.contact` in the request.
pub const FLOW_A_HANDLES: Cassette = Cassette(include_str!(
    "../../../../dev/accept/cassettes/flow-a-handles.jsonl"
));
/// The same, but the first forward names handle 99 for the recipient; the second needs the router's
/// refusal in the history ("names a handle that does not exist", "handles you hold: #1 #2 #3").
pub const FLOW_A_FAULT_LINE: Cassette = Cassette(include_str!(
    "../../../../dev/accept/cassettes/flow-a-fault-line.jsonl"
));
/// The same, but the first forward leaves the recipient out; the second needs the fault line for a
/// reply that could not be read as a call.
pub const FLOW_A_UNREAD: Cassette = Cassette(include_str!(
    "../../../../dev/accept/cassettes/flow-a-unread.jsonl"
));
/// Flow (a), the person refuses: the closing words need the planner to have been told so.
pub const FLOW_A_REFUSED: Cassette = Cassette(include_str!(
    "../../../../dev/accept/cassettes/flow-a-refused.jsonl"
));
/// Two searches in a Space that has no grants yet.
pub const FIRST_USE: Cassette = Cassette(include_str!(
    "../../../../dev/accept/cassettes/first-use.jsonl"
));
/// Flow (c): the injected thread, the reader, the send. Every planner entry refuses to answer a
/// view that shows the body.
pub const FLOW_C: Cassette = Cassette(include_str!(
    "../../../../dev/accept/cassettes/flow-c.jsonl"
));

/// The handle the planner is shown for the reader's answer (the body of the thread is handle 1);
/// the flow-c cassette names it in the send.
pub const SUMMARY_HANDLE: u64 = 2;

/// Panics with the daemons' logs beside the message.
pub fn fail(world: &World, why: &str) -> ! {
    panic!("{why}\n{}", world.logs());
}

/// Two turns: a search and its words, then words only (after the host's restart, in the tests
/// that restart it).
pub const ACP_TWO_TURNS: Cassette = Cassette(include_str!(
    "../../../../dev/accept/cassettes/acp-two-turns.jsonl"
));

/// Two turns whose policies differ: the second one's words allow contact searches too, which the
/// router asks the person about while it records the turn.
pub const ACP_WIDENS: Cassette = Cassette(include_str!(
    "../../../../dev/accept/cassettes/acp-widens.jsonl"
));
