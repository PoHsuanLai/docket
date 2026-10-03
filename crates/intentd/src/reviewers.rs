//! The reviewer cascade over inferd, from the configured set or the placeholder one.

use action_review::{InferReviewer, ModelChoice, ModelFamily, ReviewerSet};
use docket_core::ReviewTimeouts;
use docket_dbus::BusConnection;
use porter_client::AnyTransport;
use porter_core::{AccountId, Billing, Locality, ModelId};
use porter_infer::ModelCard;

use crate::infer::InferdModel;

fn choice(model: &str, family: &str) -> Option<ModelChoice> {
    Some(ModelChoice {
        account: AccountId::parse("local").ok()?,
        model: ModelId::parse(model).ok()?,
        family: ModelFamily(family.to_owned()),
    })
}

/// The set used when `intentd.toml` names none: three placeholder local models of three
/// families. inferd answers a model it does not have with an error, which asks the person, so
/// the placeholder set can never allow anything; it only keeps the cascade built.
pub fn placeholder_set() -> Option<ReviewerSet> {
    Some(ReviewerSet {
        quick: choice("quire-quick", "quick")?,
        deliberate: choice("quire-deliberate", "deliberate")?,
        second: choice("quire-second", "second")?,
    })
}

fn model(connection: &BusConnection, choice: &ModelChoice) -> InferdModel<AnyTransport> {
    InferdModel::on_bus(
        connection,
        ModelCard {
            account: choice.account.clone(),
            model: choice.model.clone(),
            locality: Locality::OnDevice,
            billing: Billing::Free,
            capabilities: vec![],
        },
    )
}

/// The cascade for `set` (the placeholder one when none is given), with `timeouts` per stage.
pub fn reviewer(
    connection: &BusConnection,
    set: Option<&ReviewerSet>,
    timeouts: ReviewTimeouts,
) -> Option<InferReviewer<InferdModel<AnyTransport>>> {
    let placeholder = placeholder_set();
    let set = set.or(placeholder.as_ref())?;
    Some(InferReviewer {
        quick: model(connection, &set.quick),
        deliberate: model(connection, &set.deliberate),
        second: model(connection, &set.second),
        timeouts,
    })
}
