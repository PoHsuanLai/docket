//! The rules every backend keeps, run against the scripted `FakeBackend`. The native backend runs
//! the same functions in `docket-tasks`.

use crate::support::*;
use docket_core::{Reveal, UserTurn};
use docket_session::contract::{self, Harness};
use docket_session::fake::FakeBackend;
use docket_session::*;
use prov::Effect;

/// A fake whose every turn calls `archive` and then says "Archived.".
struct Fake;

fn script() -> Vec<Vec<BackendEvent>> {
    let call = || {
        [
            BackendEvent::Call(CallEvent::Started(CallOpen {
                call: docket_core::CallId(0),
                action: action("mail.thread.archive"),
                effect: Effect::UndoableWrite,
            })),
            BackendEvent::Call(CallEvent::Ended(step_line())),
            BackendEvent::Words(Reveal::Plain("Archived.".into())),
        ]
    };
    vec![call().to_vec(), call().to_vec()]
}

fn step_line() -> docket_core::StepLine {
    match step(0, "mail.thread.archive") {
        SessionEntry::Step(line) => line,
        other => panic!("{other:?}"),
    }
}

impl Harness for Fake {
    type Backend = FakeBackend;

    async fn backend(&mut self) -> FakeBackend {
        let mut backend = FakeBackend::new(script());
        backend
            .start(StartSession {
                session: session("s-1"),
                opening: opening(),
            })
            .await
            .expect("start");
        backend
    }

    fn turn(&self, n: u64) -> UserTurn {
        turn(n, "archive the digest")
    }

    fn ran(&self, backend: &FakeBackend) -> usize {
        backend.ran
    }

    async fn settle(&mut self) {}

    fn stalls(&self) -> usize {
        0
    }
}

#[test]
fn the_fake_backend_runs_a_turn_by_the_contract() {
    block_on(contract::runs_a_turn(&mut Fake));
}

#[test]
fn the_fake_backend_stops_a_call_before_it_runs() {
    block_on(contract::a_stop_before_the_next_pull_stops_the_call(
        &mut Fake,
    ));
}

#[test]
fn the_fake_backend_makes_a_call_only_on_the_pull_after_its_announcement() {
    block_on(contract::a_call_waits_for_the_pull_after_its_announcement(
        &mut Fake,
    ));
}

#[test]
fn the_fake_backend_loses_nothing_to_a_dropped_pull() {
    block_on(contract::a_dropped_pull_loses_and_repeats_nothing(
        &mut Fake,
    ));
}
