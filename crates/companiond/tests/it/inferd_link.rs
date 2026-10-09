//! companiond's planner reaches inferd over porter-client's D-Bus transport on the daemon's own
//! connection. The planner is filled and tested over a scripted model (`tests/it/planner.rs`); this one
//! holds only that the constructor is built on the bus and calls nothing until the first session.

use crate::support::bus::PrivateBus;
use companiond::PlannerModel;

#[tokio::test]
async fn the_planner_is_built_on_the_bus() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let connection = bus.connect().await;
    let planner = PlannerModel::new(docket_dbus::inferd_transport(
        &connection,
        docket_dbus::tap::Tap::off(),
    ));
    assert!(format!("{planner:?}").contains("Dbus"), "{planner:?}");
}
