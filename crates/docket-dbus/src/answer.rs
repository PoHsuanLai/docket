//! `org.quire.Companion1.Answer`: one answer, at `/org/quire/Companion1/answer/<task>`.

use zbus::fdo;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedObjectPath;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Companion1.Answer",
    default_service = "org.quire.Companion1"
)]
pub trait CompanionAnswer {
    /// Takes a card action (`CardActionId` JSON); becomes a `Run.Perform`. Answers a Request.
    fn act(&self, action: &str) -> zbus::Result<OwnedObjectPath>;

    /// Cancels the task.
    fn cancel(&self) -> zbus::Result<()>;

    /// The answer changed (`AnswerWire` JSON).
    #[zbus(signal)]
    fn updated(&self, view: &str) -> zbus::Result<()>;

    /// The current view (`AnswerWire` JSON).
    #[zbus(property)]
    fn view(&self) -> zbus::Result<String>;

    /// The router session (`SessionId` text) this answer's handles are shown through
    /// (`Intents1.Session.Display`). It stays open until the answer is dismissed with
    /// `Companion1.Close`; after that the object is gone.
    #[zbus(property)]
    fn session(&self) -> zbus::Result<String>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct CompanionAnswerSkeleton;

#[zbus::interface(name = "org.quire.Companion1.Answer")]
impl CompanionAnswerSkeleton {
    fn act(&self, action: String) -> fdo::Result<OwnedObjectPath> {
        let _ = (action,);
        Err(crate::introspect::frozen())
    }

    fn cancel(&self) -> fdo::Result<()> {
        Err(crate::introspect::frozen())
    }

    #[zbus(signal)]
    async fn updated(emitter: &SignalEmitter<'_>, view: &str) -> zbus::Result<()>;

    #[zbus(property)]
    fn session(&self) -> fdo::Result<String> {
        Err(crate::introspect::frozen())
    }

    #[zbus(property)]
    fn view(&self) -> fdo::Result<String> {
        Err(crate::introspect::frozen())
    }
}
