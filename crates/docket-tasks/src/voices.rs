//! Whose voice a session's turns are in. The assistant's planner, role and model drive every
//! session of a host; a helper's conversation is the exception, and is driven exactly as one of
//! the helper's jobs is: its role, the model the person chose for it (or the pool the router
//! chooses from, with the helper as the one asking). Its reach and ceiling are the router's: the
//! session's seat cuts every policy to them.

use crate::desk::Pending;
use crate::runtime::Companion;
use crate::seams::Surface;
use docket_client::Transport as IntentsTransport;
use docket_core::Clock;
use docket_planner::Voice;
use porter_client::Transport as InferTransport;
use prov::SessionId;
use std::fmt;
use std::sync::Arc;

/// Which sessions are a helper's conversation, and the voice each is in.
pub trait Voices: fmt::Debug + Send + Sync {
    /// The voice of `session`, or none when it is the assistant's (or the helper is gone).
    fn voice_of(&self, session: &SessionId) -> Pending<'_, Option<Voice>>;
}

/// What a companion that gives helpers' conversations their own voice keeps.
#[derive(Debug, Clone)]
pub(crate) struct Speaking {
    voices: Arc<dyn Voices>,
    /// The assistant's own, put back for every other session.
    own: Voice,
}

impl<P: InferTransport, I: IntentsTransport, K: Clock, S: Surface> Companion<P, I, K, S> {
    /// The same companion, driving the conversations `voices` names in their helper's voice and
    /// every other session in the voice it has now.
    pub fn with_voices(mut self, voices: Arc<dyn Voices>) -> Self {
        self.speaking = Some(Speaking {
            voices,
            own: self.planner.voice(),
        });
        self
    }

    /// Sets the voice for a turn in `session`: the helper's when it is a helper's conversation,
    /// the assistant's own otherwise. Every turn sets it, so one session never leaves its voice
    /// on another.
    pub async fn speak_as(&mut self, session: &SessionId) {
        let Some(speaking) = self.speaking.clone() else {
            return;
        };
        let voice = speaking
            .voices
            .voice_of(session)
            .await
            .unwrap_or(speaking.own);
        self.planner.set_voice(voice);
    }
}
