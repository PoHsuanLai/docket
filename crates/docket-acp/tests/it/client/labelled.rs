//! The label an external agent is shown under is the person's, from `agents.toml`: the host says
//! it when it opens the session, and nothing the agent says of itself changes it.

use super::agent::{Act, call};
use super::calls::read;
use super::rig::{Setup, run_turn, started};
use docket_core::AuditRecord;
use prov::{Actor, AgentLabel};

fn actors(rig: &super::rig::Rig<super::rig::Fakes>) -> Vec<Actor> {
    rig.audit()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Call { actor, .. } => Some(actor),
            _ => None,
        })
        .collect()
}

async fn acted(label: Option<AgentLabel>) -> Vec<Actor> {
    let (mut rig, _files) = started(Setup {
        turns: vec![vec![
            call("r", "fs/read_text_file", read("/work/app/README.md")),
            Act::Stop("end_turn"),
        ]],
        label,
        ..Setup::default()
    })
    .await;
    run_turn(&mut rig, "read it").await;
    actors(&rig)
}

#[tokio::test]
async fn the_hosts_label_is_on_the_audited_actor() {
    let label = AgentLabel("Claude Code".to_owned());
    let seen = acted(Some(label.clone())).await;
    assert!(!seen.is_empty());
    for actor in seen {
        assert!(matches!(actor, Actor::Acp { label: Some(l), .. } if l == label));
    }
}

#[tokio::test]
async fn no_label_configured_is_none_whatever_the_agent_calls_itself() {
    // The fake agent's `initialize` answers with a title of its own.
    let seen = acted(None).await;
    assert!(!seen.is_empty());
    for actor in seen {
        assert!(matches!(actor, Actor::Acp { label: None, .. }));
    }
}

#[tokio::test]
async fn the_agents_title_does_not_change_the_label() {
    let label = AgentLabel("Mine".to_owned());
    for actor in acted(Some(label.clone())).await {
        assert!(matches!(actor, Actor::Acp { label: Some(l), .. } if l == label));
    }
}
