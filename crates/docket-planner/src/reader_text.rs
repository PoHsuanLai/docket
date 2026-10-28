//! The reader's answer as the planner reads it. The reader's step is the companion's own
//! (`companion.read.<task>`); in the history it reads as the planner's `quire_read`, and what it
//! returned is named for what it is: the reader's answer, never content. The view also says which
//! of those answers have not yet been shown to the person, so the planner can see that the task
//! can end by showing one.

use crate::show::MOST_SHOWN;
use crate::step_text::{returned, with_text};
use docket_core::{HandleCard, HandleShape, PlannerView, StepEnd, StepLine};
use docket_vocab::Handle;

/// The head of a history line for the reader's step: `quire_read summarise #10`. None for any
/// other step.
pub(crate) fn reader_head(step: &StepLine) -> Option<String> {
    let task = step.reader_task()?;
    Some(format!(
        "quire_read {}{}",
        task.word(),
        with_text(&step.with)
    ))
}

/// The line of a reader step that returned one handle: `quire_read summarise #10 → #11 (the
/// reader's answer, text)`. None for any other step, or one that returned something else.
pub(crate) fn reader_answer(head: &str, step: &StepLine, handles: &[HandleCard]) -> Option<String> {
    step.reader_task()?;
    let StepEnd::Done {
        value: Some(value), ..
    } = &step.end
    else {
        return None;
    };
    let (answer, false) = returned(value)? else {
        return None;
    };
    let [handle] = answer.as_slice() else {
        return None;
    };
    let text = match handles.iter().find(|c| c.handle == *handle) {
        Some(card) if card.shape == HandleShape::Text => ", text",
        _ => "",
    };
    Some(format!(
        "{head} → #{} (the reader's answer{text})",
        handle.0
    ))
}

/// The one handle a reader step returned, when it is the reader's step and returned one.
fn answered_by(step: &StepLine) -> Option<(Handle, &StepLine)> {
    step.reader_task()?;
    let StepEnd::Done {
        value: Some(value), ..
    } = &step.end
    else {
        return None;
    };
    match returned(value)? {
        (answer, false) => match answer.as_slice() {
            [handle] => Some((*handle, step)),
            _ => None,
        },
        (_, true) => None,
    }
}

/// How the handle list says where a handle came from, when the reader made it: `the reader's
/// summarise answer about #10`. None for a handle some other step made.
pub(crate) fn reader_origin(handle: Handle, history: &[StepLine]) -> Option<String> {
    let (_, step) = history
        .iter()
        .filter_map(answered_by)
        .find(|(made, _)| *made == handle)?;
    let task = step.reader_task()?;
    Some(format!(
        "the reader's {} answer about{}",
        task.word(),
        with_text(&step.with)
    ))
}

/// Whether a step that ended done took `handle` as an input: an answer already put to use (the
/// body of a file that was made, of a message that was sent) is not waiting to be shown.
fn put_to_use(handle: Handle, history: &[StepLine]) -> bool {
    history
        .iter()
        .any(|s| matches!(s.end, StepEnd::Done { .. }) && s.with.contains(&handle))
}

/// The reader's text answers the person has not been shown and no done step has used, oldest
/// first, at most [`MOST_SHOWN`] of the newest: what `quire_finish` can show to answer them.
pub(crate) fn ready_to_show(view: &PlannerView) -> Vec<Handle> {
    let mut ready: Vec<Handle> = view
        .history
        .iter()
        .filter_map(answered_by)
        .map(|(handle, _)| handle)
        .filter(|handle| !view.shown.contains(handle))
        .filter(|handle| !put_to_use(*handle, &view.history))
        .filter(|handle| {
            view.handles
                .iter()
                .any(|c| c.handle == *handle && c.shape == HandleShape::Text)
        })
        .collect();
    ready.dedup();
    let older = ready.len().saturating_sub(MOST_SHOWN);
    ready.drain(..older);
    ready
}

/// The line that offers the finish: `Ready to show: #11 (quire_finish {"show":[11]})`. None when
/// no reader answer waits to be shown.
pub(crate) fn ready_line(view: &PlannerView) -> Option<String> {
    let ready = ready_to_show(view);
    if ready.is_empty() {
        return None;
    }
    let names: Vec<String> = ready.iter().map(|h| format!("#{}", h.0)).collect();
    let numbers: Vec<String> = ready.iter().map(|h| h.0.to_string()).collect();
    Some(format!(
        "Ready to show: {} (quire_finish {{\"show\":[{}]}})",
        names.join(", "),
        numbers.join(",")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::user_text;
    use agent_loop::{Sources, assemble};
    use docket_core::{
        AgentConfig, ContextView, Held, HereView, Reveal, Roster, SelectionView, StepShown,
        TextTargetView, VisibleView,
    };
    use docket_vocab::{ActionRef, CallId, CharCount, Value};
    use porter_core::AppName;
    use porter_ids::Count;
    use prov::{ActionName, Effect, Integrity, Source};

    fn step(app: &str, name: &str, with: &[u64], end: StepEnd) -> StepLine {
        StepLine {
            call: CallId(0),
            action: ActionRef {
                app: AppName::parse(app).expect("app"),
                name: ActionName::parse(name).expect("name"),
            },
            effect: Effect::Read,
            end,
            shown: StepShown::Full,
            with: with.iter().copied().map(Handle).collect(),
        }
    }

    fn returns(n: u64) -> StepEnd {
        StepEnd::Done {
            said: None,
            value: Some(Reveal::Handle(Handle(n))),
            undo: None,
        }
    }

    fn reader(task: &str, with: &[u64], end: StepEnd) -> StepLine {
        step(
            "org.quire.Companion",
            &format!("companion.read.{task}"),
            with,
            end,
        )
    }

    fn text_card(n: u64, size: u32) -> HandleCard {
        HandleCard {
            handle: Handle(n),
            shape: HandleShape::Text,
            from: Source::User,
            size: CharCount(size),
        }
    }

    /// A task that read a thing (#10) and had the reader summarise it (#11).
    fn sources(history: Vec<StepLine>, handles: Vec<HandleCard>, shown: Vec<Handle>) -> Sources {
        Sources {
            cards: vec![],
            profile: vec![],
            primer: None,
            rollup: None,
            roster: Roster::default(),
            episodes: vec![],
            recalled: vec![],
            context: ContextView {
                app: AppName::parse("org.quire.Shell").expect("app"),
                window: Reveal::Plain("w".into()),
                here: HereView::Nowhere,
                selection: SelectionView::Nothing,
                visible: VisibleView {
                    kind: None,
                    items: vec![],
                    total: Count(0),
                },
                text_target: TextTargetView::None,
            },
            turns: vec![],
            history,
            handles,
            inbox: vec![],
            skills: vec![],
            skill_texts: vec![],
            shown,
            taint: Integrity::Trusted,
            task_policy: None,
            summary: None,
        }
    }

    fn summarised() -> Vec<StepLine> {
        vec![
            step("org.quire.Mail", "mail.thread.read", &[3], returns(10)),
            reader("summarise", &[10], returns(11)),
        ]
    }

    fn viewed(history: Vec<StepLine>, shown: Vec<Handle>) -> String {
        let handles = vec![text_card(10, 58), text_card(11, 63)];
        let budget = AgentConfig::default().assembler;
        user_text(&assemble(&budget, &sources(history, handles, shown)))
    }

    #[test]
    fn the_readers_step_reads_as_the_reader_and_its_answer_says_what_it_is() {
        let text = viewed(summarised(), vec![]);
        assert!(
            text.contains("- quire_read summarise #10 → #11 (the reader's answer, text)"),
            "{text}"
        );
        assert!(
            text.contains("- #11 text (63 characters): the reader's summarise answer about #10"),
            "{text}"
        );
        assert!(
            text.contains("- #10 text (58 characters), returned by mail.thread.read #3"),
            "{text}"
        );
        assert!(!text.contains("companion.read"), "{text}");
    }

    #[test]
    fn a_reader_text_answer_not_yet_shown_is_offered_and_not_after() {
        let ready = "Ready to show: #11 (quire_finish {\"show\":[11]})";
        assert!(viewed(summarised(), vec![]).contains(ready));
        assert!(!viewed(summarised(), vec![Handle(11)]).contains("Ready to show"));
        let only_read = summarised().into_iter().take(1).collect();
        assert!(!viewed(only_read, vec![]).contains("Ready to show"));
    }

    #[test]
    fn a_reader_answer_already_put_to_use_is_not_offered() {
        let mut steps = summarised();
        let made = StepEnd::Done {
            said: None,
            value: None,
            undo: None,
        };
        steps.push(step("org.quire.Files", "files.file.create", &[11], made));
        assert!(!viewed(steps, vec![]).contains("Ready to show"));
    }

    #[test]
    fn a_reader_answer_that_is_not_text_or_was_not_run_is_not_offered() {
        let held = vec![reader("summarise", &[10], StepEnd::Held(Held::Unchanged))];
        assert!(!viewed(held, vec![]).contains("Ready to show"));
        let choice = StepEnd::Done {
            said: None,
            value: Some(Reveal::Plain(Value::Integer(3))),
            undo: None,
        };
        let line = step_line_of(&reader("classify", &[10], choice));
        assert!(
            line.starts_with("quire_read classify #10 done value "),
            "{line}"
        );
        assert_eq!(
            ready_to_show_in(vec![reader("classify", &[10], returns(12))]),
            vec![]
        );
    }

    fn step_line_of(step: &StepLine) -> String {
        crate::step_text::step_line(step, &[])
    }

    fn ready_to_show_in(history: Vec<StepLine>) -> Vec<Handle> {
        let budget = AgentConfig::default().assembler;
        let handles = vec![text_card(10, 58), text_card(11, 63)];
        ready_to_show(&assemble(&budget, &sources(history, handles, vec![])))
    }

    #[test]
    fn the_newest_answers_are_offered_when_many_wait() {
        let history: Vec<StepLine> = (0..10u64)
            .map(|n| reader("summarise", &[10], returns(100 + n)))
            .collect();
        let budget = AgentConfig::default().assembler;
        let handles = (0..10u64).map(|n| text_card(100 + n, 5)).collect();
        let view = assemble(&budget, &sources(history, handles, vec![]));
        let ready = ready_to_show(&view);
        assert_eq!(ready.len(), MOST_SHOWN);
        assert_eq!(ready.last(), Some(&Handle(109)));
    }

    #[test]
    fn a_repeated_reader_ask_is_told_what_is_already_held() {
        let ran = reader("summarise", &[10], returns(11));
        let again = reader("summarise", &[10], StepEnd::Held(Held::Unchanged));
        let handles = [text_card(10, 58), text_card(11, 63)];
        let earlier = [ran];
        let seen = crate::step_text::Seen {
            handles: &handles,
            earlier: &earlier,
            actions: &[],
            roster: &[],
        };
        assert_eq!(
            crate::step_text::step_line_in(&again, &seen),
            "quire_read summarise #10 not run: you already hold its answer #11; repeating it again asks the person"
        );
    }
}
