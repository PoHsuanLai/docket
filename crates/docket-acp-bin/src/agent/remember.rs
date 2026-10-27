//! What a session's opening says about the agent, written down for the Settings app: the models
//! it offered, the ways of signing in it listed, and whether it let the session open. Best
//! effort: a record that cannot be written changes nothing about the session.

use agents_core::dirs::AgentsDir;
use agents_core::offered::{Named, Offered, Start};
use docket_acp_host::{AcpBackend, Models, Seams};
use docket_agents::store::write_offered;
use docket_session::{BackendFault, Choice, Choices, HostFault};

fn named(choices: &[Choice]) -> Vec<Named> {
    choices
        .iter()
        .map(|c| Named {
            id: c.id.clone(),
            name: c.name.clone(),
        })
        .collect()
}

/// The record an opening leaves, before its counter is set; none when the opening failed for a
/// reason that says nothing about sign-in or models.
pub fn of_opening(
    fault: Option<&BackendFault>,
    models: Option<&Models>,
    listed: &Choices,
) -> Option<Offered> {
    let record = |start, in_use, models: Vec<Named>, ways: &Choices| Offered {
        seen: 0,
        start,
        in_use,
        models,
        ways: named(&ways.0),
    };
    match fault {
        None => Some(record(
            Start::SignedIn,
            models.and_then(|m| m.current.clone()),
            models.map_or_else(Vec::new, |m| named(&m.available)),
            listed,
        )),
        Some(BackendFault::SignInNeeded) => {
            Some(record(Start::NeedsSignIn, None, Vec::new(), listed))
        }
        Some(BackendFault::SignInChoose(ways)) => {
            Some(record(Start::NeedsSignIn, None, Vec::new(), ways))
        }
        Some(BackendFault::ModelNotOffered(offer)) => {
            Some(record(Start::SignedIn, None, named(&offer.0), listed))
        }
        Some(_) => None,
    }
}

/// Writes down what opening a session with `backend` showed of the agent installed as `id`.
pub fn remember<X: Seams>(
    dir: &AgentsDir,
    id: &str,
    backend: &AcpBackend<X>,
    opened: Result<(), &HostFault>,
) {
    let none = Choices(Vec::new());
    let fault = match opened {
        Ok(()) => None,
        Err(HostFault::Backend(fault)) => Some(fault),
        Err(_) => return,
    };
    let listed = backend.sign_in_ways().unwrap_or(&none);
    if let Some(record) = of_opening(fault, backend.models(), listed) {
        let _ = write_offered(dir, id, record);
    }
}

#[cfg(test)]
mod tests {
    //! What an opening says about the agent, as the record that gets written: no agent runs here.

    use super::of_opening;
    use agents_core::offered::Start;
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
}
