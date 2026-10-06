//! `quire-do ask` as the real binary against the real daemons on a private bus: a terminal's
//! child (a `vte-spawn-*` scope of the fake proc root) opens a conversation with companiond, its
//! turn is recorded by intentd, the replayed model searches and answers, and the answer is
//! printed. A confirmation is shown on sill's sheet and never answered by `quire-do`.

mod support;

use docket_accept::world::{Cassette, Consent, World};
use support::*;

/// The scripted search, then the closing words; a read, so nothing asks.
const TERMINAL_ASK: Cassette = Cassette(include_str!(
    "../../../dev/accept/cassettes/terminal-ask.jsonl"
));

fn said(out: &std::process::Output) -> String {
    format!(
        "exit {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quire_do_ask_prints_the_companions_answer() {
    let world = World::start(&binaries(), Consent::Standing, TERMINAL_ASK).await;
    let out = world
        .quire_do(
            &binaries().quire_do,
            &[
                "ask", "--space", "work", "find", "the", "Lisbon", "receipts",
            ],
        )
        .await;
    if out.status.code() != Some(0) {
        fail(&world, &said(&out));
    }
    // Standard output is a pipe here, so the answer is JSON: the closing words are its text.
    let answer: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(answer["answer"]["phase"]["kind"], "done", "{}", said(&out));
    assert_eq!(
        answer["answer"]["body"]["v"]["lines"][0]["v"],
        "I found two threads about Lisbon.",
        "{}",
        said(&out)
    );
    assert!(world.sheet.shown().is_empty(), "a read asks nobody");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quire_do_ask_leaves_a_confirmation_to_the_shell() {
    let mut world = World::start(&binaries(), Consent::Standing, FLOW_A).await;
    let out = world
        .quire_do(
            &binaries().quire_do,
            &[
                "ask",
                "--space",
                "work",
                "forward",
                "the",
                "Lisbon",
                "receipts",
                "to",
                "accounting",
            ],
        )
        .await;
    if out.status.code() != Some(8) {
        fail(&world, &said(&out));
    }
    let answer: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(
        answer["answer"]["phase"]["kind"],
        "needs_you",
        "{}",
        said(&out)
    );
    assert_eq!(
        answer["answer"]["phase"]["v"]["kind"],
        "confirm",
        "{}",
        said(&out)
    );
    // The sheet is sill's: it arrives there (the event is waited for, not slept on), asks Forward,
    // and nobody answers it.
    let sheet = tokio::time::timeout(docket_accept::world::GIVE_UP, world.confirms.recv())
        .await
        .expect("the sheet came")
        .expect("a sheet");
    assert_eq!(sheet.action.as_str(), "Forward", "{sheet:#?}");
    assert!(
        world.mail.messages().is_empty(),
        "nothing was sent: quire-do answers nothing"
    );
}
