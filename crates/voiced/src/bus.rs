//! `org.quire.Voice1` and its two object interfaces, as proxies for callers and skeletons for
//! the daemon (voice.md §3.5). Bodies are JSON in `s` (`voice-wire` types); the event stream is
//! an fd of `VoiceEvent` frames. Caller identity comes from the connection, never an argument.
//! Every skeleton method answers `NotSupported`.

use crate::introspect::frozen;
use zbus::fdo;
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
pub struct VoiceSkeleton;

#[zbus::interface(name = "org.quire.Voice1")]
impl VoiceSkeleton {
    fn begin(&self, begin: String) -> fdo::Result<(OwnedObjectPath, OwnedFd)> {
        let _ = begin;
        Err(frozen())
    }

    fn speak(&self, speak: String) -> fdo::Result<OwnedObjectPath> {
        let _ = speak;
        Err(frozen())
    }

    fn hush(&self) -> fdo::Result<()> {
        Err(frozen())
    }

    fn prepare(&self) -> fdo::Result<String> {
        Err(frozen())
    }

    fn status(&self) -> fdo::Result<String> {
        Err(frozen())
    }

    #[zbus(signal)]
    async fn status_changed(emitter: &SignalEmitter<'_>, status: &str) -> zbus::Result<()>;
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
    /// Discards everything (the Begin caller or the attached app).
    fn cancel(&self) -> zbus::Result<()>;
    /// `UtteranceEnd` without text, unicast to the Begin caller and the attached app.
    #[zbus(signal)]
    fn ended(&self, end: &str) -> zbus::Result<()>;
}

/// The daemon's side of one utterance object.
#[derive(Debug, Default)]
pub struct UtteranceSkeleton;

#[zbus::interface(name = "org.quire.Voice1.Utterance")]
impl UtteranceSkeleton {
    fn route(&self, target: String) -> fdo::Result<()> {
        let _ = target;
        Err(frozen())
    }

    fn attach(&self) -> fdo::Result<OwnedFd> {
        Err(frozen())
    }

    fn release(&self) -> fdo::Result<()> {
        Err(frozen())
    }

    fn cancel(&self) -> fdo::Result<()> {
        Err(frozen())
    }

    #[zbus(signal)]
    async fn ended(emitter: &SignalEmitter<'_>, end: &str) -> zbus::Result<()>;
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
pub struct SpeechSkeleton;

#[zbus::interface(name = "org.quire.Voice1.Speech")]
impl SpeechSkeleton {
    fn stop(&self) -> fdo::Result<()> {
        Err(frozen())
    }

    #[zbus(signal)]
    async fn finished(emitter: &SignalEmitter<'_>, end: &str) -> zbus::Result<()>;
}
