//! The filter outside the sandbox of an `allowlist` agent: it listens on the socket the
//! forwarder inside hands every connection to, and lets through only what the program's lists
//! allow. The journal gets the site and the verdict, which are all bulkhead gives it: never a
//! header or a body.

use crate::reach::Reach;
use bulkhead::egress::{Entry, Gate, Journal, Proxy, Resolver};
use std::path::Path;
use std::sync::Arc;

/// Writes each verdict to the log: `host:port` and what was done.
#[derive(Debug, Clone, Copy, Default)]
pub struct TraceJournal;

impl Journal for TraceJournal {
    fn record(&self, entry: &Entry) {
        match &entry.target {
            Some(target) => {
                tracing::info!(%target, outcome = ?entry.outcome, "agent network request")
            }
            None => {
                tracing::info!(outcome = ?entry.outcome, "agent network request not understood")
            }
        }
    }
}

/// Starts the filter on `socket` (which must not exist yet). The socket goes when the returned
/// handle is dropped, so the caller keeps it exactly as long as the agent lives.
pub fn start(socket: &Path, reach: &Reach, resolver: Arc<dyn Resolver>) -> std::io::Result<Proxy> {
    let gate = Gate::new(reach.policy().clone(), resolver, Arc::new(TraceJournal));
    Proxy::start(socket, gate)
}
