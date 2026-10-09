//! The service behind `org.quire.Reader1`.

use crate::host::ReaderHost;
use docket_client::{Intents, Transport as IntentsTransport};
use docket_core::{Reader, ReaderAsk, ReaderError, Value};
use docket_dbus::InferLink;
use porter_client::Transport as InferTransport;
use prov::{Labelled, Quarantined, SessionId};

/// Reads for intentd: resolves handles through `Intents1.Session.Resolve` (the reader role),
/// asks the reader model through inferd (`Need::Llm` with structured output, no tools), and
/// answers a value that fits the schema.
#[derive(Debug)]
pub struct ReaderService<P: InferTransport, I: IntentsTransport> {
    host: ReaderHost,
    infer: P,
    intents: Intents<I>,
}

impl<I: IntentsTransport> ReaderService<InferLink, I> {
    /// A service over inferd on the session bus (`InferLink`) and the router. Nothing is
    /// called here: inferd is found, and started by activation, at the first session.
    pub fn on_bus(
        host: ReaderHost,
        connection: &docket_dbus::BusConnection,
        intents: Intents<I>,
        tap: docket_dbus::tap::Tap,
    ) -> Self {
        Self::new(
            host,
            docket_dbus::inferd_transport(connection, tap),
            intents,
        )
    }
}

impl<P: InferTransport, I: IntentsTransport> ReaderService<P, I> {
    /// A service over inferd and the router.
    pub fn new(host: ReaderHost, infer: P, intents: Intents<I>) -> Self {
        Self {
            host,
            infer,
            intents,
        }
    }

    /// One `Reader1.Extract(session, ask)`: resolves each handle of the ask in the session,
    /// then reads. A handle the router will not resolve (not this session's, or the caller is
    /// not the reader) fails the whole read: nothing is read from a part of the ask.
    pub async fn extract_in(
        &self,
        session: &SessionId,
        ask: ReaderAsk,
    ) -> Result<Value, ReaderError> {
        let mut inputs = Vec::with_capacity(ask.inputs.len());
        for handle in &ask.inputs {
            let resolved = self
                .intents
                .session_resolve(session.clone(), *handle)
                .await
                .map_err(|_| ReaderError::ModelUnavailable)?;
            inputs.push(Quarantined::new(Labelled {
                value: resolved.text,
                label: resolved.label,
            }));
        }
        self.extract(session, ask, inputs).await
    }
}

impl<P: InferTransport, I: IntentsTransport> Reader for ReaderService<P, I> {
    async fn extract(
        &self,
        _session: &SessionId,
        ask: ReaderAsk,
        inputs: Vec<Quarantined<String>>,
    ) -> Result<Value, ReaderError> {
        let opened: Vec<Labelled<String>> = inputs.into_iter().map(|q| self.host.open(q)).collect();
        docket_reader::read(&self.infer, &ask, &opened).await
    }
}
