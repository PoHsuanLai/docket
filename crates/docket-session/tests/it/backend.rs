use crate::support::*;
use docket_core::Reveal;
use docket_session::fake::FakeBackend;
use docket_session::*;

#[test]
fn the_fake_backend_answers_each_turn_from_its_script_and_ends_it() {
    let mut backend = FakeBackend::new(vec![
        vec![BackendEvent::Words(Reveal::Plain("done".into()))],
        vec![BackendEvent::TurnEnd(TurnEnd::Refused)],
    ]);
    assert_eq!(
        block_on(backend.turn(turn(1, "x"))),
        Err(BackendFault::NotRunning)
    );
    block_on(backend.start(StartSession {
        session: session("s-1"),
        opening: opening(),
    }))
    .expect("start");
    assert_eq!(backend.kind(), BackendKind::Fake);

    block_on(backend.turn(turn(1, "hello"))).expect("turn");
    assert_eq!(
        block_on(backend.turn(turn(2, "again"))),
        Err(BackendFault::Busy)
    );
    let mut events = Vec::new();
    while let Some(e) = block_on(backend.next_event()) {
        events.push(e);
    }
    assert_eq!(
        events,
        vec![
            BackendEvent::Words(Reveal::Plain("done".into())),
            BackendEvent::TurnEnd(TurnEnd::Done),
        ]
    );

    block_on(backend.turn(turn(2, "again"))).expect("turn");
    assert_eq!(
        block_on(backend.next_event()),
        Some(BackendEvent::TurnEnd(TurnEnd::Refused))
    );
    assert_eq!(backend.turns.len(), 2);
}

#[test]
fn cancel_ends_the_turn_cancelled_and_close_stops_it() {
    let mut backend = FakeBackend::new(vec![vec![BackendEvent::Thought("hm".into())]]);
    block_on(backend.start(StartSession {
        session: session("s-1"),
        opening: opening(),
    }))
    .expect("start");
    block_on(backend.turn(turn(1, "go"))).expect("turn");
    block_on(backend.cancel());
    assert_eq!(
        block_on(backend.next_event()),
        Some(BackendEvent::TurnEnd(TurnEnd::Cancelled))
    );
    block_on(backend.close());
    assert_eq!(
        block_on(backend.turn(turn(2, "x"))),
        Err(BackendFault::NotRunning)
    );
}

#[test]
fn a_resume_restores() {
    let plan = resume_plan(&rows(&canonical())).expect("plan");
    let mut backend = FakeBackend::new(vec![]);
    assert_eq!(block_on(backend.resume(&plan)), Ok(Resumed::Restored));
}
