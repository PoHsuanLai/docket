//! One past step as the planner reads it. A refusal is told as a short reason the model can act
//! on (which argument, why, and which handles it does hold), never a policy id or a reviewer's
//! words: the router's coarse code is all that crosses, and a handle stays an opaque `#n`.

use docket_core::{ArgFault, CallRefusal, HandleCard, Held, Reveal, StepEnd, StepLine, StepShown};

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// The handles the planner may name, as `#3 #4`, or that it holds none.
fn held_handles(handles: &[HandleCard]) -> String {
    if handles.is_empty() {
        return "you hold no handles yet".to_owned();
    }
    let list: Vec<String> = handles.iter().map(|h| format!("#{}", h.handle.0)).collect();
    format!("handles you hold: {}", list.join(" "))
}

/// What to change after a refusal for an argument.
fn arg_hint(param: &str, why: ArgFault, handles: &[HandleCard]) -> String {
    match why {
        ArgFault::Missing => format!("argument \"{param}\" is required and was missing"),
        ArgFault::WrongType => format!("argument \"{param}\" has the wrong type"),
        ArgFault::OutOfRange => format!("argument \"{param}\" is out of its range"),
        ArgFault::UnknownHandle => format!(
            "argument \"{param}\" names a handle that does not exist; {}",
            held_handles(handles)
        ),
        ArgFault::CrossSpace => {
            format!("argument \"{param}\" is from another Space; use only what this Space gave you")
        }
    }
}

/// The reason after a refusal, where there is something to act on.
fn refusal_hint(refusal: &CallRefusal, handles: &[HandleCard]) -> Option<String> {
    match refusal {
        CallRefusal::BadArgs { param, why } => Some(arg_hint(param.as_str(), *why, handles)),
        CallRefusal::NoSuchAction(_) => {
            Some("there is no such action; call only the tools you were given".to_owned())
        }
        _ => None,
    }
}

/// Why a repeated call was not run, and what to do instead.
fn held_text(held: Held) -> &'static str {
    match held {
        Held::Empty => "you already called it with these arguments and got nothing",
        Held::Unchanged => "you already called it with these arguments and got the same answer",
        Held::Refused => "you already called it with these arguments and it was refused",
    }
}

const INSTEAD: &str =
    "change the arguments, try another action, ask the person with quire_ask, or finish";

/// One step, as one line of the planner's history. `handles` are the ones the planner holds.
pub(crate) fn step_line(step: &StepLine, handles: &[HandleCard]) -> String {
    let action = format!("{}.{}", step.action.app, step.action.name);
    match (&step.shown, &step.end) {
        (StepShown::Masked, StepEnd::Done { said, undo, .. }) => {
            let said = said.as_ref().map_or("done", |s| s.as_str());
            let undo = undo.map(|u| format!(", undo #{}", u.0)).unwrap_or_default();
            format!("{action} [outcome: {said}{undo}]")
        }
        (_, StepEnd::Done { said, value, undo }) => {
            let said = said
                .as_ref()
                .map(|s| format!(" \"{}\"", s.as_str()))
                .unwrap_or_default();
            let value = value
                .as_ref()
                .map(|v| match v {
                    Reveal::Plain(v) => format!(" value {}", json(v)),
                    Reveal::Handle(h) => format!(" value #{}", h.0),
                })
                .unwrap_or_default();
            let undo = undo.map(|u| format!(" undo #{}", u.0)).unwrap_or_default();
            format!("{action} done{said}{value}{undo}")
        }
        (_, StepEnd::Refused(refusal)) => {
            let hint = refusal_hint(refusal, handles)
                .map(|h| format!(": {h}"))
                .unwrap_or_default();
            format!("{action} refused {}{hint}", json(refusal))
        }
        (_, StepEnd::Unconfirmed(end)) => format!("{action} not confirmed {}", json(end)),
        (_, StepEnd::Held(held)) => {
            format!("{action} not run: {}; {INSTEAD}", held_text(*held))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docket_core::{ActionRef, CallId, CharCount, Handle, HandleShape};
    use prov::{ActionName, Effect, Source};

    fn step(end: StepEnd) -> StepLine {
        StepLine {
            call: CallId(0),
            action: ActionRef {
                app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
                name: ActionName::parse("mail.message.forward").expect("name"),
            },
            effect: Effect::Outbound,
            end,
            shown: StepShown::Full,
        }
    }

    fn card(n: u64) -> HandleCard {
        HandleCard {
            handle: Handle(n),
            shape: HandleShape::Text,
            from: Source::Mail,
            size: CharCount(1),
        }
    }

    #[test]
    fn an_unknown_handle_is_told_with_the_handles_held() {
        let end = StepEnd::Refused(CallRefusal::BadArgs {
            param: docket_core::ParamName::parse("recipient").expect("param"),
            why: ArgFault::UnknownHandle,
        });
        let none = step_line(&step(end.clone()), &[]);
        assert!(none.contains("\"recipient\" names a handle that does not exist"));
        assert!(none.contains("you hold no handles yet"));
        let some = step_line(&step(end), &[card(3), card(4)]);
        assert!(some.ends_with("handles you hold: #3 #4"), "{some}");
    }

    #[test]
    fn a_held_call_tells_what_to_do_instead() {
        let line = step_line(&step(StepEnd::Held(Held::Empty)), &[]);
        assert!(line.contains("got nothing"));
        assert!(line.contains("quire_ask"));
    }

    #[test]
    fn a_coarse_denial_stays_a_bare_code() {
        let end = StepEnd::Refused(CallRefusal::Denied(docket_core::DenyCode::OutsideTask));
        let line = step_line(&step(end), &[]);
        assert!(line.ends_with("\"outside_task\"}"), "{line}");
    }
}
