//! intentd's inferd link is porter-client's D-Bus transport on the daemon's own connection. The
//! bus here is a private one with no inferd on it, so the first `open` says inferd is away (the
//! reviewer then asks the person) instead of the daemon failing to start. The link is
//! `docket_dbus::inferd_transport`, the one constructor intentd, companiond and readerd share.

use crate::support::bus::PrivateBus;
use intentd::{InferdModel, InferdWriter, inferd_transport};
use porter_client::{AnyTransport, Transport, TransportError};
use porter_core::Tokens;
use porter_core::capability::LlmFeature;
use porter_core::need::LlmNeed;
use porter_core::{DataClass, Need, Tier};
use porter_infer::{Model, ModelCard};
use std::collections::BTreeSet;

#[tokio::test]
async fn the_link_is_the_dbus_transport_and_an_absent_inferd_is_unreachable() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let connection = bus.connect().await;

    let link = inferd_transport(&connection, docket_dbus::tap::Tap::off());
    assert!(matches!(link.inner(), AnyTransport::Dbus(_)), "{link:?}");
    let need = Need::Llm(LlmNeed::new(
        BTreeSet::from([LlmFeature::Chat]),
        Tokens(4096),
    ));
    let opened = link.open(&need, DataClass::Prompt, Tier::Fast).await;
    assert!(
        matches!(opened, Err(TransportError::Unreachable)),
        "{:?}",
        opened.map(|_| ())
    );
}

#[tokio::test]
async fn the_models_and_the_writer_are_built_on_the_bus() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let connection = bus.connect().await;
    let card = ModelCard {
        account: porter_core::AccountId::parse("local").expect("account"),
        model: porter_core::ModelId::parse("q").expect("model"),
        locality: porter_core::Locality::OnDevice,
        billing: porter_core::Billing::Free,
        capabilities: vec![],
    };
    let model = InferdModel::on_bus(&connection, card.clone(), docket_dbus::tap::Tap::off());
    assert_eq!(model.card(), &card);
    assert!(format!("{model:?}").contains("Dbus"), "{model:?}");
    let writer = InferdWriter::on_bus(&connection, docket_dbus::tap::Tap::off());
    assert!(format!("{writer:?}").contains("Dbus"), "{writer:?}");
}
