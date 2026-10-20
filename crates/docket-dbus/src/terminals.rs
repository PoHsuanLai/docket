//! `org.quire.Companion1.Terminals`: the conversations with an agent app that a terminal started,
//! which the person can bring into docket. Only the shell reads or acts on it. Every body is JSON
//! of `companion-wire`'s handoff types, and every sentence in them is plain words.

use zbus::fdo;
use zbus::object_server::SignalEmitter;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Companion1.Terminals",
    default_service = "org.quire.Companion1",
    default_path = "/org/quire/Companion1"
)]
pub trait CompanionTerminals {
    /// The terminals' conversations that can be brought in (`TerminalSession` JSON list).
    fn sessions(&self) -> zbus::Result<String>;

    /// Brings one into docket (`session` is the id a `TerminalSession` carries; answers
    /// `BroughtBack` JSON).
    fn bring_back(&self, session: &str) -> zbus::Result<String>;

    /// Which listed app a command line a terminal saw in a pane is picking a conversation up
    /// of (`seen` is `SeenCommand` JSON, the words of the line; answers the `WatchedAgent` JSON,
    /// or `null` when it is no listed app). Read-only: a terminal may ask it, and nothing is
    /// stored. The line can hold secrets, so it is matched and dropped.
    fn recognise(&self, seen: &str) -> zbus::Result<String>;

    /// Something about the terminals' conversations changed. Content-free: read `Sessions`.
    #[zbus(signal)]
    fn changed(&self) -> zbus::Result<()>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct CompanionTerminalsSkeleton;

#[zbus::interface(name = "org.quire.Companion1.Terminals")]
impl CompanionTerminalsSkeleton {
    fn sessions(&self) -> fdo::Result<String> {
        Err(crate::introspect::frozen())
    }

    fn bring_back(&self, session: String) -> fdo::Result<String> {
        let _ = (session,);
        Err(crate::introspect::frozen())
    }

    fn recognise(&self, seen: String) -> fdo::Result<String> {
        let _ = (seen,);
        Err(crate::introspect::frozen())
    }

    #[zbus(signal)]
    async fn changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}
