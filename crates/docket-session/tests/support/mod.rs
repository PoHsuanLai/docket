//! Fixtures: a canonical session log and the rows around it. Scripted, no clock, no network.
#![allow(dead_code)]

use docket_core::BreakerTrip;
use docket_core::{
    ActionMatch, ActionRef, CallId, Handle, HandleShape, LabelText, Ledger, StepEnd, StepLine,
    StepShown, TaskPolicy, TaskPolicyState, TurnId, TurnSource, TurnVia, UserTurn,
};
use docket_session::*;
use porter_core::{AppName, Count, DataClass};
use prov::{
    ActionName, AgentRef, Effect, EntityKind, Label, SessionId, Source, SpaceId, TaskId,
    UnixSeconds,
};
use std::collections::BTreeSet;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

/// Polls a future that never waits.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(out) = future.as_mut().poll(&mut cx) {
            return out;
        }
    }
}

pub fn session(n: &str) -> SessionId {
    SessionId::parse(n).expect("session")
}
pub fn task(n: &str) -> TaskId {
    TaskId::parse(n).expect("task")
}
pub fn space() -> SpaceId {
    SpaceId::parse("work").expect("space")
}
pub fn mail() -> AppName {
    AppName::parse("org.quire.Mail").expect("app")
}
pub fn action(name: &str) -> ActionRef {
    ActionRef {
        app: mail(),
        name: ActionName::parse(name).expect("action"),
    }
}

pub fn opening() -> Opening {
    Opening {
        task: task("t-1"),
        space: space(),
        opener: Some(mail()),
        agent: Some(AgentRef::Companion),
        backend: BackendKind::Native,
        parent: None,
        forked_from: None,
        cwd: None,
    }
}

pub fn turn(id: u64, text: &str) -> UserTurn {
    UserTurn {
        id: TurnId(id),
        text: text.to_owned(),
        at: UnixSeconds(id as i64),
        from: TurnSource::Launcher,
        via: TurnVia::Typed,
    }
}

pub fn policy() -> TaskPolicy {
    TaskPolicy {
        task: task("t-1"),
        space: space(),
        from: vec![TurnId(1)],
        actions: BTreeSet::from([ActionMatch::AppUpTo(mail(), Effect::UndoableWrite)]),
        kinds: BTreeSet::from([EntityKind::parse("mail.thread").expect("kind")]),
        ceiling: Effect::UndoableWrite,
        max_count: Count(10),
        recipients: vec![],
        destinations: vec![],
        paths: vec![],
        expires: UnixSeconds(i64::MAX),
        rationale: LabelText::parse("archive the newsletters").expect("words"),
        state: TaskPolicyState::Active,
    }
}

pub fn call(n: u64, name: &str) -> SessionEntry {
    SessionEntry::Call(CallOpen {
        call: CallId(n),
        action: action(name),
        effect: Effect::UndoableWrite,
    })
}

pub fn step(n: u64, name: &str) -> SessionEntry {
    SessionEntry::Step(StepLine {
        call: CallId(n),
        action: action(name),
        effect: Effect::UndoableWrite,
        end: StepEnd::Done {
            said: None,
            value: None,
            undo: None,
        },
        shown: StepShown::Full,
        with: Vec::new(),
    })
}

pub fn untrusted_handle(n: u64) -> SessionEntry {
    SessionEntry::Handle(HandleLabel {
        handle: Handle(n),
        label: Label::untrusted(Source::Mail, DataClass::Mail, space()),
        shape: HandleShape::Text,
        from: Source::Mail,
    })
}

pub fn trusted_handle(n: u64) -> SessionEntry {
    SessionEntry::Handle(HandleLabel {
        handle: Handle(n),
        label: Label::trusted_user(),
        shape: HandleShape::File,
        from: Source::User,
    })
}

pub fn taint() -> SessionEntry {
    SessionEntry::Taint(TaintNote {
        cause: TaintCause::UntrustedReveal,
        at_call: Some(CallId(1)),
    })
}

pub fn ledger(calls: u32) -> Ledger {
    let mut l = Ledger::new(UnixSeconds(0));
    l.calls = Count(calls);
    l
}

/// Rows numbered from 0 as a writer would have written them.
pub fn rows(entries: &[SessionEntry]) -> Vec<Logged> {
    entries
        .iter()
        .enumerate()
        .map(|(i, e)| Logged {
            seq: Seq(i as u64),
            read: Read::Entry(Box::new(e.clone())),
        })
        .collect()
}

/// A whole session: the write-ahead order (taint before the handle it guards, a call before its
/// step), a trip and a turn, a checkpoint, a close.
pub fn canonical() -> Vec<SessionEntry> {
    vec![
        SessionEntry::Opened(opening()),
        SessionEntry::Turn(turn(1, "archive the newsletters")),
        SessionEntry::Policy(policy()),
        call(1, "mail.thread.search"),
        taint(),
        untrusted_handle(1),
        step(1, "mail.thread.search"),
        call(2, "mail.thread.archive"),
        SessionEntry::Breaker(BreakerNote::Tripped(BreakerTrip::Consecutive)),
        SessionEntry::Turn(turn(2, "go on")),
        SessionEntry::Budget(ledger(2)),
        trusted_handle(2),
        step(2, "mail.thread.archive"),
        SessionEntry::Closed(EndCause::Closed),
    ]
}
