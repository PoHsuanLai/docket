//! `IntentsCourt`: the router as `Intents1`, over any `docket-client` transport (the session bus
//! in the process, an in-process router in a test). The host's role is `acp_agent`: it opens the
//! agent's session, records the person's turns, and performs the agent's calls.
//!
//! A call runs as a task of its own that outlives a dropped `call` future, and asking again with
//! the same number waits for that one task: the backend's pull is cancel-safe (see `Court`), and a
//! second router call would put a second sheet in front of the person.

use super::call::AgentCall;
use super::court::{Court, CourtFault, OpenAgent, Ruled};
use docket_client::{Intents, Transport};
use docket_core::{CallRefusal, ContextKeep, DenyCode, Keep, Origin, SessionOpen, TurnIn, TurnVia};
use prov::{AgentRef, SessionId, SpaceId};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::Notify;

/// One call in flight, and where its answer lands.
#[derive(Debug, Default)]
struct Flight {
    done: Mutex<Option<Ruled>>,
    wake: Notify,
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The router, as the host of an external agent calls it.
pub struct IntentsCourt<T: Transport + 'static> {
    intents: Arc<Intents<T>>,
    flights: Arc<Mutex<BTreeMap<u64, Arc<Flight>>>>,
}

impl<T: Transport + 'static> Clone for IntentsCourt<T> {
    fn clone(&self) -> Self {
        Self {
            intents: self.intents.clone(),
            flights: self.flights.clone(),
        }
    }
}

impl<T: Transport + 'static> std::fmt::Debug for IntentsCourt<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IntentsCourt")
    }
}

impl<T: Transport + 'static> IntentsCourt<T> {
    /// Calls the router through `transport`.
    pub fn over(transport: T) -> Self {
        Self {
            intents: Arc::new(Intents::over(transport)),
            flights: Arc::default(),
        }
    }

    /// The flight numbered `n`, started with `request` if there is none yet.
    fn flight(&self, n: u64, session: &SessionId, call: &AgentCall) -> Arc<Flight> {
        let mut flights = locked(&self.flights);
        if let Some(flight) = flights.get(&n) {
            return flight.clone();
        }
        let flight = Arc::new(Flight::default());
        flights.insert(n, flight.clone());
        let request = call.request();
        let (intents, session, landing) = (self.intents.clone(), session.clone(), flight.clone());
        tokio::spawn(async move {
            let ruled = match request {
                // A name or path the router's grammar refuses is not a call it was asked.
                None => Ruled::Refused(CallRefusal::Denied(DenyCode::NotAllowed)),
                Some(request) => match intents.perform(request, Some(session), None).await {
                    Ok(Ok(outcome)) => Ruled::Done(Box::new(outcome)),
                    Ok(Err(why)) => Ruled::Refused(why),
                    Err(_) => Ruled::Lost,
                },
            };
            *locked(&landing.done) = Some(ruled);
            landing.wake.notify_one();
        });
        flight
    }
}

fn kept_nothing() -> ContextKeep {
    ContextKeep {
        query: Keep::Dropped,
        results: Keep::Dropped,
        selection: Keep::Dropped,
        window: Keep::Dropped,
    }
}

impl<T: Transport + 'static> Court for IntentsCourt<T> {
    async fn open(&mut self, open: OpenAgent) -> Result<SessionId, CourtFault> {
        let opened = self
            .intents
            .session_open(SessionOpen {
                space: SpaceId::desktop(),
                agent: AgentRef::Companion,
                parent: None,
                cwd: Some(open.cwd.clone()),
                started_from: None,
                external: Some(open.external()),
            })
            .await
            .map_err(|_| CourtFault::Refused)?;
        Ok(opened.session)
    }

    async fn turn(&mut self, session: &SessionId, text: &str) -> Result<(), CourtFault> {
        let said = TurnIn {
            text: text.to_owned(),
            origin: Origin::InWindowField,
            keep: kept_nothing(),
            via: TurnVia::Typed,
        };
        self.intents
            .session_turn(session.clone(), said)
            .await
            .map(|_| ())
            .map_err(|_| CourtFault::Refused)
    }

    async fn call(&mut self, session: &SessionId, n: u64, call: &AgentCall) -> Ruled {
        let flight = self.flight(n, session, call);
        loop {
            if let Some(ruled) = locked(&flight.done).take() {
                locked(&self.flights).remove(&n);
                return ruled;
            }
            flight.wake.notified().await;
        }
    }

    async fn close(&mut self, session: &SessionId) {
        let _ = self.intents.session_close(session.clone()).await;
    }
}
