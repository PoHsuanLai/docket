//! The reviewer cascade over inferd, from the configured set or the placeholder one. The set,
//! its cards and the placeholder are `docket-models`'; this makes each stage's link on the bus.

use action_review::{InferReviewer, ModelChoice, ReviewerSet};
use docket_core::ReviewTimeouts;
use docket_dbus::tap::Tap;
use docket_dbus::{BusConnection, InferLink};

use crate::infer::InferdModel;

pub use docket_models::placeholder_set;

fn model(connection: &BusConnection, choice: &ModelChoice, tap: &Tap) -> InferdModel<InferLink> {
    InferdModel::on_bus(connection, docket_models::card_of(choice), tap.clone())
}

/// The cascade for `set` (the placeholder one when none is given), with `timeouts` per stage, its
/// links behind `tap`.
pub fn reviewer(
    connection: &BusConnection,
    set: Option<&ReviewerSet>,
    timeouts: ReviewTimeouts,
    tap: &Tap,
) -> Option<InferReviewer<InferdModel<InferLink>>> {
    let placeholder = placeholder_set();
    let set = set.or(placeholder.as_ref())?;
    Some(InferReviewer {
        quick: model(connection, &set.quick, tap),
        deliberate: model(connection, &set.deliberate, tap),
        second: model(connection, &set.second, tap),
        timeouts,
    })
}
