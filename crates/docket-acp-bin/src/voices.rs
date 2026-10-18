//! Whose voice an editor's session is in. A session that is a helper's conversation (companiond
//! names it on the helper's line) is driven in that helper's voice: its role, and its model asked
//! for by the helper. Every other session is the assistant's.

use companion_client::HelpersTransport;
use docket_planner::Voice;
use docket_tasks::{Pending, Voices, conversation_voice};
use prov::SessionId;
use std::time::Duration;

/// How long a turn waits for the daemon to say whose voice it is before it goes on in the
/// assistant's.
const ANSWER_WITHIN: Duration = Duration::from_secs(2);

/// The voices of the helpers companiond keeps.
pub struct BusVoices<T: HelpersTransport> {
    transport: T,
}

impl<T: HelpersTransport> std::fmt::Debug for BusVoices<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BusVoices")
    }
}

impl<T: HelpersTransport> BusVoices<T> {
    /// Voices read from the daemon over `transport`.
    pub fn over(transport: T) -> Self {
        Self { transport }
    }

    async fn read(&self, session: &SessionId) -> Option<Voice> {
        // No sentences are wanted, only the list of helpers.
        let view = self.transport.list(u64::MAX).await.ok()?;
        let line = view
            .helpers
            .into_iter()
            .find(|line| line.conversation.as_ref() == Some(session))?;
        let card = self.transport.card(&line.id).await.ok()?;
        conversation_voice(&line.id, &line.name, &card.role, &card.model).ok()
    }
}

impl<T: HelpersTransport> Voices for BusVoices<T> {
    fn voice_of(&self, session: &SessionId) -> Pending<'_, Option<Voice>> {
        let session = session.clone();
        Box::pin(async move {
            tokio::time::timeout(ANSWER_WITHIN, self.read(&session))
                .await
                .ok()
                .flatten()
        })
    }
}
