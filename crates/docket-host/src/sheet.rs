//! The confirm sheet. The host draws it in its own window and answers with a
//! `Option<SheetChoice>` (`None`: closed without an answer); [`SheetConfirmer`] turns that into
//! the router's `ConfirmAnswer` and mints the
//! receipt itself (the input proof is `SheetFallback`: the app's own pointer event, no
//! compositor attestation), so a sheet can say yes or no but a model can never forge a yes.

use docket_core::{
    ConfirmAnswer, ConfirmEnd, ConfirmId, ConfirmOffer, ConfirmRequest, Confirmer, GrantScope,
};
use docket_permissions_core::AlwaysOffer;
use docket_session::SheetChoice;
use docket_vocab::Clock;
use prov::{Confidentiality, ConfirmReceipt, InputProof};
use std::future::Future;

/// What an app implements to ask its person. Draw `request` (it names the action, the effect,
/// the count, the concrete change and why it asks) and answer when they have chosen. Dropping
/// the future withdraws the sheet.
pub trait ConfirmSheet: Send + Sync {
    /// Asks the person. `None` is a sheet closed without an answer.
    fn ask(&self, request: &ConfirmRequest) -> impl Future<Output = Option<SheetChoice>> + Send;
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

fn scope_of(choice: SheetChoice, offer: ConfirmOffer) -> GrantScope {
    match (choice, offer) {
        (SheetChoice::Always, ConfirmOffer::OnceOrAlways) => GrantScope::Always,
        _ => GrantScope::Once,
    }
}

impl<T: ConfirmSheet, K: Clock> Confirmer for SheetConfirmer<T, K> {
    async fn confirm(&self, request: ConfirmRequest) -> ConfirmAnswer {
        let Some(choice) = self.sheet.ask(&request).await else {
            return ConfirmAnswer::Ended(ConfirmEnd::Dismissed);
        };
        match choice {
            SheetChoice::Once | SheetChoice::Always => ConfirmAnswer::Allowed {
                written: None,
                scope: if choice == SheetChoice::Always
                    && matches!(request.always, AlwaysOffer::Offered(_))
                {
                    // A scoped standing grant the router itself offered; it re-derives the scope.
                    GrantScope::Always
                } else {
                    scope_of(choice, request.offer)
                },
                receipt: ConfirmReceipt {
                    id: request.id,
                    input: InputProof::SheetFallback,
                    at: self.clock.now(),
                    covers: Confidentiality::Secret,
                },
            },
            SheetChoice::Refused => ConfirmAnswer::Ended(ConfirmEnd::Refused),
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
                SheetChoice::Always,
                ConfirmOffer::OnceOrAlways,
                GrantScope::Always,
            ),
            (
                SheetChoice::Always,
                ConfirmOffer::OnceOnly,
                GrantScope::Once,
            ),
            (
                SheetChoice::Always,
                ConfirmOffer::OnceOrFromTerminal,
                GrantScope::Once,
            ),
            (
                SheetChoice::Once,
                ConfirmOffer::OnceOrAlways,
                GrantScope::Once,
            ),
        ];
        for (choice, offer, want) in table {
            assert_eq!(scope_of(choice, offer), want, "{choice:?} on {offer:?}");
        }
    }
}
