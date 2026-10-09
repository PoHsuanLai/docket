//! Choosing the model: the agent's offer is read from its `session/new` reply, the configured
//! model is switched to and the reply awaited, and one it does not offer is refused with the list.

use super::agent::{Auth, Offer, View};
use super::rig::{CWD, Fakes, Setup, opening, wired_over};
use docket_acp::client::fake::FakeFiles;
use docket_acp::client::{AgentHost, ModelId};
use docket_session::{BackendFault, HostFault, SessionHost};
use serde_json::json;

fn model(id: &str) -> Option<ModelId> {
    Some(ModelId::parse(id).expect("a model id"))
}

async fn open_with(offer: Offer, wanted: Option<ModelId>) -> (Result<(), HostFault>, View) {
    let wired = wired_over::<Fakes>(
        FakeFiles::new(),
        Setup {
            offer,
            model: wanted,
            auth: Auth::Open,
            ..Setup::default()
        },
    );
    let mut host = AgentHost::new(
        wired.backend,
        wired.court.clone(),
        wired.desk.clone(),
        wired.fallback,
    );
    let opened = host.open(opening(CWD)).await.map(|_| ());
    (opened, wired.agent)
}

#[tokio::test]
async fn the_configured_model_is_set_with_the_session_option_and_awaited() {
    let (opened, agent) = open_with(Offer::Option, model("gemini-pro-agent")).await;
    assert_eq!(opened, Ok(()));
    let sessions = agent.switches();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].0, "session/set_config_option");
    assert_eq!(sessions[0].1["configId"], json!("model"));
    assert_eq!(sessions[0].1["value"], json!("gemini-pro-agent"));
}

#[tokio::test]
async fn the_older_model_list_is_switched_with_set_model() {
    let (opened, agent) = open_with(Offer::Older, model("gemini-pro-agent")).await;
    assert_eq!(opened, Ok(()));
    let sessions = agent.switches();
    assert_eq!(sessions[0].0, "session/set_model");
    assert_eq!(sessions[0].1["modelId"], json!("gemini-pro-agent"));
}

#[tokio::test]
async fn a_model_that_is_not_offered_is_refused_with_the_ones_that_are() {
    for offer in [Offer::Option, Offer::Older] {
        let (opened, agent) = open_with(offer, model("gpt-9")).await;
        let Err(HostFault::Backend(BackendFault::ModelNotOffered(offered))) = opened else {
            panic!("expected the list of models, got {opened:?}");
        };
        let ids: Vec<_> = offered.0.iter().map(|c| c.id.as_str()).collect();
        let names: Vec<_> = offered.0.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(ids, ["flash", "gemini-pro-agent"]);
        assert_eq!(names, ["Gemini Flash", "Gemini 3.1 Pro"]);
        assert!(agent.switches().is_empty(), "nothing was sent");
    }
}

#[tokio::test]
async fn the_model_already_in_use_is_not_switched_to() {
    let (opened, agent) = open_with(Offer::Option, model("flash")).await;
    assert_eq!(opened, Ok(()));
    assert!(agent.switches().is_empty());
}

#[tokio::test]
async fn an_agent_that_offers_no_choice_refuses_a_configured_model() {
    let (opened, _agent) = open_with(Offer::None, model("flash")).await;
    assert_eq!(
        opened,
        Err(HostFault::Backend(BackendFault::ModelNotChoosable))
    );
}

#[tokio::test]
async fn an_agent_that_refuses_the_switch_is_a_typed_refusal() {
    let (opened, _agent) = open_with(Offer::Stubborn, model("gemini-pro-agent")).await;
    assert_eq!(opened, Err(HostFault::Backend(BackendFault::ModelRefused)));
}

#[tokio::test]
async fn no_model_configured_leaves_the_agents_own_and_lists_what_it_offers() {
    let wired = wired_over::<Fakes>(
        FakeFiles::new(),
        Setup {
            offer: Offer::Option,
            ..Setup::default()
        },
    );
    let mut host = AgentHost::new(
        wired.backend,
        wired.court.clone(),
        wired.desk.clone(),
        wired.fallback,
    );
    host.open(opening(CWD)).await.expect("open");
    assert!(wired.agent.switches().is_empty());
    let models = host.backend().models().expect("offered");
    assert_eq!(models.current.as_deref(), Some("flash"));
    assert_eq!(models.available.len(), 2);
}

#[test]
fn a_model_id_has_no_spaces() {
    assert!(ModelId::parse("gemini-pro-agent").is_ok());
    for bad in ["", "two words", "tab\t"] {
        assert!(ModelId::parse(bad).is_err(), "{bad:?}");
    }
}
