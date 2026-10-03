//! companiond's planner reaches inferd over porter-client's D-Bus transport on the daemon's own
//! connection. `serve` and the planner's bodies are still `todo!()`, so only the constructor is
//! wired; nothing is called until the first session.

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
