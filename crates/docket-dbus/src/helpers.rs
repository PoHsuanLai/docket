//! `org.quire.Companion1.Helpers`: the helpers the person keeps, on the same object as
//! `org.quire.Companion1`. Only the shell speaks for the person here: it lists the helpers, reads
//! a card, makes, changes, pauses, resumes and removes one, and gives one a job. Every body is
//! JSON of `companion-wire`'s helper types, and every sentence in them is plain words.

use zbus::fdo;
use zbus::object_server::SignalEmitter;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Companion1.Helpers",
    default_service = "org.quire.Companion1",
    default_path = "/org/quire/Companion1"
)]
pub trait CompanionHelpers {
    /// The helpers (`HelpersView` JSON): one line each, and what they told the person after the
    /// sentence numbered `after` (0 for all that are kept).
    fn list(&self, after: u64) -> zbus::Result<String>;

    /// One helper's card (`HelperCardView` JSON; an error for a helper that is not there).
    fn card(&self, helper: &str) -> zbus::Result<String>;

    /// Makes a helper from what the person confirmed (`HelperForm` JSON; answers `HelperReply`
    /// JSON). It has no model until the person chooses one.
    fn create(&self, form: &str) -> zbus::Result<String>;

    /// Changes a helper to what the person confirmed (`HelperForm` JSON; answers `HelperReply`
    /// JSON).
    fn edit(&self, helper: &str, form: &str) -> zbus::Result<String>;

    /// The person pauses a helper: its running job stops, and it takes none until resumed.
    fn pause(&self, helper: &str) -> zbus::Result<()>;

    /// The person resumes a helper that is paused, or asks one whose job did not finish to try
    /// again.
    fn resume(&self, helper: &str) -> zbus::Result<()>;

    /// The person removes a helper and its jobs.
    fn remove(&self, helper: &str) -> zbus::Result<()>;

    /// The person gives a helper a job in their own words (`things`: the things only that job may
    /// change, `EntityId` JSON list; answers `HelperReply` JSON).
    fn post_job(&self, helper: &str, goal: &str, things: &str) -> zbus::Result<String>;

    /// Something about the helpers changed. Content-free: read `List`.
    #[zbus(signal)]
    fn changed(&self) -> zbus::Result<()>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct CompanionHelpersSkeleton;

#[zbus::interface(name = "org.quire.Companion1.Helpers")]
impl CompanionHelpersSkeleton {
    fn list(&self, after: u64) -> fdo::Result<String> {
        let _ = (after,);
        Err(crate::introspect::frozen())
    }

    fn card(&self, helper: String) -> fdo::Result<String> {
        let _ = (helper,);
        Err(crate::introspect::frozen())
    }

    fn create(&self, form: String) -> fdo::Result<String> {
        let _ = (form,);
        Err(crate::introspect::frozen())
    }

    fn edit(&self, helper: String, form: String) -> fdo::Result<String> {
        let _ = (helper, form);
        Err(crate::introspect::frozen())
    }

    fn pause(&self, helper: String) -> fdo::Result<()> {
        let _ = (helper,);
        Err(crate::introspect::frozen())
    }

    fn resume(&self, helper: String) -> fdo::Result<()> {
        let _ = (helper,);
        Err(crate::introspect::frozen())
    }

    fn remove(&self, helper: String) -> fdo::Result<()> {
        let _ = (helper,);
        Err(crate::introspect::frozen())
    }

    fn post_job(&self, helper: String, goal: String, things: String) -> fdo::Result<String> {
        let _ = (helper, goal, things);
        Err(crate::introspect::frozen())
    }

    #[zbus(signal)]
    async fn changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}
