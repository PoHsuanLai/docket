//! inferd, scripted: a transport whose sessions are porter-fake's `FakeInferSession`, with the
//! frames the daemon sent logged, and the `next` of an empty script waiting instead of closing
//! (the fake closes when nothing is ready; a real session waits).

use porter_client::{InferSession, OpenOptions, SessionError, Transport, TransportError};
use porter_core::{AccountsReply, AccountsRequest, DataClass, Need, Tier};
use porter_fake::{FakeInferSession, Script};
use porter_infer::{ClientFrame, InferEvent};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

/// One open the daemon made.
#[derive(Debug, Clone, PartialEq)]
pub struct Opened {
    pub need: Need,
    pub class: DataClass,
    pub tier: Tier,
}

#[derive(Debug, Default)]
pub struct Seen {
    pub opens: Mutex<Vec<Opened>>,
    /// Every frame sent, per session in order of opening.
    pub frames: Mutex<Vec<Vec<ClientFrame>>>,
}

impl Seen {
    pub fn frames_of(&self, session: usize) -> Vec<ClientFrame> {
        self.frames
            .lock()
            .expect("frames")
            .get(session)
            .cloned()
            .unwrap_or_default()
    }

    pub fn opens(&self) -> Vec<Opened> {
        self.opens.lock().expect("opens").clone()
    }
}

#[derive(Debug, Clone)]
pub struct ScriptedInfer {
    scripts: Arc<Mutex<VecDeque<Vec<Script>>>>,
    pub seen: Arc<Seen>,
    hold: Option<Arc<Notify>>,
    fail: Option<TransportError>,
}

impl ScriptedInfer {
    /// One script list per session, in the order the daemon opens them.
    pub fn new(sessions: Vec<Vec<Script>>) -> Self {
        Self {
            scripts: Arc::new(Mutex::new(sessions.into())),
            seen: Arc::new(Seen::default()),
            hold: None,
            fail: None,
        }
    }

    /// Opens wait until the returned handle is notified (a cold engine).
    pub fn cold(mut self) -> (Self, Arc<Notify>) {
        let gate = Arc::new(Notify::new());
        self.hold = Some(gate.clone());
        (self, gate)
    }

    /// Every open fails with `error`.
    pub fn unreachable(mut self, error: TransportError) -> Self {
        self.fail = Some(error);
        self
    }
}

#[derive(Debug)]
pub struct ScriptedSession {
    inner: FakeInferSession,
    index: usize,
    seen: Arc<Seen>,
    wake: Arc<Notify>,
}

impl InferSession for ScriptedSession {
    async fn send(&mut self, frame: ClientFrame) -> Result<(), SessionError> {
        self.seen.frames.lock().expect("frames")[self.index].push(frame.clone());
        let sent = self.inner.send(frame).await;
        self.wake.notify_one();
        sent
    }

    async fn next(&mut self) -> Result<InferEvent, SessionError> {
        loop {
            match self.inner.next().await {
                Err(SessionError::Closed) => self.wake.notified().await,
                other => return other,
            }
        }
    }
}

impl Transport for ScriptedInfer {
    type Session = ScriptedSession;

    async fn call(&self, _request: AccountsRequest) -> Result<AccountsReply, TransportError> {
        Err(TransportError::Unreachable)
    }

    async fn open_with(
        &self,
        need: &Need,
        class: DataClass,
        tier: Tier,
        _options: &OpenOptions,
    ) -> Result<ScriptedSession, TransportError> {
        if let Some(gate) = &self.hold {
            gate.notified().await;
        }
        if let Some(error) = self.fail.clone() {
            return Err(error);
        }
        self.seen.opens.lock().expect("opens").push(Opened {
            need: need.clone(),
            class,
            tier,
        });
        let scripts = self
            .scripts
            .lock()
            .expect("scripts")
            .pop_front()
            .unwrap_or_default();
        let mut frames = self.seen.frames.lock().expect("frames");
        frames.push(Vec::new());
        Ok(ScriptedSession {
            inner: FakeInferSession::scripted(scripts),
            index: frames.len() - 1,
            seen: self.seen.clone(),
            wake: Arc::new(Notify::new()),
        })
    }
}
