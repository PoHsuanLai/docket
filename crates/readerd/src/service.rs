//! The service behind `org.quire.Reader1`.

use crate::host::ReaderHost;
use docket_client::{Intents, Transport as IntentsTransport};
use docket_core::{Reader, ReaderAsk, ReaderError, Value};
use porter_client::Transport as InferTransport;
use prov::{Quarantined, SessionId};

/// Reads for intentd: resolves handles through `Intents1.Session.Resolve` (the reader role),
/// asks the reader model through inferd (`Need::Llm` with structured output, no tools), and
/// answers a value that fits the schema.
#[derive(Debug)]
pub struct ReaderService<P: InferTransport, I: IntentsTransport> {
    host: ReaderHost,
    infer: P,
    intents: Intents<I>,
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
    /// then reads.
    pub async fn extract_in(
        &self,
        session: &SessionId,
        ask: ReaderAsk,
    ) -> Result<Value, ReaderError> {
        let _ = (&self.host, &self.infer, &self.intents, session, ask);
        todo!("ReaderService::extract_in: Session.Resolve for each handle, then Reader::extract")
    }
}

impl<P: InferTransport, I: IntentsTransport> Reader for ReaderService<P, I> {
    async fn extract(
        &self,
        ask: ReaderAsk,
        inputs: Vec<Quarantined<String>>,
    ) -> Result<Value, ReaderError> {
        let _ = (&self.host, &self.infer, ask, inputs);
        todo!(
            "ReaderService::extract: open the inputs with the host, reader_request, one session through porter-client, parse the reply into a Value, check it with docket_core::conforms; anything out of schema is ReaderError::OutOfSchema, never passed on"
        )
    }
}
