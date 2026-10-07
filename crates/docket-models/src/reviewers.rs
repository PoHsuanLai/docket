//! The reviewer cascade over any porter-client Transport: the placeholder set used when nothing
//! is configured, and the cascade built from a set and a way to make one transport per stage.

use crate::model::TransportModel;
use action_review::{InferReviewer, ModelChoice, ModelFamily, ReviewerSet};
use docket_core::ReviewTimeouts;
use porter_client::Transport;
use porter_core::{AccountId, Billing, Locality, ModelId};
use porter_infer::ModelCard;

fn choice(model: &str, family: &str) -> Option<ModelChoice> {
    Some(ModelChoice {
        account: AccountId::parse("local").ok()?,
        model: ModelId::parse(model).ok()?,
        family: ModelFamily(family.to_owned()),
    })
}

/// The set used when nothing names one: three placeholder local models of three families. The
/// daemon answers a model it does not have with an error, which asks the person, so the
/// placeholder set can never allow anything; it only keeps the cascade built.
pub fn placeholder_set() -> Option<ReviewerSet> {
    Some(ReviewerSet {
        quick: choice("quire-quick", "quick")?,
        deliberate: choice("quire-deliberate", "deliberate")?,
        second: choice("quire-second", "second")?,
    })
}

/// The card of a chosen model: on this computer, free.
pub fn card_of(choice: &ModelChoice) -> ModelCard {
    ModelCard {
        account: choice.account.clone(),
        model: choice.model.clone(),
        locality: Locality::OnDevice,
        billing: Billing::Free,
        capabilities: vec![],
    }
}

/// The cascade for `set` (the placeholder one when none is given), with `timeouts` per stage;
/// `transport` makes the link of each stage.
pub fn reviewer_over<T: Transport>(
    set: Option<&ReviewerSet>,
    timeouts: ReviewTimeouts,
    mut transport: impl FnMut() -> T,
) -> Option<InferReviewer<TransportModel<T>>> {
    let placeholder = placeholder_set();
    let set = set.or(placeholder.as_ref())?;
    let mut model = |choice: &ModelChoice| TransportModel::new(transport(), card_of(choice));
    Some(InferReviewer {
        quick: model(&set.quick),
        deliberate: model(&set.deliberate),
        second: model(&set.second),
        timeouts,
    })
}
