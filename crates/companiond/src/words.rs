//! The sentences the person is told when a desk says no. The desks decide with
//! `companion_core::Refusal`; this edge makes the words, one place for the helpers' desk and the
//! routines' desk, so the wire's plain-sentence refusals read as they always have.

use companion_core::Refusal;
use companion_wire::HelperReply;
use docket_core::helper::ReachFault;

fn reach_sentence(fault: &ReachFault) -> String {
    match fault {
        ReachFault::NoSuchApp(app) => format!("There is no app called {app}."),
        _ => "Choose at least one app for it to use.".to_owned(),
    }
}

/// `refusal` in plain words.
pub(crate) fn sentence(refusal: &Refusal) -> String {
    match refusal {
        Refusal::Job(why) => why.plain(),
        Refusal::Hand(why) => why.plain().to_owned(),
        Refusal::Reach(fault) => reach_sentence(fault),
        Refusal::Schedule(fault) => fault.to_string(),
        Refusal::NameTaken(name) => {
            format!("There is already a helper called {}.", name.as_str())
        }
        Refusal::NoGoal => "Say what it is to do, in a few sentences.".to_owned(),
        Refusal::NothingToHandOn(name) => format!("{name} has nothing finished to hand on."),
        Refusal::JobNotGiven => "The job could not be given.".to_owned(),
        Refusal::NeedWhy => "Say why in one short sentence.".to_owned(),
        Refusal::TooManyWaiting => {
            "Too many routines are waiting for an answer. Answer some first.".to_owned()
        }
        Refusal::BadNameOrRole => "The name or what it is for is empty or too long.".to_owned(),
        Refusal::HelperNotMade => "The helper could not be made.".to_owned(),
        Refusal::NotAConversation => "That is not a conversation with a helper.".to_owned(),
        Refusal::RunsItsOwnApp(name) => {
            format!(
                "{} runs its own app, so you talk with it there.",
                name.as_str()
            )
        }
        Refusal::SayFailed { name, fault, why } => why
            .clone()
            .unwrap_or_else(|| format!("{} stopped: {}.", name.as_str(), fault.plain())),
        Refusal::SayStopped(name) => format!("{} was stopped.", name.as_str()),
        Refusal::SayNeedsSignIn(name) => {
            format!("{} needs you to sign in first.", name.as_str())
        }
        Refusal::NotAnApp => "This helper does not run an app.".to_owned(),
        Refusal::NotAnAppToSignInTo => "This helper does not run an app to sign in to.".to_owned(),
        Refusal::NotAnAppToSignOutOf => {
            "This helper does not run an app to sign out of.".to_owned()
        }
        Refusal::NoAppHost => "No apps can be reached here.".to_owned(),
        Refusal::NoAppHostToSignInTo => "No apps can be signed in to here.".to_owned(),
        Refusal::AlreadyInTerminal => "It is already in the terminal.".to_owned(),
        Refusal::NotAWayToSignIn => "That is not one of the ways to sign in.".to_owned(),
        Refusal::SignInDeclined => {
            "It did not sign you in. Try again, or choose another way.".to_owned()
        }
        Refusal::SignInNotStarted => "It could not be started.".to_owned(),
        Refusal::SignOutFailed => "Signing out did not work.".to_owned(),
        Refusal::UnknownHelper => "There is no such helper.".to_owned(),
        Refusal::WatchKind => "Say which kind of thing to watch for.".to_owned(),
        Refusal::WatchWords => "Say what to watch for, in a few words.".to_owned(),
        Refusal::RoutineNotMade => "The routine could not be made.".to_owned(),
    }
}

/// The helper reply that says no, in plain words.
pub(crate) fn refused(refusal: Refusal) -> HelperReply {
    HelperReply::Refused(sentence(&refusal))
}
