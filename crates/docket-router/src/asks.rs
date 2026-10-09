//! Sheets that are open right now, so a second call for the same thing waits on the first
//! sheet instead of raising another.
//!
//! An agent may issue several calls at once. If they are the same action in the same Space for
//! the same caller, with the same reasons, taint and offer, the person is asked once. Only a
//! sheet that offered a standing grant is shared: one that offers "once" alone is that call's
//! own question and every call asks it for itself.
//!
//! What a waiter takes from the first sheet's answer:
//! - "allow always" (or from the terminal): the grant is now held, so the waiter proceeds as an
//!   allowed call without recording the grant again;
//! - "allow once": covers only the asking call, so the waiter asks again, on its own sheet,
//!   after the first has closed;
//! - any refusal, expiry or cancel: the waiter is refused with the same end.

use crate::state::RouterState;
use docket_core::{
    AlwaysOffer, AskReason, ConfirmAnswer, ConfirmOffer, ConfirmRequest, GrantScope, LabelText,
    TaintNote,
};
use futures_channel::oneshot::{Receiver, Sender, channel};
use porter_core::AppName;
use prov::{Actor, Effect, SessionId, SpaceId};
use std::sync::{Mutex, MutexGuard};

/// What makes two sheets the same question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AskKey {
    space: SpaceId,
    session: Option<SessionId>,
    actor: Actor,
    app: AppName,
    action: LabelText,
    effect: Effect,
    why: Vec<AskReason>,
    taint: TaintNote,
    offer: ConfirmOffer,
    always: AlwaysOffer,
}

/// Whether a sheet may be shared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Shared {
    /// It offered a standing grant, so one answer serves the calls that asked the same.
    Key(Box<AskKey>),
    /// It is this call's own question.
    Own,
}

impl AskKey {
    /// The sheet's key, or `Own` when no grant is on offer.
    pub(crate) fn of(request: &ConfirmRequest, session: Option<&SessionId>) -> Shared {
        let grant_on_offer = request.offer != ConfirmOffer::OnceOnly
            || matches!(request.always, AlwaysOffer::Offered(_));
        if !grant_on_offer {
            return Shared::Own;
        }
        Shared::Key(Box::new(AskKey {
            space: request.space.clone(),
            session: session.cloned(),
            actor: request.actor.clone(),
            app: request.app.clone(),
            action: request.action.clone(),
            effect: request.effect,
            why: request.why.clone(),
            taint: request.taint.clone(),
            offer: request.offer,
            always: request.always.clone(),
        }))
    }
}

/// What a waiter does after the first sheet closed.
#[derive(Debug)]
pub(crate) enum Settled {
    /// Take this answer.
    Take(ConfirmAnswer),
    /// The answer covered only the asking call: ask again.
    AskAgain,
}

type Waiters = Vec<Sender<ConfirmAnswer>>;

/// The sheets open now, with the calls waiting on each.
#[derive(Debug, Default)]
pub(crate) struct Asks {
    open: Vec<(AskKey, Waiters)>,
}

/// Where a call stands against the open sheets.
pub(crate) enum Seat<'a> {
    /// Nothing like it is open: this call raises the sheet.
    Asker(Opened<'a>),
    /// The same sheet is open: wait for it.
    Waiter(Receiver<ConfirmAnswer>),
    /// The sheet is this call's own.
    Alone,
}

impl Asks {
    fn join(&mut self, key: &AskKey) -> Option<Receiver<ConfirmAnswer>> {
        let (_, waiters) = self.open.iter_mut().find(|(k, _)| k == key)?;
        let (tx, rx) = channel();
        waiters.push(tx);
        Some(rx)
    }

    fn take(&mut self, key: &AskKey) -> Waiters {
        match self.open.iter().position(|(k, _)| k == key) {
            Some(at) => self.open.remove(at).1,
            None => Vec::new(),
        }
    }
}

fn locked(state: &Mutex<RouterState>) -> MutexGuard<'_, RouterState> {
    state.lock().unwrap_or_else(|p| p.into_inner())
}

/// Takes a seat for the sheet `shared`.
pub(crate) fn seat<'a>(state: &'a Mutex<RouterState>, shared: Shared) -> Seat<'a> {
    let Shared::Key(key) = shared else {
        return Seat::Alone;
    };
    let mut st = locked(state);
    match st.asks.join(&key) {
        Some(rx) => Seat::Waiter(rx),
        None => {
            st.asks.open.push(((*key).clone(), Vec::new()));
            Seat::Asker(Opened {
                state,
                key,
                answer: None,
            })
        }
    }
}

/// The sheet this call raised. When it goes away, answered or not, the table is cleared; a
/// call that is dropped mid-sheet frees its waiters to ask for themselves.
pub(crate) struct Opened<'a> {
    state: &'a Mutex<RouterState>,
    key: Box<AskKey>,
    answer: Option<ConfirmAnswer>,
}

impl Opened<'_> {
    /// Records the answer the waiters take.
    pub(crate) fn settle(&mut self, answer: &ConfirmAnswer) {
        self.answer = Some(answer.clone());
    }
}

impl Drop for Opened<'_> {
    fn drop(&mut self) {
        let waiters = locked(self.state).asks.take(&self.key);
        let shared = self.answer.take().and_then(for_waiter);
        for tx in waiters {
            if let Some(answer) = &shared {
                // A waiter that was dropped has nothing to hear.
                let _ = tx.send(answer.clone());
            }
        }
    }
}

/// What a waiter takes from an answer; nothing for "once", which was the asker's alone.
fn for_waiter(answer: ConfirmAnswer) -> Option<ConfirmAnswer> {
    match answer {
        ConfirmAnswer::Allowed {
            scope: GrantScope::Once,
            ..
        } => None,
        // The grant is recorded once, by the asker; the waiter runs on the same yes.
        ConfirmAnswer::Allowed { receipt, .. } | ConfirmAnswer::AllowedFromTerminal { receipt } => {
            Some(ConfirmAnswer::Allowed {
                scope: GrantScope::Once,
                receipt,
            })
        }
        ended @ ConfirmAnswer::Ended(_) => Some(ended),
    }
}

/// How a waiter's wait ended.
pub(crate) async fn settled(rx: Receiver<ConfirmAnswer>) -> Settled {
    match rx.await {
        Ok(answer) => Settled::Take(answer),
        Err(_) => Settled::AskAgain,
    }
}
