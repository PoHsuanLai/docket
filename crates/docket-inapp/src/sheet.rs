//! The in-app confirm sheet. The app draws it in its own window and answers with a
//! [`SheetAnswer`]; [`SheetConfirmer`] turns that into the router's `ConfirmAnswer` and mints the
//! receipt itself (the input proof is `SheetFallback`: the app's own pointer event, no
//! compositor attestation), so a sheet can say yes or no but a model can never forge a yes.

use docket_core::{
    ConfirmAnswer, ConfirmEnd, ConfirmId, ConfirmOffer, ConfirmRequest, Confirmer, GrantScope,
};
use docket_router::Clock;
use prov::{Confidentiality, ConfirmReceipt, InputProof};
use std::future::Future;

/// What the person chose on the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SheetAnswer {
    /// Yes, this once.
    Once,
    /// Yes, and from now on for this app, class and Space (only where the sheet offered it).
    Always,
    /// No.
    Refused,
    /// Closed without an answer.
    Dismissed,
}

/// What an app implements to ask its person. Draw `request` (it names the action, the effect,
/// the count, the concrete change and why it asks) and answer when they have chosen. Dropping
/// the future withdraws the sheet.
pub trait ConfirmSheet: Send + Sync {
    /// Asks the person.
    fn ask(&self, request: &ConfirmRequest) -> impl Future<Output = SheetAnswer> + Send;
    /// The router withdrew the request (a halt, a cancel): take the sheet down.
    fn withdraw(&self, id: &ConfirmId) -> impl Future<Output = ()> + Send {
        let _ = id;
        async {}
    }
}

/// A [`ConfirmSheet`] as the router's `Confirmer`.
#[derive(Debug)]
pub struct SheetConfirmer<T, K> {
    sheet: T,
    clock: K,
}

impl<T: ConfirmSheet, K: Clock> SheetConfirmer<T, K> {
    /// Asks through `sheet`, stamping receipts with `clock`.
    pub fn new(sheet: T, clock: K) -> Self {
        Self { sheet, clock }
    }

    /// The sheet, for the app to read what it showed.
    pub fn sheet(&self) -> &T {
        &self.sheet
    }

    /// The clock.
    pub fn clock(&self) -> &K {
        &self.clock
    }
}

fn scope_of(answer: SheetAnswer, offer: ConfirmOffer) -> GrantScope {
    match (answer, offer) {
        (SheetAnswer::Always, ConfirmOffer::OnceOrAlways) => GrantScope::Always,
        _ => GrantScope::Once,
    }
}

impl<T: ConfirmSheet, K: Clock> Confirmer for SheetConfirmer<T, K> {
    async fn confirm(&self, request: ConfirmRequest) -> ConfirmAnswer {
        let answer = self.sheet.ask(&request).await;
        match answer {
            SheetAnswer::Once | SheetAnswer::Always => ConfirmAnswer::Allowed {
                scope: scope_of(answer, request.offer),
                receipt: ConfirmReceipt {
                    id: request.id,
                    input: InputProof::SheetFallback,
                    at: self.clock.now(),
                    covers: Confidentiality::Secret,
                },
            },
            SheetAnswer::Refused => ConfirmAnswer::Ended(ConfirmEnd::Refused),
            SheetAnswer::Dismissed => ConfirmAnswer::Ended(ConfirmEnd::Dismissed),
        }
    }

    async fn cancel(&self, id: &ConfirmId) {
        self.sheet.withdraw(id).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_counts_only_where_the_sheet_offered_it() {
        let table = [
            (
                SheetAnswer::Always,
                ConfirmOffer::OnceOrAlways,
                GrantScope::Always,
            ),
            (
                SheetAnswer::Always,
                ConfirmOffer::OnceOnly,
                GrantScope::Once,
            ),
            (
                SheetAnswer::Always,
                ConfirmOffer::OnceOrFromTerminal,
                GrantScope::Once,
            ),
            (
                SheetAnswer::Once,
                ConfirmOffer::OnceOrAlways,
                GrantScope::Once,
            ),
        ];
        for (answer, offer, want) in table {
            assert_eq!(scope_of(answer, offer), want, "{answer:?} on {offer:?}");
        }
    }
}
