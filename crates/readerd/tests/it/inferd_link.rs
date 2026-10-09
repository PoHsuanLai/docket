//! readerd's service reaches inferd over porter-client's D-Bus transport on the daemon's own
//! connection: the constructor wires it and nothing is called until the first read (the reads
//! themselves are tested over a scripted inferd in `service.rs`).

use crate::support::bus::PrivateBus;
use docket_client::{DbusTransport, Intents};
use readerd::{ReaderHost, ReaderService};

#[tokio::test]
async fn the_service_is_built_on_the_bus() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let connection = bus.connect().await;
    let intents = Intents::over(DbusTransport::new(connection.clone()));
    let service = ReaderService::on_bus(
        ReaderHost::start(),
        &connection,
        intents,
        docket_dbus::tap::Tap::off(),
    );
    assert!(format!("{service:?}").contains("Dbus"), "{service:?}");
}
