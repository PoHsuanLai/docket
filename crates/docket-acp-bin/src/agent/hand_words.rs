//! What the host says to the person who handed a turn over: why a setup, a session or a turn did
//! not give an answer, and the answer when it did.

use super::fault::AgentFault;
use super::host::HostError;
use docket_core::{HandRefusal, Handed};
use docket_session::{BackendFault, HostFault, TurnEnd};

/// What a setup fault is to the person who asked.
pub fn refusal_of_fault(fault: &AgentFault) -> HandRefusal {
    match fault {
        AgentFault::Permit(_) => HandRefusal::TurnedOff,
        AgentFault::NoAgentsFile => HandRefusal::NotSetUp,
        AgentFault::Agents(_) => HandRefusal::Failed {
            message: "The list of coding agents can't be read. Open Settings to check it."
                .to_owned(),
        },
        AgentFault::NoSandbox(_) => HandRefusal::Failed {
            message: "This computer has no sandbox, so no coding agent is started.".to_owned(),
        },
        _ => HandRefusal::Failed {
            message: "The coding agent could not be started.".to_owned(),
        },
    }
}

/// What a session that did not open is to the person who asked.
pub fn refusal_of_open(error: HostError, label: &str) -> HandRefusal {
    match error {
        HostError::Open(HostFault::Backend(
            BackendFault::SignInNeeded
            | BackendFault::SignInUnsupported
            | BackendFault::SignInChoose(_),
        )) => HandRefusal::NeedsSignIn {
            label: label.to_owned(),
        },
        HostError::Open(HostFault::Backend(
            BackendFault::ModelNotOffered(_)
            | BackendFault::ModelNotChoosable
            | BackendFault::ModelRefused,
        )) => HandRefusal::Failed {
            message: format!(
                "The model chosen for {label} is not available. Choose another in Settings."
            ),
        },
        HostError::Setup(words) => HandRefusal::Failed { message: words },
        HostError::Open(_) => HandRefusal::Failed {
            message: format!("{label} could not be started."),
        },
    }
}

/// What the turn's end is to the person who asked: the agent's words, or why there are none.
pub fn end_of(end: TurnEnd, said: &[String], label: &str) -> Result<Handed, HandRefusal> {
    let failed = |message: String| Err(HandRefusal::Failed { message });
    match end {
        TurnEnd::Done => {
            let words = said.concat();
            let words = match words.trim() {
                "" => format!("{label} finished without a written answer."),
                text => text.to_owned(),
            };
            Ok(Handed {
                words,
                label: label.to_owned(),
            })
        }
        TurnEnd::Cancelled => failed(format!("{label} was stopped.")),
        TurnEnd::Paused(_) => failed(format!("{label} paused and is waiting for you.")),
        TurnEnd::Failed | TurnEnd::Refused => failed(format!("{label} could not finish.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setting_that_is_off_and_a_missing_list_are_said_as_what_they_are() {
        let rows = [
            (AgentFault::Permit("off".into()), HandRefusal::TurnedOff),
            (AgentFault::NoAgentsFile, HandRefusal::NotSetUp),
        ];
        for (fault, expected) in rows {
            assert_eq!(refusal_of_fault(&fault), expected, "{fault:?}");
        }
        let none = AgentFault::NoSandbox(bulkhead::CannotSandbox::NotInstalled);
        let said = refusal_of_fault(&none).sentence();
        assert!(!said.contains("bubblewrap"), "{said}");
    }

    #[test]
    fn a_missing_sign_in_names_the_agent_and_other_faults_name_no_machinery() {
        let sign_in = HostError::Open(HostFault::Backend(BackendFault::SignInNeeded));
        assert_eq!(
            refusal_of_open(sign_in, "Claude Code"),
            HandRefusal::NeedsSignIn {
                label: "Claude Code".to_owned()
            }
        );
        let down = HostError::Open(HostFault::Backend(BackendFault::Unavailable));
        let said = refusal_of_open(down, "Claude Code").sentence();
        assert_eq!(said, "Claude Code could not be started.");
    }

    #[test]
    fn a_finished_turn_is_the_agents_words_and_the_others_are_why_not() {
        let said = ["Hello".to_owned(), ", there.".to_owned()];
        let handed = end_of(TurnEnd::Done, &said, "Claude Code").expect("handed");
        assert_eq!(handed.words, "Hello, there.");
        assert_eq!(handed.label, "Claude Code");
        let quiet = end_of(TurnEnd::Done, &[], "Claude Code").expect("handed");
        assert_eq!(
            quiet.words,
            "Claude Code finished without a written answer."
        );
        for end in [TurnEnd::Failed, TurnEnd::Refused, TurnEnd::Cancelled] {
            assert!(end_of(end, &said, "Claude Code").is_err(), "{end:?}");
        }
    }
}
