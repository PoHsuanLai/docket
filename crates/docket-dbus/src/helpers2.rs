//! `org.quire.Companion1.Helpers2`: everything `Helpers` has, and the app helpers' sign-in and the
//! routines, and speaking in a helper's conversation. It is served beside `Helpers`, which stays
//! as it was; a caller that knows this one uses it alone. Only the shell speaks for the person
//! here, except that our own editor bridge may read the helpers and speak in a helper's
//! conversation (`Say`). Every body is JSON of `companion-wire`'s helper and routine types, and
//! every sentence in them is plain words.

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

    /// The routines the assistant or a helper proposed that wait for the person
    /// (`RoutineProposal` JSON list). Kept in memory only.
    fn routine_proposals(&self) -> zbus::Result<String>;

    /// The person accepts a proposal (`p-3`) on the sheet: the routine is made, skipping a time
    /// that is missed (answers `RoutineReply` JSON).
    fn accept_routine_proposal(&self, id: &str) -> zbus::Result<String>;

    /// The person declines a proposal (`p-3`): it is gone and nothing is made.
    fn decline_routine_proposal(&self, id: &str) -> zbus::Result<()>;

    /// The person opens the helper's conversation in the app's own program in a terminal
    /// (answers `HelperReply` JSON: the terminal window the shell is to open, or why not).
    fn open_in_terminal(&self, helper: &str) -> zbus::Result<String>;

    /// The person brings the helper's conversation back from the terminal (answers `HelperReply`
    /// JSON: done, or why not).
    fn bring_back(&self, helper: &str) -> zbus::Result<String>;

    /// The person speaks in a helper's conversation (`session` is the conversation's session,
    /// `turn` the number the router gave the recorded turn, `text` what was said). The helper takes
    /// the turn under its own model and reach; the answer is `SayReply` JSON: its words when the
    /// turn ends, or why it could not. The shell and our own editor bridge may; any session that
    /// is not a helper's conversation is refused.
    fn say(&self, session: &str, turn: u64, text: &str) -> zbus::Result<String>;

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

    fn routine_proposals(&self) -> fdo::Result<String> {
        Err(crate::introspect::frozen())
    }

    fn accept_routine_proposal(&self, id: String) -> fdo::Result<String> {
        let _ = (id,);
        Err(crate::introspect::frozen())
    }

    fn decline_routine_proposal(&self, id: String) -> fdo::Result<()> {
        let _ = (id,);
        Err(crate::introspect::frozen())
    }

    fn open_in_terminal(&self, helper: String) -> fdo::Result<String> {
        let _ = (helper,);
        Err(crate::introspect::frozen())
    }

    fn bring_back(&self, helper: String) -> fdo::Result<String> {
        let _ = (helper,);
        Err(crate::introspect::frozen())
    }

    fn say(&self, session: String, turn: u64, text: String) -> fdo::Result<String> {
        let _ = (session, turn, text);
        Err(crate::introspect::frozen())
    }

    #[zbus(signal)]
    async fn changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}
