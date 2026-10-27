//! `memory.propose` names the helper whose job asked, and only that: the person's own
//! assistant proposes as before. Driven straight at the provider over a recording transport.

use almanac_client::{Transport, TransportError};
use almanac_core::{FactId, FactState, MemoryReply, MemoryRequest};
use docket_core::{Args, CallId, Invocation, Origin, ParamName, TargetValue, Value};
use docket_memory::MemoryProvider;
use docket_provider::IntentProvider;
use prov::{ActionName, Actor, AgentRole, HelperId, Label, Labelled, SessionId, SpaceId, TaskId};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Default)]
struct Recording(Arc<Mutex<Vec<MemoryRequest>>>);

impl Transport for Recording {
    async fn call(&self, request: MemoryRequest) -> Result<MemoryReply, TransportError> {
        if let Ok(mut seen) = self.0.lock() {
            seen.push(request);
        }
        Ok(MemoryReply::Proposed(
            FactId::parse("0123456789abcdefghjkmnpqrs").expect("fact id"),
            FactState::Pending,
        ))
    }
}

impl Recording {
    fn proposed_for(&self) -> Vec<Option<Actor>> {
        let seen = self.0.lock().map(|s| s.clone()).unwrap_or_default();
        seen.into_iter()
            .filter_map(|r| match r {
                MemoryRequest::Propose(_, draft) => Some(draft.on_behalf),
                _ => None,
            })
            .collect()
    }
}

fn session() -> SessionId {
    SessionId::parse("s-7").expect("session")
}

fn helper_job() -> Actor {
    Actor::Companion {
        session: session(),
        role: AgentRole::Helper {
            helper: HelperId::parse("01J9ZK").expect("helper"),
            task: TaskId::parse("t-7").expect("task"),
        },
    }
}

fn propose_as(actor: Actor) -> Invocation {
    let text = Labelled {
        value: Value::Text("Sam prefers mornings".into()),
        label: Label::trusted_user(),
    };
    Invocation {
        call: CallId(1),
        action: ActionName::parse("memory.propose").expect("action"),
        target: TargetValue::Nothing,
        args: Args::from([(ParamName::parse("text").expect("param"), text)]),
        actor,
        origin: Origin::Companion,
        space: SpaceId::parse("work").expect("space"),
    }
}

async fn proposed_for(actor: Actor) -> Vec<Option<Actor>> {
    let memory = docket_core::parse(include_str!("../../../../manifests/org.quire.Memory.toml"))
        .expect("the memory manifest");
    let transport = Recording::default();
    let provider = MemoryProvider::new(memory, transport.clone());
    provider
        .perform(propose_as(actor))
        .await
        .expect("the proposal went through");
    transport.proposed_for()
}

#[tokio::test]
async fn a_helper_sessions_proposal_carries_its_helper_actor() {
    assert_eq!(proposed_for(helper_job()).await, vec![Some(helper_job())]);
}

#[tokio::test]
async fn the_persons_own_assistant_proposes_for_nobody() {
    let planner = Actor::Companion {
        session: session(),
        role: AgentRole::Planner,
    };
    assert_eq!(proposed_for(planner).await, vec![None]);
}
