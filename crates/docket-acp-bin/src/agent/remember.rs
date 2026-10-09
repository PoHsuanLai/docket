//! What a session's opening says about the agent, written down for the Settings app: the models
//! it offered, the ways of signing in it listed, and whether it let the session open. Best
//! effort: a record that cannot be written changes nothing about the session.

use docket_acp::client::{AcpBackend, Models, Seams};
use docket_agents::offered::{Named, Offered};
use docket_agents::{AgentsDir, Start};
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
        let _ = record.write(dir, id);
    }
}
