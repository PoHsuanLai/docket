//! What the router reads from memory for a task, one section at a time. Each answers for one
//! session; a router that does not answer leaves its section empty, and the planner works with
//! less, never with a guess. The companion fills every section; a smaller agent (docket-kit)
//! fills only those it was given.

use almanac_core::{InjectQuery, RecentQuery};
use docket_client::{Intents, Transport as IntentsTransport};
use docket_core::{EpisodeLine, PrimerText, ProfileLine, RecallAsk, RecallView, RecalledLine};
use prov::SessionId;

/// The Space's primer, if it has one.
pub async fn primer_of<I: IntentsTransport>(
    intents: &Intents<I>,
    session: &SessionId,
) -> Option<PrimerText> {
    match intents
        .session_recall(session.clone(), RecallAsk::Primer)
        .await
    {
        Ok(RecallView::Primer(text)) if !text.0.is_empty() => Some(text),
        _ => None,
    }
}

/// The facts the person stated themselves.
pub async fn profile_of<I: IntentsTransport>(
    intents: &Intents<I>,
    session: &SessionId,
) -> Vec<ProfileLine> {
    match intents
        .session_recall(session.clone(), RecallAsk::Profile)
        .await
    {
        Ok(RecallView::Profile(lines)) => lines,
        _ => Vec::new(),
    }
}

/// The hits `query` finds, every untrusted text a handle.
pub async fn hits_of<I: IntentsTransport>(
    intents: &Intents<I>,
    session: &SessionId,
    query: InjectQuery,
) -> Vec<RecalledLine> {
    match intents
        .session_recall(session.clone(), RecallAsk::Inject(query))
        .await
    {
        Ok(RecallView::Hits(hits)) => hits,
        _ => Vec::new(),
    }
}

/// The episodes the router holds for the session's Space, as the planner reads them.
pub async fn episodes_of<I: IntentsTransport>(
    intents: &Intents<I>,
    session: &SessionId,
    query: RecentQuery,
) -> Vec<EpisodeLine> {
    match intents
        .session_recall(session.clone(), RecallAsk::Episodes(query))
        .await
    {
        Ok(RecallView::Episodes(lines)) => lines,
        _ => Vec::new(),
    }
}
