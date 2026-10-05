//! Shared by the acceptance scenarios: where the daemon binaries are, planner steps that name
//! what an earlier step returned, and a failure that prints the daemons' logs.
#![allow(dead_code)]

use docket_accept::inferd::{Say, Step, text_of};
use docket_accept::world::{Binaries, World};
use porter_infer::ChatRequest;
use serde_json::{Value as Json, json};

pub fn binaries() -> Binaries {
    Binaries {
        intentd: env!("CARGO_BIN_EXE_accept-intentd").into(),
        companiond: env!("CARGO_BIN_EXE_accept-companiond").into(),
        readerd: env!("CARGO_BIN_EXE_accept-readerd").into(),
        memoryd: env!("CARGO_BIN_EXE_accept-memoryd").into(),
    }
}

pub fn tool(action: &str) -> String {
    format!("org.quire.Mail-{action}")
}

/// The number after the first `#` on the last line of the request that holds `marker`: the
/// handle the planner was shown for that step's value.
pub fn handle_after(request: &ChatRequest, marker: &str) -> u64 {
    let text = text_of(request);
    let line = text
        .lines()
        .rev()
        .find(|l| l.contains(marker))
        .unwrap_or_else(|| panic!("no line with {marker:?} in the planner's view:\n{text}"));
    let digits: String = line
        .split_once("value #")
        .unwrap_or_else(|| panic!("no handle on {line:?}"))
        .1
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits
        .parse()
        .unwrap_or_else(|_| panic!("no handle number on {line:?}"))
}

pub fn calls(name: &str, args: Json) -> Step {
    docket_accept::inferd::say(Say::Calls(vec![(name.to_owned(), args)]))
}

pub fn words(text: &str) -> Step {
    docket_accept::inferd::say(Say::Words(text.to_owned()))
}

pub fn contact(key: &str) -> Json {
    json!({"app": "org.quire.Mail", "kind": "mail.contact", "key": key})
}

pub fn thread(key: &str) -> Json {
    json!({"app": "org.quire.Mail", "kind": "mail.thread", "key": key})
}

/// Panics with the daemons' logs beside the message.
pub fn fail(world: &World, why: &str) -> ! {
    panic!("{why}\n{}", world.logs());
}
