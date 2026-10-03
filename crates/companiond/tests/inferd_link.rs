//! companiond's planner reaches inferd over porter-client's D-Bus transport on the daemon's own
//! connection. The planner is filled and tested over a scripted model (`tests/planner.rs`); this one
//! holds only that the constructor is built on the bus and calls nothing until the first session.

mod support;

use companiond::PlannerModel;
use support::bus::PrivateBus;

#[tokio::test]
async fn the_planner_is_built_on_the_bus() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let connection = bus.connect().await;
    let planner = PlannerModel::on_bus(&connection);
    assert!(format!("{planner:?}").contains("Dbus"), "{planner:?}");
}
