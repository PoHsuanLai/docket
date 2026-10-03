//! readerd serves `org.quire.Reader1` as declared: the interface on the bus is the one in
//! `dbus/org.quire.Reader1.xml`, and the name is claimed once.

#[path = "../../intentd/tests/support/inferd.rs"]
mod inferd;
mod support;

use docket_client::{InProcess, Intents};
use docket_core::{AgentConfig, CallerId, CallerRole};
use docket_dbus::{Bus, introspection};
use docket_fake::fake_router;
use inferd::ScriptedInferd;
use porter_core::{AppId, AppName, Isolation};
use readerd::{ReaderHost, ReaderService, ServeFault, serve_on};
use std::collections::BTreeSet;
use std::sync::Arc;
use support::bus::PrivateBus;

fn service() -> Arc<ReaderService<ScriptedInferd, InProcess<docket_fake::FakeSeams>>> {
    let router = Arc::new(fake_router(AgentConfig::default()).expect("router"));
    let reader = CallerId {
        app: AppId {
            name: AppName::parse("org.quire.Reader1").expect("app"),
            isolation: Isolation::Unsandboxed,
        },
        roles: BTreeSet::from([CallerRole::Reader]),
    };
    Arc::new(ReaderService::new(
        ReaderHost::start(),
        ScriptedInferd::away(),
        Intents::over(InProcess::new(router, reader)),
    ))
}

fn block(xml: &str, name: &str) -> String {
    let start = xml
        .find(&format!("<interface name=\"{name}\">"))
        .unwrap_or_else(|| panic!("{name} missing"));
    let rest = &xml[start..];
    let end = rest.find("</interface>").expect("end") + "</interface>".len();
    rest[..end]
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn the_served_interface_is_the_declared_one_and_the_name_is_claimed_once() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let first = bus.connect().await;
    serve_on(&first, service()).await.expect("serves");
    let proxy = zbus::fdo::IntrospectableProxy::builder(&first)
        .destination(docket_dbus::READER_BUS)
        .expect("destination")
        .path(docket_dbus::READER_PATH)
        .expect("path")
        .build()
        .await
        .expect("proxy");
    let served = proxy.introspect().await.expect("introspection");
    assert_eq!(
        block(&served, "org.quire.Reader1"),
        block(&introspection(Bus::Reader), "org.quire.Reader1")
    );

    let second = bus.connect().await;
    let again = serve_on(&second, service()).await;
    assert!(matches!(again, Err(ServeFault::Bus(_))), "{again:?}");
}
