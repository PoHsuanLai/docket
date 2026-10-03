//! readerd's service reaches inferd over porter-client's D-Bus transport on the daemon's own
//! connection. `serve` and the service's bodies are still `todo!()`, so only the constructor is
//! wired; nothing is called until the first read.

mod support;

use docket_client::{DbusTransport, Intents};
use readerd::{ReaderHost, ReaderService};
use support::bus::PrivateBus;

#[tokio::test]
async fn the_service_is_built_on_the_bus() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let connection = bus.connect().await;
    let intents = Intents::over(DbusTransport::new(connection.clone()));
    let service = ReaderService::on_bus(ReaderHost::start(), &connection, intents);
    assert!(format!("{service:?}").contains("Dbus"), "{service:?}");
}
