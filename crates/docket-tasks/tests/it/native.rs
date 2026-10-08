//! The native host over the fakes: a session opened, a turn pulled event by event, an editor's
//! stop before a call, and a restart in the middle.

use crate::support::infer::{call, words};
use crate::support::*;
use docket_core::{TurnId, TurnSource, TurnVia, UserTurn};
use docket_session::fake::MemoryLog;
use docket_session::{
    BackendEvent, BackendKind, CallEvent, DeskFault, EndCause, Opening, Read, Seq, SessionEntry,
    SessionHost, SheetChoice, SheetDesk, Standing, TurnEnd, Workspace, read_all,
};
use prov::{AgentRef, UnixSeconds};
use serde_json::json;
use std::sync::Arc;

fn opening() -> Opening {
    Opening {
        task: prov::TaskId::parse("acp-t-1").expect("task"),
        space: work(),
        opener: Some(app("org.quire.Acp")),
        agent: Some(AgentRef::Companion),
        backend: BackendKind::Native,
        parent: None,
        forked_from: None,
        cwd: Some(Workspace::parse("/work/a").expect("cwd")),
        started_from: None,
        label: None,
    }
}

fn said(text: &str) -> UserTurn {
    UserTurn {
        id: TurnId(1),
        text: text.into(),
        at: UnixSeconds(1_000),
        from: TurnSource::Editor(app("org.quire.Acp")),
        via: TurnVia::Typed,
    }
}

fn archive() -> crate::support::infer::Say {
    call(ARCHIVE, json!({ "target": [thread("t2")] }))
}

async fn pull(world: &mut World, s: &prov::SessionId) -> BackendEvent {
    world
        .host
        .next_event(s)
        .await
        .expect("host")
        .expect("an event")
}

async fn entries(log: &Arc<MemoryLog>, s: &prov::SessionId) -> Vec<SessionEntry> {
    read_all(log, s)
        .await
        .expect("rows")
        .into_iter()
        .filter_map(|r| match r.read {
            Read::Entry(e) => Some(*e),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_turn_is_announced_made_and_told_and_the_router_records_it() {
    let mut w = World::new(vec![archive(), words("Archived the digest.")]);
    let s = w.host.open(opening()).await.expect("open");
    w.host
        .turn(&s, said("archive the digest"))
        .await
        .expect("turn");
    assert!(matches!(
        pull(&mut w, &s).await,
        BackendEvent::Call(CallEvent::Started(_))
    ));
    assert_eq!(w.performed(), 0, "announced, not made");
    assert!(matches!(
        pull(&mut w, &s).await,
        BackendEvent::Call(CallEvent::Ended(_))
    ));
    assert_eq!(w.performed(), 1);
    assert!(matches!(pull(&mut w, &s).await, BackendEvent::Words(_)));
    assert_eq!(pull(&mut w, &s).await, BackendEvent::TurnEnd(TurnEnd::Done));
    assert!(w.router.seams.link.mail.is_archived("t2"));

    let log = entries(&w.log, &s).await;
    let SessionEntry::Opened(o) = &log[0] else {
        panic!("{log:?}")
    };
    assert_eq!(o.cwd, Some(Workspace::parse("/work/a").expect("cwd")));
    assert!(log.iter().any(
        |e| matches!(e, SessionEntry::Turn(t) if t.from == TurnSource::Editor(app("org.quire.Acp")))
    ));
    assert!(log.iter().any(|e| matches!(e, SessionEntry::Call(_))));
    assert!(log.iter().any(|e| matches!(e, SessionEntry::Step(_))));
}

#[tokio::test]
async fn an_editors_reject_after_the_announcement_means_the_app_never_runs() {
    let mut w = World::new(vec![archive(), words("unused")]);
    let s = w.host.open(opening()).await.expect("open");
    w.host
        .turn(&s, said("archive the digest"))
        .await
        .expect("turn");
    assert!(matches!(
        pull(&mut w, &s).await,
        BackendEvent::Call(CallEvent::Started(_))
    ));
    // The editor's person says no: the server cancels the host and reads on.
    w.host.cancel(&s).await.expect("cancel");
    let mut rest = Vec::new();
    while let Some(e) = w.host.next_event(&s).await.expect("host") {
        rest.push(e);
    }
    assert_eq!(
        rest.last(),
        Some(&BackendEvent::TurnEnd(TurnEnd::Cancelled))
    );
    assert_eq!(w.performed(), 0, "the fake app's call log is empty");
    assert!(!w.router.seams.link.mail.is_archived("t2"));
    let log = entries(&w.log, &s).await;
    assert!(!log.iter().any(|e| matches!(e, SessionEntry::Call(_))));
}

#[tokio::test]
async fn a_restart_in_the_middle_brings_the_session_back_and_it_goes_on() {
    let mut w = World::new(vec![archive(), words("Archived the digest.")]);
    let s = w.host.open(opening()).await.expect("open");
    w.host
        .turn(&s, said("archive the digest"))
        .await
        .expect("turn");
    while let Some(e) = w.host.next_event(&s).await.expect("host") {
        if matches!(e, BackendEvent::TurnEnd(_)) {
            break;
        }
    }
    let before = entries(&w.log, &s).await.len();

    w.restart(vec![words("Still here.")]);
    let plan = w.host.resume(&s).await.expect("resume");
    assert_eq!(plan.standing, Standing::Open);
    assert_eq!(plan.turns.len(), 1);
    assert_eq!(
        plan.history.len(),
        1,
        "the call that ended is in the history"
    );
    w.host.turn(&s, said("anything else?")).await.expect("turn");
    assert!(matches!(pull(&mut w, &s).await, BackendEvent::Words(_)));
    assert_eq!(pull(&mut w, &s).await, BackendEvent::TurnEnd(TurnEnd::Done));
    // The planner was shown what happened before the restart.
    let view = w.infer.user_text(0);
    assert!(view.contains("archive the digest"), "{view}");
    assert!(entries(&w.log, &s).await.len() > before);
    assert_eq!(w.performed(), 0, "nothing was run again");
}

#[tokio::test]
async fn a_closed_session_takes_no_turn_and_a_fork_goes_on_from_a_point() {
    let mut w = World::new(vec![words("One."), words("Two.")]);
    let s = w.host.open(opening()).await.expect("open");
    w.host.turn(&s, said("first")).await.expect("turn");
    while let Some(e) = w.host.next_event(&s).await.expect("host") {
        if matches!(e, BackendEvent::TurnEnd(_)) {
            break;
        }
    }
    let at = Seq(entries(&w.log, &s).await.len() as u64 - 1);
    let child = w.host.fork(&s, at).await.expect("fork");
    assert_ne!(child, s);
    let exported = w.host.export(&child).await.expect("export");
    assert!(matches!(&exported.entries[0], SessionEntry::Opened(o) if o.forked_from.is_some()));

    w.host.close(&s, EndCause::Closed).await.expect("close");
    assert!(w.host.turn(&s, said("more")).await.is_err());

    w.host.resume(&child).await.expect("resume the fork");
    w.host.turn(&child, said("second")).await.expect("turn");
    assert!(matches!(pull(&mut w, &child).await, BackendEvent::Words(_)));
    let view = w.infer.user_text(1);
    assert!(
        view.contains("first"),
        "the fork kept the person's words: {view}"
    );
}

/// A desk with one canned sheet, handed out once, and a record of the choices put to it.
#[derive(Default, Clone)]
struct Canned(
    Arc<std::sync::Mutex<Vec<SheetChoice>>>,
    Arc<std::sync::atomic::AtomicUsize>,
);

impl SheetDesk for Canned {
    fn next_sheet(
        &self,
        _session: &prov::SessionId,
    ) -> impl std::future::Future<Output = Option<docket_core::ConfirmRequest>> + Send {
        let once = self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0;
        let sheet = once
            .then(|| self.request(&docket_core::ConfirmId::parse("c-1").expect("id")))
            .flatten();
        async move {
            match sheet {
                Some(sheet) => Some(sheet),
                None => std::future::pending().await,
            }
        }
    }

    fn request(&self, id: &docket_core::ConfirmId) -> Option<docket_core::ConfirmRequest> {
        use docket_core::*;
        Some(
            ConfirmRequest::new(ConfirmParts {
                id: id.clone(),
                space: work(),
                actor: prov::Actor::Unknown,
                app: app("org.quire.Mail"),
                action: LabelText::parse("Archive messages").expect("label"),
                effect: prov::Effect::UndoableWrite,
                count: porter_core::Count(1),
                detail: ConfirmDetail::Plain,
                lines: Vec::new(),
                why: Vec::new(),
                taint: TaintNote::Clean,
                offer: ConfirmOffer::OnceOnly,
                gesture: Gesture::Press,
                anchor: Anchor::Centre,
                expires: Seconds(120),
            })
            .offering(AlwaysOffer::Withheld(Withheld::CallerCannotHold)),
        )
    }

    fn answer(&self, _id: &docket_core::ConfirmId, choice: SheetChoice) -> Result<(), DeskFault> {
        self.0.lock().expect("lock").push(choice);
        Ok(())
    }
}

#[tokio::test]
async fn a_sheet_the_router_asks_reaches_the_edge_as_a_sheet_and_its_answer_goes_back() {
    let w = World::asking(vec![archive(), words("done")]);
    let desk = Canned::default();
    let mut host = w.host.with_desk(desk.clone());
    let s = host.open(opening()).await.expect("open");
    host.turn(&s, said("archive the digest"))
        .await
        .expect("turn");
    let mut seen = Vec::new();
    while let Some(e) = host.next_event(&s).await.expect("host") {
        if let BackendEvent::Sheet(request) = &e {
            host.answer_sheet(&s, &request.id, SheetChoice::Refused)
                .await
                .expect("answered");
        }
        let over = matches!(e, BackendEvent::TurnEnd(_));
        seen.push(e);
        if over {
            break;
        }
    }
    assert!(
        seen.iter().any(|e| matches!(e, BackendEvent::Sheet(_))),
        "{seen:?}"
    );
    assert_eq!(*desk.0.lock().expect("lock"), [SheetChoice::Refused]);
}
