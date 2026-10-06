//! `org.quire.Voice1` and its two object interfaces, as proxies for callers and skeletons for
//! the daemon (voice.md §3.5). Bodies are JSON in `s` (`voice-wire` types in an `Envelope`); the
//! event stream is an fd of `VoiceEvent` frames. Caller identity comes from the connection,
//! never an argument. A skeleton made with `Default` answers `NotSupported`: that is the one the
//! introspection test reads. The serving ones hold the daemon's handle. Members carry no doc
//! comments: zbus copies them into the introspection, which is held to `dbus/org.quire.Voice1.xml`.

use crate::command::{Command, Handle};
use crate::error::VoiceError;
use crate::introspect::frozen;
use voice_wire::{SpeakWire, VoiceBegin, VoiceTarget};
use zbus::fdo;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedFd, OwnedObjectPath};

/// The caller's side of `org.quire.Voice1`.
#[zbus::proxy(
    interface = "org.quire.Voice1",
    default_service = "org.quire.Voice1",
    default_path = "/org/quire/Voice1"
)]
pub trait Voice {
    /// Opens the mic now (shell only): `VoiceBegin` in, the utterance object and an event fd out.
    fn begin(&self, begin: &str) -> zbus::Result<(OwnedObjectPath, OwnedFd)>;
    /// Speaks (shell, or the app attached to `speak.utterance`): `SpeakWire` in.
    fn speak(&self, speak: &str) -> zbus::Result<OwnedObjectPath>;
    /// Stops all speech now (shell).
    fn hush(&self) -> zbus::Result<()>;
    /// Warms the speech engine; answers a `Readiness` slug (shell).
    fn prepare(&self) -> zbus::Result<String>;
    /// `VoiceStatus`, no content.
    fn status(&self) -> zbus::Result<String>;
    /// The mic opened or closed, or speech started or stopped; content-free, broadcast.
    #[zbus(signal)]
    fn status_changed(&self, status: &str) -> zbus::Result<()>;
}

/// The daemon's side of `org.quire.Voice1`.
#[derive(Debug, Default)]
pub struct VoiceSkeleton {
    serving: Option<Handle>,
}

impl VoiceSkeleton {
    pub(crate) fn serving(handle: Handle) -> Self {
        Self {
            serving: Some(handle),
        }
    }

    fn handle(&self) -> fdo::Result<&Handle> {
        self.serving.as_ref().ok_or_else(frozen)
    }
}

#[zbus::interface(name = "org.quire.Voice1")]
impl VoiceSkeleton {
    async fn begin(
        &self,
        begin: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(OwnedObjectPath, OwnedFd), VoiceError> {
        let handle = self.handle()?;
        let begin: VoiceBegin = crate::wire::open(&begin)?;
        let caller = handle.caller(&header).await;
        let (path, fd) = handle
            .ask(|reply| Command::Begin {
                caller,
                begin,
                reply,
            })
            .await??;
        Ok((crate::wire::path(&path)?, fd.into()))
    }

    async fn speak(
        &self,
        speak: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<OwnedObjectPath, VoiceError> {
        let handle = self.handle()?;
        let wire: SpeakWire = crate::wire::open(&speak)?;
        let caller = handle.caller(&header).await;
        let path = handle
            .ask(|reply| Command::Speak {
                caller,
                wire,
                reply,
            })
            .await??;
        crate::wire::path(&path)
    }

    async fn hush(&self, #[zbus(header)] header: Header<'_>) -> Result<(), VoiceError> {
        let handle = self.handle()?;
        let caller = handle.caller(&header).await;
        handle.ask(|reply| Command::Hush { caller, reply }).await?
    }

    async fn prepare(&self, #[zbus(header)] header: Header<'_>) -> Result<String, VoiceError> {
        let handle = self.handle()?;
        let caller = handle.caller(&header).await;
        handle
            .ask(|reply| Command::Prepare { caller, reply })
            .await?
    }

    async fn status(&self) -> Result<String, VoiceError> {
        let handle = self.handle()?;
        handle.ask(|reply| Command::Status { reply }).await
    }

    #[zbus(signal)]
    pub async fn status_changed(emitter: &SignalEmitter<'_>, status: &str) -> zbus::Result<()>;
}

/// The caller's side of one utterance object (`/org/quire/Voice1/utterance/<n>`).
#[zbus::proxy(
    interface = "org.quire.Voice1.Utterance",
    default_service = "org.quire.Voice1"
)]
pub trait Utterance {
    /// Names the one app that may attach (`VoiceTarget`); the Begin caller only.
    fn route(&self, target: &str) -> zbus::Result<()>;
    /// A second event fd, starting with the committed segments and the last partial; the routed
    /// app only, at most one.
    fn attach(&self) -> zbus::Result<OwnedFd>;
    /// End of the hold: finish (the Begin caller).
    fn release(&self) -> zbus::Result<()>;
    /// Discards everything (the Begin caller or the attached app). `cause` is a sealed
    /// `CancelCause`: `escape`, `other_input`, `focus_lost` or `shell`.
    fn cancel(&self, cause: &str) -> zbus::Result<()>;
    /// `UtteranceEnd` without text, unicast to the Begin caller and the attached app.
    #[zbus(signal)]
    fn ended(&self, end: &str) -> zbus::Result<()>;
}

/// The daemon's side of one utterance object.
#[derive(Debug, Default)]
pub struct UtteranceSkeleton {
    serving: Option<(u64, Handle)>,
}

impl UtteranceSkeleton {
    pub(crate) fn serving(n: u64, handle: Handle) -> Self {
        Self {
            serving: Some((n, handle)),
        }
    }

    fn handle(&self) -> fdo::Result<&(u64, Handle)> {
        self.serving.as_ref().ok_or_else(frozen)
    }
}

#[zbus::interface(name = "org.quire.Voice1.Utterance")]
impl UtteranceSkeleton {
    async fn route(
        &self,
        target: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), VoiceError> {
        let (n, handle) = self.handle()?;
        let target: VoiceTarget = crate::wire::open(&target)?;
        let (n, caller) = (*n, handle.caller(&header).await);
        handle
            .ask(|reply| Command::Route {
                n,
                caller,
                target,
                reply,
            })
            .await?
    }

    async fn attach(&self, #[zbus(header)] header: Header<'_>) -> Result<OwnedFd, VoiceError> {
        let (n, handle) = self.handle()?;
        let (n, caller) = (*n, handle.caller(&header).await);
        let fd = handle
            .ask(|reply| Command::Attach { n, caller, reply })
            .await??;
        Ok(fd.into())
    }

    async fn release(&self, #[zbus(header)] header: Header<'_>) -> Result<(), VoiceError> {
        let (n, handle) = self.handle()?;
        let (n, caller) = (*n, handle.caller(&header).await);
        handle
            .ask(|reply| Command::Release { n, caller, reply })
            .await?
    }

    async fn cancel(
        &self,
        cause: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), VoiceError> {
        let (n, handle) = self.handle()?;
        let cause = crate::wire::caller_cause(&cause)?;
        let (n, caller) = (*n, handle.caller(&header).await);
        handle
            .ask(|reply| Command::Cancel {
                n,
                caller,
                cause,
                reply,
            })
            .await?
    }

    #[zbus(signal)]
    pub async fn ended(emitter: &SignalEmitter<'_>, end: &str) -> zbus::Result<()>;
}

/// The caller's side of one speech object (`/org/quire/Voice1/speech/<n>`).
#[zbus::proxy(
    interface = "org.quire.Voice1.Speech",
    default_service = "org.quire.Voice1"
)]
pub trait Speech {
    /// Stops this speech (the requester or the shell).
    fn stop(&self) -> zbus::Result<()>;
    /// `SpeechEnd`, unicast to the requester.
    #[zbus(signal)]
    fn finished(&self, end: &str) -> zbus::Result<()>;
}

/// The daemon's side of one speech object.
#[derive(Debug, Default)]
pub struct SpeechSkeleton {
    serving: Option<(u64, Handle)>,
}

impl SpeechSkeleton {
    pub(crate) fn serving(n: u64, handle: Handle) -> Self {
        Self {
            serving: Some((n, handle)),
        }
    }

    fn handle(&self) -> fdo::Result<&(u64, Handle)> {
        self.serving.as_ref().ok_or_else(frozen)
    }
}

#[zbus::interface(name = "org.quire.Voice1.Speech")]
impl SpeechSkeleton {
    async fn stop(&self, #[zbus(header)] header: Header<'_>) -> Result<(), VoiceError> {
        let (n, handle) = self.handle()?;
        let (n, caller) = (*n, handle.caller(&header).await);
        handle
            .ask(|reply| Command::StopSpeech { n, caller, reply })
            .await?
    }

    #[zbus(signal)]
    pub async fn finished(emitter: &SignalEmitter<'_>, end: &str) -> zbus::Result<()>;
}
