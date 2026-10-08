//! The host of an external agent also makes the agent's own tool calls (the per-session edge):
//! calls of the actions an MCP client is offered, in the agent's session, as the agent's program.
//! Nothing else: not an action nobody offered, not a session the host did not open.

use crate::acp_agent::{CLAUDE, host, world};
use crate::support::*;
use docket_core::*;
use prov::Actor;

fn read_thread() -> CallRequest {
    call("mail.thread.read", &["t2"], vec![])
}

#[tokio::test]
async fn an_offered_action_is_a_call_of_the_agents_program_in_its_session() {
    let w = world();
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    let done = w.call(&session, read_thread()).await;
    // The class is the person's data, not the directory the agent was launched in: it asks, as a
    // planner's first use does, and the answer here is none.
    assert!(matches!(done, Err(CallRefusal::Unconfirmed(_))), "{done:?}");
    assert_eq!(w.sheets().len(), 1);
    let program = prov::AgentProgram::parse(CLAUDE).expect("program");
    let actors: Vec<Actor> = w
        .records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Call { actor, action, .. }
                if action.name.as_str() == "mail.thread.read" =>
            {
                Some(actor)
            }
            _ => None,
        })
        .collect();
    assert_eq!(actors, [Actor::Acp { program }]);
}

#[tokio::test]
async fn an_action_nobody_offered_is_not_the_hosts_to_call() {
    let w = world();
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    // `mail.thread.open` is hidden from every planner and MCP client.
    let done = w
        .call(&session, call("mail.thread.open", &["t2"], vec![]))
        .await;
    assert_eq!(done, Err(CallRefusal::Denied(DenyCode::NotAllowed)));
    assert!(w.sheets().is_empty());
}

#[tokio::test]
async fn another_host_cannot_make_tool_calls_in_this_hosts_session() {
    let w = world();
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    let other = caller("org.example.OtherHost", CallerRole::AcpAgent);
    let reply = ask(
        &w.router,
        &other,
        IntentsRequest::Perform {
            activation: None,
            call: read_thread(),
            session: Some(session),
            parent_window: None,
        },
    )
    .await;
    assert_eq!(reply, IntentsReply::Refused(WireRefusal::NotAllowed));
    assert!(w.sheets().is_empty());
    let _ = host();
}

#[tokio::test]
async fn the_host_names_a_session_for_every_call() {
    let w = world();
    let _ = w.open(CLAUDE, SheetSurface::Desktop).await;
    let reply = ask(
        &w.router,
        &host(),
        IntentsRequest::Perform {
            activation: None,
            call: read_thread(),
            session: None,
            parent_window: None,
        },
    )
    .await;
    assert_eq!(reply, IntentsReply::Refused(WireRefusal::Malformed));
}
