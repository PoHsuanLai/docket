//! What the bus handlers ask the daemon's one loop to do, and the handle that carries it. A
//! handler works out who called (the connection's, never the body's), builds a command and waits
//! for the loop's answer; every state change happens in the loop.

use crate::config::VoiceRole;
use crate::error::VoiceError;
use crate::peer::Peers;
use std::os::fd::OwnedFd;
use tokio::sync::{mpsc, oneshot};
use voice_wire::{CancelCause, SpeakWire, VoiceBegin, VoiceTarget};
use zbus::message::Header;

/// Who called: the connection's unique name and the role the daemon derived for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Caller {
    pub unique: String,
    pub role: Option<VoiceRole>,
}

impl Caller {
    pub fn is_shell(&self) -> bool {
        self.role == Some(VoiceRole::Shell)
    }
}

pub(crate) type Reply<T> = oneshot::Sender<T>;
pub(crate) type Done = Reply<Result<(), VoiceError>>;

/// One request to the loop.
#[derive(Debug)]
pub(crate) enum Command {
    Begin {
        caller: Caller,
        begin: VoiceBegin,
        reply: Reply<Result<(String, OwnedFd), VoiceError>>,
    },
    Speak {
        caller: Caller,
        wire: SpeakWire,
        reply: Reply<Result<String, VoiceError>>,
    },
    Hush {
        caller: Caller,
        reply: Done,
    },
    Prepare {
        caller: Caller,
        reply: Reply<Result<String, VoiceError>>,
    },
    Status {
        reply: Reply<String>,
    },
    Route {
        n: u64,
        caller: Caller,
        target: VoiceTarget,
        reply: Done,
    },
    Attach {
        n: u64,
        caller: Caller,
        reply: Reply<Result<OwnedFd, VoiceError>>,
    },
    Release {
        n: u64,
        caller: Caller,
        reply: Done,
    },
    Cancel {
        n: u64,
        caller: Caller,
        cause: CancelCause,
        reply: Done,
    },
    StopSpeech {
        n: u64,
        caller: Caller,
        reply: Done,
    },
}

/// The handlers' end: the loop's inbox and the means to identify a caller.
#[derive(Debug, Clone)]
pub struct Handle {
    tx: mpsc::Sender<Command>,
    peers: Peers,
}

impl Handle {
    pub(crate) fn new(tx: mpsc::Sender<Command>, peers: Peers) -> Self {
        Self { tx, peers }
    }

    /// The caller behind a message header.
    pub(crate) async fn caller(&self, header: &Header<'_>) -> Caller {
        let unique = header.sender().map(ToString::to_string).unwrap_or_default();
        let role = self.peers.role(&unique).await;
        Caller { unique, role }
    }

    /// Sends the command `make` builds and waits for its answer; the daemon going away is
    /// `Gone`.
    pub(crate) async fn ask<T>(
        &self,
        make: impl FnOnce(Reply<T>) -> Command,
    ) -> Result<T, VoiceError> {
        let (reply, answer) = oneshot::channel();
        let gone = || VoiceError::Gone("voiced is stopping".to_owned());
        self.tx.send(make(reply)).await.map_err(|_| gone())?;
        answer.await.map_err(|_| gone())
    }
}
