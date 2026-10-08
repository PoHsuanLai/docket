//! The roles held through `org.quire.Acp` count only while `agent.acp.expose` is on. The name is
//! usually unowned (the feature is off by default), so without the gate any process of the person
//! could take it and play the editor and the companion. The host of an external coding agent
//! (`org.quire.AcpAgent`) waits the same way on `agent.acp.agents`.

use crate::support::world::*;
use docket_client::Transport;
use docket_core::{CallerRole, IntentsReply, IntentsRequest, RecallAsk, SessionOpen, WireRefusal};
use docket_settings::{AcpAgents, AcpExpose};
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
        started_from: None,
        external: None,
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

/// A turn, which the host of an agent may record and a plain app may not.
fn turn() -> IntentsRequest {
    IntentsRequest::SessionTurn {
        session: SessionId::parse("s-1").expect("session"),
        turn: docket_core::TurnIn {
            text: "go".to_owned(),
            origin: docket_core::Origin::InWindowField,
            keep: docket_core::ContextKeep {
                query: docket_core::Keep::Dropped,
                results: docket_core::Keep::Dropped,
                selection: docket_core::Keep::Dropped,
                window: docket_core::Keep::Dropped,
            },
            via: docket_core::TurnVia::Typed,
        },
    }
}

/// Whether the connection that owns the agent host's name is let make an `acp_agent`'s request.
async fn host_allowed(agents: AcpAgents, expose: AcpExpose) -> bool {
    let gate = AcpGate::new(expose);
    gate.set_agents(agents);
    let config = IntentdConfig {
        roles: BTreeMap::from([(CallerRole::AcpAgent, vec![app(docket_core::ACP_AGENT_APP)])]),
        ..IntentdConfig::empty()
    };
    let world = World::serving_gated(mail_router(), config, "test-runner.service", gate).await;
    let (host, _keep) = world.client(&[docket_core::ACP_AGENT_APP]).await;
    !refused(&host.call(turn()).await.expect("a reply"))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_agent_host_name_plays_its_role_only_while_the_agents_setting_is_on() {
    assert!(!host_allowed(AcpAgents::Off, AcpExpose::On).await);
    assert!(host_allowed(AcpAgents::On, AcpExpose::Off).await);
}
