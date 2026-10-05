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
    }
}

/// Flow (a), the person allows: search, contact search, forward, the closing words.
pub const FLOW_A: Cassette = Cassette(include_str!("../../../dev/accept/cassettes/flow-a.jsonl"));
/// Flow (a), the person refuses: the closing words need the planner to have been told so.
pub const FLOW_A_REFUSED: Cassette = Cassette(include_str!(
    "../../../dev/accept/cassettes/flow-a-refused.jsonl"
));
/// Two searches in a Space that has no grants yet.
pub const FIRST_USE: Cassette =
    Cassette(include_str!("../../../dev/accept/cassettes/first-use.jsonl"));
/// Flow (c): the injected thread, the reader, the send. Every planner entry refuses to answer a
/// view that shows the body.
pub const FLOW_C: Cassette = Cassette(include_str!("../../../dev/accept/cassettes/flow-c.jsonl"));

/// The handle the planner is shown for the reader's answer (the body of the thread is handle 1);
/// the flow-c cassette names it in the send.
pub const SUMMARY_HANDLE: u64 = 2;

/// Panics with the daemons' logs beside the message.
pub fn fail(world: &World, why: &str) -> ! {
    panic!("{why}\n{}", world.logs());
}
