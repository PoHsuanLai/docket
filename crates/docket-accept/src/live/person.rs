//! The scripted person of a live-planner attack run: the one who is asked when the router puts a
//! sheet up. They want the task they asked for done, so they say yes to the sheets of the actions
//! that task needs and no to every other sheet, whatever it says. A sheet is recognised by the
//! app and the label the manifest gives the action, which is all a sheet names. Every sheet shown
//! is kept for the trace.

use docket_core::{ConfirmAnswer, ConfirmEnd, ConfirmId, ConfirmRequest, Confirmer};
use porter_core::AppName;
use porter_core::consent::GrantScope;
use prov::{Confidentiality, ConfirmReceipt, InputProof, UnixSeconds};
use std::collections::BTreeSet;
use std::future::Future;
use std::sync::{Mutex, PoisonError};

/// The person at the sheet.
#[derive(Debug)]
pub struct Person {
    yes: BTreeSet<(AppName, String)>,
    shown: Mutex<Vec<ConfirmRequest>>,
}

impl Person {
    /// A person who allows the sheets of these actions (app and label) once, and refuses the rest.
    pub fn saying_yes_to(yes: BTreeSet<(AppName, String)>) -> Self {
        Self {
            yes,
            shown: Mutex::new(Vec::new()),
        }
    }

    /// Whether the person allows a sheet for `app`'s action labelled `label`.
    pub fn says_yes(&self, app: &AppName, label: &str) -> bool {
        self.yes.iter().any(|(a, l)| a == app && l == label)
    }

    /// Every sheet shown so far, oldest first.
    pub fn requests(&self) -> Vec<ConfirmRequest> {
        self.shown
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn answer(&self, request: &ConfirmRequest) -> ConfirmAnswer {
        match self.says_yes(&request.app, request.action.as_str()) {
            true => ConfirmAnswer::Allowed {
                scope: GrantScope::Once,
                receipt: ConfirmReceipt {
                    id: request.id.clone(),
                    input: InputProof::SheetFallback,
                    at: UnixSeconds(1),
                    covers: Confidentiality::Secret,
                },
            },
            false => ConfirmAnswer::Ended(ConfirmEnd::Refused),
        }
    }
}

impl Confirmer for Person {
    fn confirm(&self, request: ConfirmRequest) -> impl Future<Output = ConfirmAnswer> + Send {
        let answer = self.answer(&request);
        self.shown
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(request);
        std::future::ready(answer)
    }

    fn cancel(&self, _id: &ConfirmId) -> impl Future<Output = ()> + Send {
        std::future::ready(())
    }
}
