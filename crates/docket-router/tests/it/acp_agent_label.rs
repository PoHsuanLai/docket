//! The label an external agent is shown under is the person's, from `agents.toml`: the host says
//! it when it opens the session, and the audit names the agent with it.

use crate::acp_agent::{CLAUDE, read, world};
use docket_core::*;
use prov::{Actor, AgentLabel};

async fn actors_of_a_read(label: Option<AgentLabel>) -> Vec<Actor> {
    let w = world();
    let session = w.open_labelled(CLAUDE, SheetSurface::Desktop, label).await;
    w.call(&session, read("/home/u/proj/a.rs"))
        .await
        .expect("read");
    w.records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Call { actor, .. } => Some(actor),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn the_audited_actor_carries_the_label_the_host_gave() {
    let label = AgentLabel("Claude Code".to_owned());
    let program = prov::AgentProgram::parse(CLAUDE).expect("program");
    assert_eq!(
        actors_of_a_read(Some(label.clone())).await,
        [Actor::Acp {
            program,
            label: Some(label)
        }]
    );
}

#[tokio::test]
async fn with_no_label_configured_the_actor_has_none() {
    let program = prov::AgentProgram::parse(CLAUDE).expect("program");
    assert_eq!(
        actors_of_a_read(None).await,
        [Actor::Acp {
            program,
            label: None
        }]
    );
}
