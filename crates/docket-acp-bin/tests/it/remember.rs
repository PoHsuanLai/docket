//! What an opening says about the agent, as the record that gets written: no agent runs here.

use docket_acp_bin::agent::remember::of_opening;
use docket_agents::Start;
use docket_session::{BackendFault, Choice, Choices};

fn choices(ids: &[&str]) -> Choices {
    Choices(
        ids.iter()
            .map(|id| Choice {
                id: (*id).to_owned(),
                name: format!("name of {id}"),
            })
            .collect(),
    )
}

#[test]
fn an_opened_session_is_signed_in_and_lists_the_ways_the_agent_gave() {
    let record = of_opening(None, None, &choices(&["login"])).expect("record");
    assert_eq!(record.start, Start::SignedIn);
    assert_eq!(record.ways.len(), 1);
    assert!(record.models.is_empty());
}

#[test]
fn a_sign_in_the_agent_asked_for_lists_its_ways() {
    let fault = BackendFault::SignInChoose(choices(&["google", "key"]));
    let record = of_opening(Some(&fault), None, &choices(&[])).expect("record");
    assert_eq!(record.start, Start::NeedsSignIn);
    assert_eq!(record.ways.len(), 2);
    let plain = of_opening(
        Some(&BackendFault::SignInNeeded),
        None,
        &choices(&["login"]),
    );
    assert_eq!(plain.expect("record").start, Start::NeedsSignIn);
}

#[test]
fn a_refused_model_still_records_the_models_it_offered() {
    let fault = BackendFault::ModelNotOffered(choices(&["a", "b"]));
    let record = of_opening(Some(&fault), None, &choices(&[])).expect("record");
    assert_eq!(record.start, Start::SignedIn);
    assert_eq!(record.models.len(), 2);
}

#[test]
fn a_start_that_failed_for_another_reason_records_nothing() {
    assert!(of_opening(Some(&BackendFault::Unavailable), None, &choices(&[])).is_none());
}
