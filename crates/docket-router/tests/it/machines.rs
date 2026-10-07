//! The session machine, the index machine and the undo journal.

use docket_core::*;
use docket_router::*;
use prov::{Actor, AgentRole, RunId, SessionId, UnixSeconds};

#[test]
fn session_machine_table() {
    use SessionEffect as E;
    use SessionEvent as V;
    use SessionState as S;
    let trip = BreakerTrip::Consecutive;
    let cases: Vec<(
        &str,
        SessionState,
        SessionEvent,
        SessionState,
        Vec<SessionEffect>,
    )> = vec![
        (
            "an untrusted reveal taints",
            S::Open(Taint::Clean),
            V::UntrustedReveal,
            S::Open(Taint::Tainted),
            vec![],
        ),
        (
            "taint never goes back",
            S::Open(Taint::Tainted),
            V::UserTurn,
            S::Open(Taint::Tainted),
            vec![],
        ),
        (
            "a trip pauses and signals",
            S::Open(Taint::Tainted),
            V::BreakerTrip(trip),
            S::Paused {
                taint: Taint::Tainted,
                trip,
            },
            vec![E::EmitBreakerTripped(trip)],
        ),
        (
            "a user turn resumes and resets",
            S::Paused {
                taint: Taint::Clean,
                trip,
            },
            V::UserTurn,
            S::Open(Taint::Clean),
            vec![E::ResetBreaker],
        ),
        (
            "a perform while paused is refused",
            S::Paused {
                taint: Taint::Clean,
                trip,
            },
            V::Perform,
            S::Paused {
                taint: Taint::Clean,
                trip,
            },
            vec![E::Refuse(CallRefusal::Paused(trip))],
        ),
        (
            "a perform while open changes nothing",
            S::Open(Taint::Clean),
            V::Perform,
            S::Open(Taint::Clean),
            vec![],
        ),
        (
            "a second trip while paused keeps the first",
            S::Paused {
                taint: Taint::Clean,
                trip,
            },
            V::BreakerTrip(BreakerTrip::Probing),
            S::Paused {
                taint: Taint::Clean,
                trip,
            },
            vec![],
        ),
        (
            "a reveal while paused still taints",
            S::Paused {
                taint: Taint::Clean,
                trip,
            },
            V::UntrustedReveal,
            S::Paused {
                taint: Taint::Tainted,
                trip,
            },
            vec![],
        ),
        (
            "close closes",
            S::Open(Taint::Clean),
            V::Close,
            S::Closed(CloseCause::Closed),
            vec![],
        ),
        (
            "a halted Space closes a paused session",
            S::Paused {
                taint: Taint::Clean,
                trip,
            },
            V::SpaceHalted,
            S::Closed(CloseCause::SpaceHalted),
            vec![],
        ),
        (
            "the wall closes",
            S::Open(Taint::Clean),
            V::WallExhausted,
            S::Closed(CloseCause::WallExhausted),
            vec![],
        ),
        (
            "closed stays closed",
            S::Closed(CloseCause::Closed),
            V::UserTurn,
            S::Closed(CloseCause::Closed),
            vec![],
        ),
    ];
    for (name, from, event, to, effects) in cases {
        assert_eq!(session_step(from, event), (to, effects), "case: {name}");
    }
}

#[test]
fn index_machine_table() {
    use IndexAction as A;
    use IndexEvent as V;
    use IndexState as S;
    let cases: Vec<(&str, IndexState, IndexEvent, IndexState, IndexAction)> = vec![
        (
            "reset starts an epoch",
            S::Unknown,
            V::Reset(4),
            S::Syncing(4),
            A::None,
        ),
        (
            "a push in the epoch completes it",
            S::Syncing(4),
            V::Push(4),
            S::Synced(4),
            A::Accept,
        ),
        (
            "more pushes in the epoch are taken",
            S::Synced(4),
            V::Push(4),
            S::Synced(4),
            A::Accept,
        ),
        (
            "a push in another epoch is refused",
            S::Synced(4),
            V::Push(5),
            S::Synced(4),
            A::RefuseAskReset,
        ),
        (
            "a push before any reset is refused",
            S::Unknown,
            V::Push(1),
            S::Unknown,
            A::RefuseAskReset,
        ),
        (
            "a push in the wrong epoch while syncing is refused",
            S::Syncing(4),
            V::Push(9),
            S::Syncing(4),
            A::RefuseAskReset,
        ),
        ("stale forgets", S::Synced(4), V::Stale, S::Unknown, A::None),
        (
            "a restart forgets",
            S::Syncing(4),
            V::AppRestarted,
            S::Unknown,
            A::None,
        ),
        (
            "a reset replaces a synced epoch",
            S::Synced(4),
            V::Reset(5),
            S::Syncing(5),
            A::None,
        ),
    ];
    for (name, from, event, to, action) in cases {
        assert_eq!(index_step(from, event), (to, action), "case: {name}");
    }
}

#[test]
fn undo_all_replays_newest_first_and_only_what_is_available() {
    let mut journal = UndoJournal::new();
    let actor = Actor::Companion {
        session: SessionId::parse("s-1").expect("s"),
        role: AgentRole::Cua {
            run: RunId::parse("r-1").expect("r"),
        },
    };
    let act = ActionRef {
        app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
        name: prov::ActionName::parse("mail.thread.archive").expect("a"),
    };
    let run = |n: &str| Some(RunId::parse(n).expect("run"));
    let mut ids = Vec::new();
    for (t, r) in [(10, "r-1"), (20, "r-2"), (30, "r-1")] {
        ids.push(journal.record(
            UnixSeconds(t),
            actor.clone(),
            act.clone(),
            LabelText::parse("Archived").expect("l"),
            UndoToken::parse(&format!("tok-{t}")).expect("t"),
            run(r),
            None,
        ));
    }
    assert_eq!(ids, [UndoId(1), UndoId(2), UndoId(3)]);
    let r1 = UndoScope::Run(RunId::parse("r-1").expect("run"));
    assert_eq!(journal.plan_undo(&r1, &[]), [UndoId(3), UndoId(1)]);
    assert!(journal.mark(UndoId(3), UndoState::Undone { by: actor.clone() }));
    assert_eq!(journal.plan_undo(&r1, &[]), [UndoId(1)]);
    assert_eq!(
        journal.plan_undo(&UndoScope::Entry(UndoId(2)), &[]),
        [UndoId(2)]
    );
    assert!(!journal.mark(UndoId(99), UndoState::Expired));
}

#[test]
fn old_entries_expire() {
    let mut journal = UndoJournal::new();
    let actor = Actor::User {
        via: porter_core::AppName::parse("org.quire.Shell").expect("app"),
    };
    let act = ActionRef {
        app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
        name: prov::ActionName::parse("mail.thread.archive").expect("a"),
    };
    let id = journal.record(
        UnixSeconds(0),
        actor,
        act,
        LabelText::parse("Archived").expect("l"),
        UndoToken::parse("t").expect("t"),
        None,
        None,
    );
    journal.expire(UnixSeconds(100), Seconds(200));
    assert_eq!(
        journal.get(id).map(|e| e.state.clone()),
        Some(UndoState::Available)
    );
    journal.expire(UnixSeconds(201), Seconds(200));
    assert_eq!(
        journal.get(id).map(|e| e.state.clone()),
        Some(UndoState::Expired)
    );
}
