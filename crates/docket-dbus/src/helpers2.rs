//! `org.quire.Companion1.Helpers2`: everything `Helpers` has, and the app helpers' sign-in and the
//! routines. It is served beside `Helpers`, which stays as it was; a caller that knows this one
//! uses it alone. Only the shell speaks for the person here. Every body is JSON of
//! `companion-wire`'s helper and routine types, and every sentence in them is plain words.

use zbus::fdo;
use zbus::object_server::SignalEmitter;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Companion1.Helpers2",
    default_service = "org.quire.Companion1",
    default_path = "/org/quire/Companion1"
)]
pub trait CompanionHelpers2 {
    /// The helpers (`HelpersView` JSON): one line each, and what they told the person after the
    /// sentence numbered `after` (0 for all that are kept).
    fn list(&self, after: u64) -> zbus::Result<String>;

    /// One helper's card (`HelperCardView` JSON; an error for a helper that is not there).
    fn card(&self, helper: &str) -> zbus::Result<String>;

    /// Makes a helper from what the person confirmed (`HelperForm` JSON; answers `HelperReply`
    /// JSON).
    fn create(&self, form: &str) -> zbus::Result<String>;

    /// Changes a helper to what the person confirmed (`HelperForm` JSON; answers `HelperReply`
    /// JSON).
    fn edit(&self, helper: &str, form: &str) -> zbus::Result<String>;

    /// The person pauses a helper.
    fn pause(&self, helper: &str) -> zbus::Result<()>;

    /// The person resumes a helper that is paused, or asks one whose job did not finish to try
    /// again.
    fn resume(&self, helper: &str) -> zbus::Result<()>;

    /// The person removes a helper and its jobs.
    fn remove(&self, helper: &str) -> zbus::Result<()>;

    /// The person gives a helper a job in their own words (`things`: the things only that job may
    /// change, `EntityId` JSON list; answers `HelperReply` JSON).
    fn post_job(&self, helper: &str, goal: &str, things: &str) -> zbus::Result<String>;

    /// The person chose one of the ways of signing in to the app the helper runs (`way` is the id
    /// of a sign-in choice on its card; answers `HelperReply` JSON: done, or the window the shell
    /// is to open).
    fn sign_in(&self, helper: &str, way: &str) -> zbus::Result<String>;

    /// The person signs out of the app the helper runs (answers `HelperReply` JSON).
    fn sign_out(&self, helper: &str) -> zbus::Result<String>;

    /// The routines (`RoutinesView` JSON).
    fn routines(&self) -> zbus::Result<String>;

    /// One routine with its recent runs (`RoutineCardView` JSON; an error for one that is not
    /// there).
    fn routine_card(&self, routine: &str) -> zbus::Result<String>;

    /// Makes a routine from what the person confirmed (`RoutineForm` JSON; answers
    /// `RoutineReply` JSON).
    fn add_routine(&self, form: &str) -> zbus::Result<String>;

    /// The person pauses a routine.
    fn pause_routine(&self, routine: &str) -> zbus::Result<()>;

    /// The person resumes a routine.
    fn resume_routine(&self, routine: &str) -> zbus::Result<()>;

    /// The person removes a routine.
    fn remove_routine(&self, routine: &str) -> zbus::Result<()>;

    /// Something about the helpers or their routines changed. Content-free: read `List`.
    #[zbus(signal)]
    fn changed(&self) -> zbus::Result<()>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct CompanionHelpers2Skeleton;

#[zbus::interface(name = "org.quire.Companion1.Helpers2")]
impl CompanionHelpers2Skeleton {
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

    fn sign_in(&self, helper: String, way: String) -> fdo::Result<String> {
        let _ = (helper, way);
        Err(crate::introspect::frozen())
    }

    fn sign_out(&self, helper: String) -> fdo::Result<String> {
        let _ = (helper,);
        Err(crate::introspect::frozen())
    }

    fn routines(&self) -> fdo::Result<String> {
        Err(crate::introspect::frozen())
    }

    fn routine_card(&self, routine: String) -> fdo::Result<String> {
        let _ = (routine,);
        Err(crate::introspect::frozen())
    }

    fn add_routine(&self, form: String) -> fdo::Result<String> {
        let _ = (form,);
        Err(crate::introspect::frozen())
    }

    fn pause_routine(&self, routine: String) -> fdo::Result<()> {
        let _ = (routine,);
        Err(crate::introspect::frozen())
    }

    fn resume_routine(&self, routine: String) -> fdo::Result<()> {
        let _ = (routine,);
        Err(crate::introspect::frozen())
    }

    fn remove_routine(&self, routine: String) -> fdo::Result<()> {
        let _ = (routine,);
        Err(crate::introspect::frozen())
    }

    #[zbus(signal)]
    async fn changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}
