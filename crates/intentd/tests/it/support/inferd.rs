//! A scripted inferd for the daemons' model tests: a porter-client `Transport` whose sessions
//! are porter-fake's scripted ones, wrapped so the test can read what was opened (the need, the
//! data class, the tier) and every frame the code under test sent. No bus, no engine, no GPU.
#![allow(dead_code)]

use porter_client::{Transport, TransportError};
use porter_core::{AccountsReply, AccountsRequest, DataClass, Need, Tier};
use porter_fake::{FakeInferSession, Script, ScriptStep};
use porter_infer::{
    ChatReply, ClientFrame, InferEvent, InferReply, InferSession, OpenOptions, RequestKind,
    ServedBy, SessionError, StopReason, TokenUsage,
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// What a session was opened for.
#[derive(Debug, Clone, PartialEq)]
pub struct Opened {
    pub need: Need,
    pub class: DataClass,
    pub tier: Tier,
}

#[derive(Debug, Default)]
struct Log {
    opened: Vec<Opened>,
    frames: Vec<ClientFrame>,
}

/// The transport: hands out the scripted sessions in order; with none left, inferd is away.
#[derive(Debug, Clone)]
pub struct ScriptedInferd {
    sessions: Arc<Mutex<VecDeque<FakeInferSession>>>,
    log: Arc<Mutex<Log>>,
}

impl ScriptedInferd {
    pub fn new(sessions: impl IntoIterator<Item = FakeInferSession>) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(sessions.into_iter().collect())),
            log: Arc::default(),
        }
    }

    /// One session that answers each chat request with this text and then ends the turn.
    pub fn answering(text: &str) -> Self {
        Self::new([chat_session(text)])
    }

    /// Nobody home.
    pub fn away() -> Self {
        Self::new([])
    }

    pub fn opened(&self) -> Vec<Opened> {
        self.log.lock().expect("lock").opened.clone()
    }

    pub fn frames(&self) -> Vec<ClientFrame> {
        self.log.lock().expect("lock").frames.clone()
    }
}

/// The session the code under test sees: porter-fake's, with its frames logged.
#[derive(Debug)]
pub struct LoggedSession {
    inner: FakeInferSession,
    log: Arc<Mutex<Log>>,
}

impl InferSession for LoggedSession {
    async fn send(&mut self, frame: ClientFrame) -> Result<(), SessionError> {
        self.log.lock().expect("lock").frames.push(frame.clone());
        self.inner.send(frame).await
    }

    async fn next(&mut self) -> Result<InferEvent, SessionError> {
        self.inner.next().await
    }
}

impl Transport for ScriptedInferd {
    type Session = LoggedSession;

    async fn call(&self, _request: AccountsRequest) -> Result<AccountsReply, TransportError> {
        Err(TransportError::Unreachable)
    }

    async fn open_with(
        &self,
        need: &Need,
        class: DataClass,
        tier: Tier,
        _options: &OpenOptions,
    ) -> Result<LoggedSession, TransportError> {
        self.log.lock().expect("lock").opened.push(Opened {
            need: need.clone(),
            class,
            tier,
        });
        let inner = self
            .sessions
            .lock()
            .expect("lock")
            .pop_front()
            .ok_or(TransportError::Unreachable)?;
        Ok(LoggedSession {
            inner,
            log: self.log.clone(),
        })
    }
}

pub fn served() -> ServedBy {
    ServedBy {
        account: porter_core::AccountId::parse("local").expect("account"),
        model: porter_core::ModelId::parse("scripted").expect("model"),
        locality: porter_core::Locality::OnDevice,
    }
}

pub fn usage() -> TokenUsage {
    TokenUsage {
        input: porter_core::Tokens(1),
        output: porter_core::Tokens(1),
        cached: porter_core::Tokens(0),
    }
}

/// A chat reply that ended its turn normally.
pub fn reply(text: &str) -> ChatReply {
    reply_stopped(text, StopReason::EndTurn)
}

pub fn reply_stopped(text: &str, stop: StopReason) -> ChatReply {
    ChatReply::new(text.to_owned(), stop, usage(), served())
}

/// A script: some text deltas, then the reply.
pub fn chat_script(reply: ChatReply) -> Script {
    Script {
        kind: RequestKind::Chat,
        steps: vec![
            ScriptStep::Emit(InferEvent::Routed(served())),
            ScriptStep::Emit(InferEvent::TextDelta(reply.text.clone())),
            ScriptStep::Emit(InferEvent::Finished(InferReply::Chat(reply))),
        ],
    }
}

pub fn chat_session(text: &str) -> FakeInferSession {
    FakeInferSession::scripted([chat_script(reply(text))])
}

/// A session whose one turn ends in this reply (a refusal, a failure, a cancel).
pub fn finishing(kind: RequestKind, reply: InferReply) -> FakeInferSession {
    FakeInferSession::scripted([Script {
        kind,
        steps: vec![ScriptStep::Emit(InferEvent::Finished(reply))],
    }])
}
