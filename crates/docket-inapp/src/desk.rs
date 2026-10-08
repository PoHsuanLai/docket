//! `EditorDesk`: the sheet of an app whose person sits in an editor. The router asks it like any
//! [`ConfirmSheet`] and the request waits on the desk; the session host hands the request to the
//! editor (`SheetDesk::request`) and puts the editor\'s choice back (`SheetDesk::answer`). The
//! choice names no scope: [`SheetConfirmer`](crate::SheetConfirmer) builds the answer and the
//! receipt from it, and the router honours an "always" only where its own sheet offered one.

use crate::{ConfirmSheet, SheetAnswer};
use docket_core::{ConfirmId, ConfirmRequest};
use docket_session::{DeskFault, SheetChoice, SheetDesk};
use futures_channel::oneshot;
use prov::SessionId;
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};

/// Whether the host has been handed a sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Handed {
    Not,
    Yes,
}

/// A sheet waiting for its answer.
#[derive(Debug)]
struct Waiting {
    request: ConfirmRequest,
    handed: Handed,
    answer: oneshot::Sender<SheetAnswer>,
}

#[derive(Debug, Default)]
struct Held {
    open: BTreeMap<ConfirmId, Waiting>,
    /// Hosts waiting for a sheet to arrive: each is pinged when one does, and looks again.
    watchers: Vec<oneshot::Sender<()>>,
}

impl Held {
    fn ping(&mut self) {
        for watcher in std::mem::take(&mut self.watchers) {
            // A watcher that stopped waiting needs nothing.
            let _ = watcher.send(());
        }
    }

    /// The oldest sheet of `session` not yet handed out, now marked handed.
    fn hand_out(&mut self, session: &SessionId) -> Option<ConfirmRequest> {
        let waiting = self.open.values_mut().find(|w| {
            w.handed == Handed::Not
                && w.request
                    .editor
                    .as_ref()
                    .is_some_and(|r| &r.session == session)
        })?;
        waiting.handed = Handed::Yes;
        Some(waiting.request.clone())
    }
}

/// The sheets put to the editor and not yet answered. Cloning shares the desk.
#[derive(Debug, Clone, Default)]
pub struct EditorDesk {
    held: Arc<Mutex<Held>>,
}

impl EditorDesk {
    /// A desk with nothing on it.
    pub fn new() -> Self {
        Self::default()
    }

    fn with<R>(&self, f: impl FnOnce(&mut Held) -> R) -> R {
        f(&mut self.held.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// How many sheets wait.
    pub fn waiting(&self) -> usize {
        self.with(|held| held.open.len())
    }
}

impl ConfirmSheet for EditorDesk {
    fn ask(&self, request: &ConfirmRequest) -> impl Future<Output = SheetAnswer> + Send {
        let (answer, wait) = oneshot::channel();
        self.with(|held| {
            held.open.insert(
                request.id.clone(),
                Waiting {
                    request: request.clone(),
                    handed: Handed::Not,
                    answer,
                },
            );
            held.ping();
        });
        // A desk dropped with the sheet open is a sheet closed.
        async move { wait.await.unwrap_or(SheetAnswer::Dismissed) }
    }

    fn withdraw(&self, id: &ConfirmId) -> impl Future<Output = ()> + Send {
        self.with(|held| held.open.remove(id));
        async {}
    }
}

impl SheetDesk for EditorDesk {
    fn request(&self, id: &ConfirmId) -> Option<ConfirmRequest> {
        self.with(|held| held.open.get(id).map(|w| w.request.clone()))
    }

    fn next_sheet(
        &self,
        session: &SessionId,
    ) -> impl Future<Output = Option<ConfirmRequest>> + Send {
        let desk = self.clone();
        let session = session.clone();
        async move {
            loop {
                let (ping, wait) = oneshot::channel();
                // Looked at and registered under one lock, so an arrival cannot slip between.
                let found = desk.with(|held| {
                    let found = held.hand_out(&session);
                    if found.is_none() {
                        held.watchers.push(ping);
                    }
                    found
                });
                if found.is_some() {
                    return found;
                }
                if wait.await.is_err() {
                    return None;
                }
            }
        }
    }

    fn answer(&self, id: &ConfirmId, choice: SheetChoice) -> Result<(), DeskFault> {
        let waiting = self
            .with(|held| held.open.remove(id))
            .ok_or(DeskFault::NoSuchSheet)?;
        let answer = match choice {
            SheetChoice::Once => SheetAnswer::Once,
            SheetChoice::Always => SheetAnswer::Always,
            SheetChoice::Refused => SheetAnswer::Refused,
        };
        // The router stopped waiting (the call was cancelled): nothing is left to tell.
        let _ = waiting.answer.send(answer);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docket_core::{
        Anchor, ConfirmDetail, ConfirmOffer, ConfirmParts, Gesture, LabelText, Seconds, TaintNote,
    };
    use futures_util::FutureExt;
    use porter_core::{AppName, Count};
    use prov::{Actor, Effect, SpaceId};

    fn request(id: &str) -> ConfirmRequest {
        ConfirmRequest::new(ConfirmParts {
            id: ConfirmId::parse(id).expect("id"),
            space: SpaceId::desktop(),
            actor: Actor::Unknown,
            app: AppName::parse("org.quire.Mail").expect("app"),
            action: LabelText::parse("Archive messages").expect("label"),
            effect: Effect::UndoableWrite,
            count: Count(1),
            detail: ConfirmDetail::Plain,
            lines: Vec::new(),
            why: Vec::new(),
            taint: TaintNote::Clean,
            offer: ConfirmOffer::OnceOnly,
            gesture: Gesture::Press,
            anchor: Anchor::Centre,
            expires: Seconds(120),
        })
    }

    #[test]
    fn a_sheet_waits_on_the_desk_until_the_editor_answers_it_once() {
        let desk = EditorDesk::new();
        let ask = request("c-1");
        let mut asking = Box::pin(desk.ask(&ask));
        assert_eq!((&mut asking).now_or_never(), None);
        assert_eq!(desk.request(&ask.id), Some(ask.clone()));
        assert_eq!(desk.answer(&ask.id, SheetChoice::Always), Ok(()));
        assert_eq!(asking.now_or_never(), Some(SheetAnswer::Always));
        assert_eq!(
            desk.answer(&ask.id, SheetChoice::Once),
            Err(DeskFault::NoSuchSheet)
        );
        assert_eq!(desk.waiting(), 0);
    }

    #[test]
    fn a_sheet_is_handed_to_its_sessions_host_once() {
        let desk = EditorDesk::new();
        let mine = SessionId::parse("s-1").expect("session");
        let other = SessionId::parse("s-2").expect("session");
        let mut waiting = Box::pin(desk.next_sheet(&mine));
        assert_eq!((&mut waiting).now_or_never(), None);
        let ask = request("c-3").for_editor(Some(docket_core::EditorRoute {
            client: prov::ClientName::parse("org.quire.Acp").expect("client"),
            session: mine.clone(),
        }));
        let _asking = desk.ask(&ask);
        assert_eq!(waiting.now_or_never(), Some(Some(ask.clone())));
        // Once: not again, and never to another session.
        assert_eq!(desk.next_sheet(&mine).now_or_never(), None);
        assert_eq!(desk.next_sheet(&other).now_or_never(), None);
        // Still on the desk, to be answered.
        assert_eq!(desk.request(&ask.id), Some(ask.clone()));
    }

    #[test]
    fn a_withdrawn_sheet_cannot_be_answered() {
        let desk = EditorDesk::new();
        let ask = request("c-2");
        let _asking = desk.ask(&ask);
        assert!(desk.request(&ask.id).is_some());
        desk.withdraw(&ask.id).now_or_never();
        assert_eq!(desk.request(&ask.id), None);
        assert_eq!(
            desk.answer(&ask.id, SheetChoice::Once),
            Err(DeskFault::NoSuchSheet)
        );
    }
}
