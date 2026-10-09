//! One past step as the planner reads it. A refusal is told as a short reason the model can act
//! on (which argument, why, and which handles it does hold), never a policy id or a reviewer's
//! words: the router's coarse code is all that crosses, and a handle stays an opaque `#n`.

use crate::accepts::used_as;
use crate::read_ask::read_fault_text;
use crate::unconfirmed::unconfirmed_text;
use docket_core::{
    ActionCard, ArgFault, ArgsFault, CallRefusal, Handle, HandleCard, HandleShape, Held,
    ReplyFault, Reveal, StepEnd, StepLine, StepShown, TargetFault, Value, Why,
};

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

/// A handle as the planner names it: `#3`, and what it holds when it holds a thing
/// (`#3 mail.thread`). Never its content.
fn named(handle: Handle, handles: &[HandleCard]) -> String {
    match handles
        .iter()
        .find(|c| c.handle == handle)
        .map(|c| &c.shape)
    {
        Some(HandleShape::Entity(kind)) => format!("#{} {kind}", handle.0),
        Some(HandleShape::Text | HandleShape::File) | None => format!("#{}", handle.0),
    }
}

/// How many things a list holds, as ` (2 items)`.
fn items_text(count: usize) -> String {
    match count {
        1 => " (1 item)".to_owned(),
        n => format!(" ({n} items)"),
    }
}

/// A value of a step as the planner reads it: handles (and lists of them, as a search returns
/// its things) as `#n`, everything else as JSON.
fn value_text(value: &Value, handles: &[HandleCard]) -> String {
    match value {
        Value::Handle(h) => named(*h, handles),
        Value::List(items) if items.iter().all(|i| matches!(i, Value::Handle(_))) => {
            let names: Vec<String> = items
                .iter()
                .filter_map(|i| match i {
                    Value::Handle(h) => Some(named(*h, handles)),
                    _ => None,
                })
                .collect();
            format!("[{}]{}", names.join(", "), items_text(items.len()))
        }
        other => json(other),
    }
}

/// What was wrong with a reply that could not be read as a call, and how to write it again.
fn unread_text(fault: &ReplyFault) -> String {
    let said = |name: &str| {
        if name.is_empty() {
            "an argument".to_owned()
        } else {
            format!("argument \"{name}\"")
        }
    };
    let why = match fault {
        ReplyFault::NoSuchTool(name) if name.is_empty() => {
            "there is no tool by that name; call only the tools you were given".to_owned()
        }
        ReplyFault::NoSuchTool(name) => {
            format!("there is no tool named \"{name}\"; call only the tools you were given")
        }
        ReplyFault::NotJson => "its arguments were not valid JSON".to_owned(),
        ReplyFault::Args(ArgsFault::NotAnObject) => {
            "its arguments must be a JSON object".to_owned()
        }
        ReplyFault::Args(ArgsFault::Unknown(name)) => {
            format!("{} is not one this action takes", said(name))
        }
        ReplyFault::Args(ArgsFault::Missing(param)) => {
            format!(
                "argument \"{}\" is required and was missing",
                param.as_str()
            )
        }
        ReplyFault::Args(ArgsFault::Wrong { param, why }) => {
            let how = match why {
                Why::Type => "has the wrong type",
                Why::Range => "is out of its range, or not one of its options",
            };
            format!("argument \"{}\" {how}", param.as_str())
        }
        ReplyFault::Args(ArgsFault::Target(target)) => target_text(*target),
        ReplyFault::Read(fault) => read_fault_text(fault),
    };
    let lead = match fault {
        ReplyFault::Read(_) => "your quire_read gave no answer",
        _ => "your last reply could not be read as a call",
    };
    format!("{lead}: {why}; write the call again, ask the person with quire_ask, or finish")
}

fn target_text(fault: TargetFault) -> String {
    match fault {
        TargetFault::Missing => "\"target\" is required: name what the action acts on".to_owned(),
        TargetFault::Unexpected => "this action takes no \"target\"".to_owned(),
        TargetFault::NotNameable => "this action acts on a live text field, which you cannot name".to_owned(),
        TargetFault::Malformed => "\"target\" is not of the shape this action acts on: a thing as {\"app\", \"kind\", \"key\"} or {\"handle\": n}".to_owned(),
    }
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

/// The handles a repeated call already returned, when every earlier run of it (same action, same
/// handles) returned the same: nothing is restated when the runs differ, since a line cannot say
/// which of them this repeat was.
fn held_answer(step: &StepLine, seen: &Seen<'_>) -> Option<(Vec<Handle>, bool)> {
    let mut answers = seen
        .earlier
        .iter()
        .filter(|s| s.action == step.action && s.with == step.with)
        .filter_map(|s| match &s.end {
            StepEnd::Done { value: Some(v), .. } => Some(v),
            _ => None,
        });
    let first = answers.next()?;
    if answers.any(|a| a != first) {
        return None;
    }
    returned(first)
}

/// Where the things a repeated call returned can go next, one clause per kind of thing.
fn next_use(handles: &[Handle], seen: &Seen<'_>) -> String {
    let mut kinds: Vec<&prov::EntityKind> = Vec::new();
    for card in seen.handles.iter().filter(|c| handles.contains(&c.handle)) {
        if let HandleShape::Entity(kind) = &card.shape
            && !kinds.contains(&kind)
        {
            kinds.push(kind);
        }
    }
    let clauses: Vec<String> = kinds
        .into_iter()
        .filter_map(|kind| {
            let of_kind: Vec<String> = seen
                .handles
                .iter()
                .filter(|c| {
                    handles.contains(&c.handle) && c.shape == HandleShape::Entity(kind.clone())
                })
                .map(|c| format!("#{}", c.handle.0))
                .collect();
            let used = used_as(kind, seen.actions)?;
            Some(format!("{} can be used {used}", bounded(of_kind).join(" ")))
        })
        .collect();
    clauses.iter().map(|c| format!("; {c}")).collect()
}

/// A call that was not run because it repeats one with the same answer: when that answer is
/// held, it is restated with where it goes next, so the repeat has nothing left to look for.
fn held_line(head: &str, step: &StepLine, held: Held, seen: &Seen<'_>) -> String {
    match (held, held_answer(step, seen)) {
        (Held::Unchanged, Some((hs, list))) => {
            let names = bounded(hs.iter().map(|h| named(*h, seen.handles)).collect());
            let answer = if list {
                format!("[{}]{}", names.join(", "), items_text(hs.len()))
            } else {
                names.join(", ")
            };
            format!(
                "{head} not run: you already hold its answer {answer}{}; repeating it again asks the person",
                next_use(&hs, seen)
            )
        }
        _ => format!("{head} not run: {}; {INSTEAD}", held_text(held)),
    }
}

const INSTEAD: &str =
    "change the arguments, try another action, ask the person with quire_ask, or finish";

/// The most handles a masked line names in one place; the rest are counted, so a line stays short
/// and the same every turn.
const MASKED_HANDLES: usize = 4;

/// The handles a value is made of, in order, and whether it was a list.
fn returned(value: &Reveal<Value>) -> Option<(Vec<Handle>, bool)> {
    match value {
        Reveal::Handle(h) | Reveal::Plain(Value::Handle(h)) => Some((vec![*h], false)),
        Reveal::Plain(Value::List(items)) => items
            .iter()
            .map(|i| match i {
                Value::Handle(h) => Some(*h),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .map(|hs| (hs, true)),
        Reveal::Plain(_) => None,
    }
}

/// At most `MASKED_HANDLES` of `names`, then `+k more`.
fn bounded(names: Vec<String>) -> Vec<String> {
    let more = names.len().saturating_sub(MASKED_HANDLES);
    let mut shown: Vec<String> = names.into_iter().take(MASKED_HANDLES).collect();
    if more > 0 {
        shown.push(format!("+{more} more"));
    }
    shown
}

/// The handles a call named, as ` #1 #3` (a leading space), or nothing. Always plain `#n`: a
/// masked line must read the same on every turn.
fn with_text(with: &[Handle]) -> String {
    let names = bounded(with.iter().map(|h| format!("#{}", h.0)).collect());
    if names.is_empty() {
        String::new()
    } else {
        format!(" {}", names.join(" "))
    }
}

/// What a masked step returned, as ` → #4` or ` → [#1 mail.thread, #2 mail.thread]`.
fn returned_text(value: Option<&Reveal<Value>>, handles: &[HandleCard]) -> String {
    match value.and_then(returned) {
        Some((hs, false)) => hs
            .first()
            .map(|h| format!(" → {}", named(*h, handles)))
            .unwrap_or_default(),
        Some((hs, true)) => {
            let names = bounded(hs.iter().map(|h| named(*h, handles)).collect());
            format!(" → [{}]{}", names.join(", "), items_text(hs.len()))
        }
        None => String::new(),
    }
}

/// What a line of history may look back on: the handles the planner holds, the steps before
/// this one, and the actions it is offered.
pub(crate) struct Seen<'a> {
    pub(crate) handles: &'a [HandleCard],
    pub(crate) earlier: &'a [StepLine],
    pub(crate) actions: &'a [ActionCard],
}

/// One step, as one line of the planner's history, with nothing before it to look back on.
pub(crate) fn step_line(step: &StepLine, handles: &[HandleCard]) -> String {
    step_line_in(
        step,
        &Seen {
            handles,
            earlier: &[],
            actions: &[],
        },
    )
}

/// One step, as one line of the planner's history.
pub(crate) fn step_line_in(step: &StepLine, seen: &Seen<'_>) -> String {
    let handles = seen.handles;
    let action = format!("{}.{}", step.action.app, step.action.name);
    let head = format!("{action}{}", with_text(&step.with));
    match (&step.shown, &step.end) {
        (StepShown::Masked, StepEnd::Done { said, undo, value }) => {
            let said = said.as_ref().map_or("done", |s| s.as_str());
            let undo = undo.map(|u| format!(", undo #{}", u.0)).unwrap_or_default();
            let got = returned_text(value.as_ref(), handles);
            format!("{head}{got} [outcome: {said}{undo}]")
        }
        (_, StepEnd::Done { said, value, undo }) => {
            let said = said
                .as_ref()
                .map(|s| format!(" \"{}\"", s.as_str()))
                .unwrap_or_default();
            let value = value
                .as_ref()
                .map(|v| match v {
                    Reveal::Plain(v) => format!(" value {}", value_text(v, handles)),
                    Reveal::Handle(h) => format!(" value {}", named(*h, handles)),
                })
                .unwrap_or_default();
            let undo = undo.map(|u| format!(" undo #{}", u.0)).unwrap_or_default();
            format!("{head} done{said}{value}{undo}")
        }
        (_, StepEnd::Refused(refusal)) => {
            let hint = refusal_hint(refusal, handles)
                .map(|h| format!(": {h}"))
                .unwrap_or_default();
            format!("{head} refused {}{hint}", json(refusal))
        }
        (_, StepEnd::Unconfirmed(end)) => {
            format!("{head} not confirmed: {}", unconfirmed_text(*end))
        }
        (_, StepEnd::Unread(fault)) => unread_text(fault),
        (_, StepEnd::Interrupted) => format!(
            "{head} interrupted by a restart: it may have run, it is not run again; check before asking again"
        ),
        (_, StepEnd::Held(held)) => held_line(&head, step, *held, seen),
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
            with: Vec::new(),
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
    fn an_interrupted_call_says_it_may_have_run_and_will_not_run_again() {
        let line = step_line(&step(StepEnd::Interrupted), &[]);
        assert!(line.contains("interrupted by a restart"), "{line}");
        assert!(line.contains("may have run"));
        assert!(line.contains("not run again"));
    }

    #[test]
    fn a_held_call_tells_what_to_do_instead() {
        let line = step_line(&step(StepEnd::Held(Held::Empty)), &[]);
        assert!(line.contains("got nothing"));
        assert!(line.contains("quire_ask"));
    }

    fn searched(action: &str, end: StepEnd) -> StepLine {
        StepLine {
            action: ActionRef {
                app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
                name: ActionName::parse(action).expect("name"),
            },
            ..step(end)
        }
    }

    fn found_contact() -> StepEnd {
        StepEnd::Done {
            said: None,
            value: Some(Reveal::Plain(Value::List(vec![Value::Handle(Handle(5))]))),
            undo: None,
        }
    }

    #[test]
    fn a_repeat_gets_the_answer_it_already_holds_and_where_it_goes() {
        let ran = searched("mail.contact.search", found_contact());
        let again = searched("mail.contact.search", StepEnd::Held(Held::Unchanged));
        let cards = [entity_card(5, "mail.contact")];
        let actions = crate::accepts::fixtures::mail();
        let earlier = [ran.clone(), ran];
        let seen = Seen {
            handles: &cards,
            earlier: &earlier,
            actions: &actions,
        };
        assert_eq!(
            step_line_in(&again, &seen),
            "org.quire.Mail.mail.contact.search not run: you already hold its answer [#5 mail.contact] (1 item); #5 can be used as \"to\" in mail.message.forward, mail.message.send; repeating it again asks the person"
        );
    }

    #[test]
    fn a_repeat_is_not_told_an_answer_the_earlier_runs_do_not_agree_on() {
        let first = searched("mail.contact.search", found_contact());
        let other = searched(
            "mail.contact.search",
            done(
                "Found",
                Reveal::Plain(Value::List(vec![Value::Handle(Handle(6))])),
            ),
        );
        let again = searched("mail.contact.search", StepEnd::Held(Held::Unchanged));
        let earlier = [first, other];
        let seen = Seen {
            handles: &[],
            earlier: &earlier,
            actions: &[],
        };
        let line = step_line_in(&again, &seen);
        assert!(
            line.contains("got the same answer") && line.contains(INSTEAD),
            "{line}"
        );
    }

    #[test]
    fn a_repeat_with_no_answer_to_restate_keeps_the_old_line() {
        let again = searched("mail.contact.search", StepEnd::Held(Held::Empty));
        let line = step_line(&again, &[]);
        assert!(
            line.ends_with(INSTEAD) && line.contains("got nothing"),
            "{line}"
        );
    }

    #[test]
    fn a_coarse_denial_stays_a_bare_code() {
        let end = StepEnd::Refused(CallRefusal::Denied(docket_core::DenyCode::OutsideTask));
        let line = step_line(&step(end), &[]);
        assert!(line.ends_with("\"outside_task\"}"), "{line}");
    }

    fn entity_card(n: u64, kind: &str) -> HandleCard {
        HandleCard {
            handle: Handle(n),
            shape: HandleShape::Entity(prov::EntityKind::parse(kind).expect("kind")),
            from: Source::Mail,
            size: CharCount(0),
        }
    }

    fn found(value: Value) -> StepEnd {
        StepEnd::Done {
            said: Some(docket_core::LabelText::parse("Found threads").expect("text")),
            value: Some(Reveal::Plain(value)),
            undo: None,
        }
    }

    #[test]
    fn things_a_step_returned_are_shown_as_handles_with_their_kind() {
        let value = Value::List(vec![Value::Handle(Handle(3)), Value::Handle(Handle(4))]);
        let cards = [entity_card(3, "mail.thread"), entity_card(4, "mail.thread")];
        let line = step_line(&step(found(value)), &cards);
        assert!(
            line.ends_with(
                "done \"Found threads\" value [#3 mail.thread, #4 mail.thread] (2 items)"
            ),
            "{line}"
        );
        assert!(
            !line.contains("lisbon"),
            "no key of the app reaches the planner: {line}"
        );
        let one = step_line(
            &step(found(Value::Handle(Handle(5)))),
            &[entity_card(5, "mail.contact")],
        );
        assert!(one.ends_with("value #5 mail.contact"), "{one}");
        let plain = step_line(&step(found(Value::Integer(7))), &[]);
        assert!(
            plain.ends_with("value {\"kind\":\"integer\",\"v\":7}"),
            "{plain}"
        );
    }

    #[test]
    fn an_unreadable_reply_is_told_what_was_wrong() {
        let table = [
            (
                ReplyFault::NoSuchTool("mail.nuke".into()),
                "no tool named \"mail.nuke\"",
            ),
            (
                ReplyFault::NoSuchTool(String::new()),
                "no tool by that name",
            ),
            (ReplyFault::NotJson, "not valid JSON"),
            (
                ReplyFault::Args(ArgsFault::Missing(
                    docket_core::ParamName::parse("to").expect("p"),
                )),
                "argument \"to\" is required and was missing",
            ),
            (
                ReplyFault::Args(ArgsFault::Wrong {
                    param: docket_core::ParamName::parse("query").expect("p"),
                    why: Why::Type,
                }),
                "argument \"query\" has the wrong type",
            ),
            (
                ReplyFault::Args(ArgsFault::Unknown("admin".into())),
                "argument \"admin\" is not one this action takes",
            ),
            (
                ReplyFault::Args(ArgsFault::Target(TargetFault::Malformed)),
                "{\"handle\": n}",
            ),
        ];
        for (fault, want) in table {
            let line = step_line(&step(StepEnd::Unread(fault.clone())), &[]);
            assert!(
                line.starts_with("your last reply could not be read as a call: "),
                "{line}"
            );
            assert!(line.contains(want), "{fault:?}: {line}");
            assert!(line.contains("quire_ask"), "{line}");
        }
    }

    #[test]
    fn a_read_that_gave_no_answer_is_told_what_was_wrong_and_the_shape_expected() {
        use docket_core::{ReadFault, SchemaFault};
        let table = [
            (ReadFault::Inputs, "list of handle numbers"),
            (ReadFault::Task, "classify, extract, summarise, compare"),
            (
                ReadFault::Want,
                "{\"kind\": \"choice\", \"v\": [\"forward\", \"skip\"]}",
            ),
            (
                ReadFault::WantOption("Lisbon receipts?!".to_owned()),
                "option \"Lisbon receipts?!\" is not an id and has no id to read it as: use lowercase letters, digits and _, such as \"lisbon_receipts\"",
            ),
            (
                ReadFault::WantClash(docket_core::ChoiceId::parse("a_b").expect("id")),
                "both \"a_b\"",
            ),
            (ReadFault::WantText, "text needs v: {\"max\": N}"),
            (ReadFault::NotHeld, "not a handle you were shown"),
            (
                ReadFault::NotText {
                    handle: Handle(1),
                    shape: HandleShape::Entity(
                        prov::EntityKind::parse("mail.thread").expect("kind"),
                    ),
                },
                "#1 is a mail.thread, not text: read it first with the action that reads a mail.thread (such as mail.thread.read) and give quire_read the handle that call returns",
            ),
            (
                ReadFault::NotText {
                    handle: Handle(2),
                    shape: HandleShape::File,
                },
                "#2 is a file, not text",
            ),
            (
                ReadFault::OutOfSchema(SchemaFault::NotInSet),
                "did not fit \"want\"",
            ),
            (ReadFault::Unparseable, "could not be read"),
            (ReadFault::Refused, "declined"),
        ];
        for (fault, want) in table {
            let line = step_line(&step(StepEnd::Unread(ReplyFault::Read(fault.clone()))), &[]);
            assert!(
                line.starts_with("your quire_read gave no answer: "),
                "{line}"
            );
            assert!(line.contains(want), "{fault:?}: {line}");
        }
    }

    fn masked(action: &str, with: Vec<u64>, end: StepEnd) -> StepLine {
        StepLine {
            action: ActionRef {
                app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
                name: ActionName::parse(action).expect("name"),
            },
            shown: StepShown::Masked,
            with: with.into_iter().map(Handle).collect(),
            ..step(end)
        }
    }

    fn done(said: &str, value: Reveal<Value>) -> StepEnd {
        StepEnd::Done {
            said: Some(docket_core::LabelText::parse(said).expect("text")),
            value: Some(value),
            undo: None,
        }
    }

    #[test]
    fn a_masked_read_keeps_what_it_named_and_what_it_returned() {
        let read = masked(
            "mail.thread.read",
            vec![1],
            done("Read the thread", Reveal::Handle(Handle(4))),
        );
        let line = step_line(&read, &[card(4)]);
        assert_eq!(
            line,
            "org.quire.Mail.mail.thread.read #1 → #4 [outcome: Read the thread]"
        );
        assert_eq!(
            line,
            step_line(&read, &[card(4), card(5)]),
            "stable as cards grow"
        );
    }

    #[test]
    fn a_masked_search_keeps_the_things_it_found() {
        let found = Value::List(vec![Value::Handle(Handle(1)), Value::Handle(Handle(2))]);
        let search = masked(
            "mail.thread.search",
            vec![],
            done("Found threads", Reveal::Plain(found)),
        );
        let cards = [entity_card(1, "mail.thread"), entity_card(2, "mail.thread")];
        assert_eq!(
            step_line(&search, &cards),
            "org.quire.Mail.mail.thread.search → [#1 mail.thread, #2 mail.thread] (2 items) [outcome: Found threads]"
        );
    }

    #[test]
    fn a_masked_line_is_bounded() {
        let many: Vec<u64> = (1..=9).collect();
        let list = Value::List(many.iter().map(|n| Value::Handle(Handle(*n))).collect());
        let line = step_line(
            &masked(
                "mail.message.forward",
                many,
                done("Done", Reveal::Plain(list)),
            ),
            &[],
        );
        assert!(
            line.contains(" #1 #2 #3 #4 +5 more → [#1, #2, #3, #4, +5 more]"),
            "{line}"
        );
    }

    #[test]
    fn a_full_step_shows_its_handle_arguments() {
        let mut forward = step(StepEnd::Held(Held::Empty));
        forward.with = vec![Handle(1), Handle(3)];
        let line = step_line(&forward, &[]);
        assert!(
            line.starts_with("org.quire.Mail.mail.message.forward #1 #3 not run"),
            "{line}"
        );
    }
}
