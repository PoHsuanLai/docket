//! The planner's view in the person's words. What a planner reads, it repeats: an id such as
//! `t-179` or `r-1`, or an action's dotted name, comes back in the answer as if it meant
//! something to the person. So the view names a task or a run by what it is, and an action by
//! its label, and keeps the ids and names for the machinery that acts on them (handles, tool
//! names). Nothing here changes what the planner may do.

use docket_core::ActionCard;

/// What an id's first letter stands for, in words.
const KINDS: [(char, &str); 3] = [('t', "the task"), ('r', "the run"), ('s', "the session")];

/// The text of a step line of a skeleton (`step: <action> <targets> <outcome>`).
const STEP_START: &str = "step: ";

/// `text` with every id like `t-179`, `r-1` or `s-3` said as what it is ("the task").
pub(crate) fn without_ids(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < chars.len() {
        match id_at(&chars, at) {
            Some((len, noun)) => {
                out.push_str(noun);
                at += len;
            }
            None => {
                out.push(chars[at]);
                at += 1;
            }
        }
    }
    out
}

/// The length and noun of the id that starts at `at`, if one does: a kind letter, a dash and
/// digits, on word edges.
fn id_at(chars: &[char], at: usize) -> Option<(usize, &'static str)> {
    let starts =
        at == 0 || (!matches!(chars[at - 1], '-' | '_') && !chars[at - 1].is_alphanumeric());
    if !starts {
        return None;
    }
    let noun = KINDS
        .iter()
        .find(|(c, _)| *c == chars[at])
        .map(|(_, n)| *n)?;
    if chars.get(at + 1) != Some(&'-') {
        return None;
    }
    let digits = chars[at + 2..]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .count();
    let end = at + 2 + digits;
    let ends = chars.get(end).is_none_or(|c| !c.is_alphanumeric());
    (digits > 0 && ends).then_some((end - at, noun))
}

/// A skeleton's text with each step's action said by its label, where the action is offered and
/// its label is its own (the same rule as a step of the history).
pub(crate) fn step_labels(skeleton: &str, actions: &[ActionCard]) -> String {
    skeleton
        .lines()
        .map(|line| labelled(line, actions))
        .collect::<Vec<_>>()
        .join("\n")
}

fn labelled(line: &str, actions: &[ActionCard]) -> String {
    let Some(rest) = line.strip_prefix(STEP_START) else {
        return line.to_owned();
    };
    let (name, tail) = rest.split_once(' ').unwrap_or((rest, ""));
    let mut named = actions.iter().filter(|c| c.action.name.as_str() == name);
    match (named.next(), named.next()) {
        (Some(card), None) if actions.iter().filter(|c| c.label == card.label).count() == 1 => {
            format!("{STEP_START}{} {tail}", card.label.as_str())
        }
        _ => line.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_said_as_what_they_are_and_words_that_look_alike_stay() {
        let rows = [
            (
                "The only task, t-179 (research)",
                "The only task, the task (research)",
            ),
            ("run r-1 failed", "run the run failed"),
            ("\"r-12\"", "\"the run\""),
            ("s-3.", "the session."),
            (
                "list-3 and t-shirt and cart-4 and t-1a",
                "list-3 and t-shirt and cart-4 and t-1a",
            ),
            ("t-", "t-"),
            ("", ""),
        ];
        for (text, want) in rows {
            assert_eq!(without_ids(text), want, "{text}");
        }
    }
}
