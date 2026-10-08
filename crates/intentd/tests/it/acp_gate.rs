//! The roles held through `org.quire.Acp` count only while `agent.acp.expose` is on. The name is
//! usually unowned (the feature is off by default), so without the gate any process of the person
//! could take it and play the editor and the companion.

use crate::support::world::*;
use docket_client::Transport;
use docket_core::{CallerRole, IntentsReply, IntentsRequest, RecallAsk, SessionOpen, WireRefusal};
use docket_settings::AcpExpose;
use intentd::{ACP_NAME, AcpGate, IntentdConfig};
use prov::{AgentRef, SessionId, SpaceId};
use std::collections::BTreeMap;

/// A configuration in which the ACP name plays exactly `role`.
fn only(role: CallerRole) -> IntentdConfig {
    IntentdConfig {
        roles: BTreeMap::from([(role, vec![app(ACP_NAME)])]),
        ..IntentdConfig::empty()
    }
}

fn open() -> IntentsRequest {
    IntentsRequest::SessionOpen(SessionOpen {
        space: SpaceId::desktop(),
        agent: AgentRef::Companion,
        parent: None,
        cwd: None,
    })
}

/// A member only a companion may call; what it answers for a missing session does not matter.
fn recall() -> IntentsRequest {
    IntentsRequest::SessionRecall {
        session: SessionId::parse("s-1").expect("session"),
        ask: RecallAsk::Primer,
    }
}

fn refused(reply: &IntentsReply) -> bool {
    *reply == IntentsReply::Refused(WireRefusal::NotAllowed)
}

/// What the connection that owns the ACP name is allowed to do in a world with `role` for it.
async fn allowed(role: CallerRole, request: IntentsRequest, expose: AcpExpose) -> bool {
    let gate = AcpGate::new(expose);
    let world = World::serving_gated(mail_router(), only(role), "test-runner.service", gate).await;
    let (acp, _keep) = world.client(&[ACP_NAME]).await;
    !refused(&acp.call(request).await.expect("a reply"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_the_setting_off_the_acp_name_is_neither_an_editor_nor_a_companion() {
    assert!(!allowed(CallerRole::Editor, open(), AcpExpose::Off).await);
    assert!(!allowed(CallerRole::Companion, recall(), AcpExpose::Off).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_the_setting_on_the_acp_name_plays_its_roles() {
    assert!(allowed(CallerRole::Editor, open(), AcpExpose::On).await);
    assert!(allowed(CallerRole::Companion, recall(), AcpExpose::On).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn flipping_the_setting_changes_the_next_call() {
    let gate = AcpGate::shut();
    let world = World::serving_gated(
        mail_router(),
        only(CallerRole::Editor),
        "test-runner.service",
        gate.clone(),
    )
    .await;
    let (acp, _keep) = world.client(&[ACP_NAME]).await;
    let mut seen = Vec::new();
    for expose in [AcpExpose::Off, AcpExpose::On, AcpExpose::Off] {
        gate.set(expose);
        seen.push(refused(&acp.call(open()).await.expect("a reply")));
    }
    assert_eq!(seen, [true, false, true]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn other_names_do_not_wait_on_the_setting() {
    let world = World::serving_gated(
        mail_router(),
        IntentdConfig::shipped().expect("shipped"),
        "test-runner.service",
        AcpGate::shut(),
    )
    .await;
    let (sill, _keep) = world.client(&["org.quire.Shell"]).await;
    assert!(!refused(&sill.call(open()).await.expect("a reply")));
}
