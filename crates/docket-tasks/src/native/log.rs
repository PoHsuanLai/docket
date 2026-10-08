//! `RouterLog`: the durable log of sessions as an edge outside the router reads it, through
//! `Session.Stored`. memoryd answers log reads for the router and the shell only, and the router
//! lists and pages only the sessions the caller may bring back, so an editor\'s host asks the
//! router. The router writes the log of a native session, so an append here is refused.

use docket_client::{ClientError, Intents, Transport as IntentsTransport};
use docket_core::{StoredAsk, StoredView, WireRefusal};
use docket_session::{
    Appended, LogFault, LogPage, Logged, PageSize, Seq, SessionEntry, SessionLog,
};
use prov::SessionId;
use std::borrow::Borrow;
use std::marker::PhantomData;

/// The log, read through a router link.
#[derive(Debug)]
pub struct RouterLog<I: IntentsTransport, H: Borrow<Intents<I>> = Intents<I>> {
    intents: H,
    transport: PhantomData<fn() -> I>,
}

impl<I: IntentsTransport> RouterLog<I> {
    /// The log as `intents` reaches it.
    pub fn new(intents: Intents<I>) -> Self {
        Self {
            intents,
            transport: PhantomData,
        }
    }
}

impl<'a, I: IntentsTransport> RouterLog<I, &'a Intents<I>> {
    /// The log as a link someone else holds reaches it.
    pub fn reading(intents: &'a Intents<I>) -> Self {
        Self {
            intents,
            transport: PhantomData,
        }
    }
}

fn fault_of(error: &ClientError) -> LogFault {
    match error {
        ClientError::Refused(WireRefusal::NotAllowed) => LogFault::Refused,
        ClientError::Refused(_) | ClientError::Transport(_) | ClientError::Unexpected => {
            LogFault::Unavailable
        }
    }
}

impl<I: IntentsTransport, H: Borrow<Intents<I>> + Send + Sync> SessionLog for RouterLog<I, H> {
    async fn append(
        &self,
        _session: &SessionId,
        _seq: Seq,
        _entry: &SessionEntry,
    ) -> Result<Appended, LogFault> {
        Err(LogFault::Refused)
    }

    async fn page(
        &self,
        session: &SessionId,
        from: Option<Seq>,
        size: PageSize,
    ) -> Result<LogPage, LogFault> {
        let ask = StoredAsk::Rows {
            session: session.clone(),
            from: from.map(|s| s.0),
            size: size.0.0,
        };
        match self.intents.borrow().session_stored(ask).await {
            Ok(StoredView::Rows { rows, next }) => Ok(LogPage {
                rows: rows
                    .iter()
                    .map(|r| serde_json::from_str::<Logged>(&r.json).map_err(|_| LogFault::Encode))
                    .collect::<Result<_, _>>()?,
                next: next.map(Seq),
            }),
            Ok(StoredView::Sessions(_) | StoredView::Forked(_)) => Err(LogFault::Unavailable),
            // A session the router will not show this caller is a session with no rows.
            Err(ClientError::Refused(WireRefusal::NoSuchSession)) => Ok(LogPage {
                rows: Vec::new(),
                next: None,
            }),
            Err(error) => Err(fault_of(&error)),
        }
    }

    async fn sessions(&self) -> Result<Vec<SessionId>, LogFault> {
        match self.intents.borrow().session_stored(StoredAsk::List).await {
            Ok(StoredView::Sessions(all)) => Ok(all),
            Ok(StoredView::Rows { .. } | StoredView::Forked(_)) => Err(LogFault::Unavailable),
            Err(error) => Err(fault_of(&error)),
        }
    }
}
