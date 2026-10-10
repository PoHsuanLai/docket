//! A model that only talks: every chat is answered with the same words and no tool call. For an
//! example or a test that needs a planner to finish a turn without caring what it says.

use porter_client::{Transport, TransportError};
use porter_core::{
    AccountId, AccountsReply, AccountsRequest, DataClass, Locality, ModelId, Need, Tier, Tokens,
};
use porter_infer::{
    ChatReply, ClientFrame, InferEvent, InferReply, InferRequest, InferSession, OpenOptions,
    ServedBy, SessionError, StopReason, TokenUsage,
};

/// The model transport that answers every chat with `says`.
#[derive(Debug, Clone)]
pub struct WordsModel {
    says: String,
}

impl WordsModel {
    /// A model that always answers with these words.
    pub fn says(words: &str) -> Self {
        Self {
            says: words.to_owned(),
        }
    }

    fn reply(&self) -> Result<InferEvent, SessionError> {
        let malformed = |what: &str| SessionError::Malformed(what.to_owned());
        Ok(InferEvent::Finished(InferReply::Chat(ChatReply::new(
            self.says.clone(),
            StopReason::EndTurn,
            TokenUsage {
                input: Tokens(0),
                output: Tokens(0),
                cached: Tokens(0),
            },
            ServedBy {
                account: AccountId::parse("local").map_err(|_| malformed("account"))?,
                model: ModelId::parse("words").map_err(|_| malformed("model"))?,
                locality: Locality::OnDevice,
            },
        ))))
    }
}

/// One open conversation with a [`WordsModel`].
#[derive(Debug)]
pub struct WordsSession {
    model: WordsModel,
    asked: bool,
}

impl InferSession for WordsSession {
    async fn send(&mut self, frame: ClientFrame) -> Result<(), SessionError> {
        match frame {
            ClientFrame::Request(InferRequest::Chat(_)) => {
                self.asked = true;
                Ok(())
            }
            _ => Err(SessionError::Malformed("not a chat".to_owned())),
        }
    }

    async fn next(&mut self) -> Result<InferEvent, SessionError> {
        match std::mem::take(&mut self.asked) {
            true => self.model.reply(),
            false => Err(SessionError::Closed),
        }
    }
}

impl Transport for WordsModel {
    type Session = WordsSession;

    async fn call(&self, _request: AccountsRequest) -> Result<AccountsReply, TransportError> {
        Err(TransportError::Unreachable)
    }

    async fn open_with(
        &self,
        _need: &Need,
        _class: DataClass,
        _tier: Tier,
        _options: &OpenOptions,
    ) -> Result<WordsSession, TransportError> {
        Ok(WordsSession {
            model: self.clone(),
            asked: false,
        })
    }
}
