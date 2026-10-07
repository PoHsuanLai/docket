//! The repeat guard: a pure ledger of the calls one turn has made and what each came to. A small
//! model that gets nothing back tends to make the same call again, word for word, until the
//! budget ends the turn with nothing for the person. The guard holds the second identical call
//! back (the planner is told so in its history), and on the third stops the turn so the person is
//! asked. A call is never held until its last answer was empty, unchanged or refused for its
//! arguments, so asking again after something changed is free.

use docket_core::{ActionRef, CallId, CallRefusal, CallRequest, Held, StepEnd, Value};
use serde::{Deserialize, Serialize};

/// How many times a stale call is held back, with a note, before the turn stops: the second
/// identical call is held, the third stops the turn.
pub const HOLDS_BEFORE_STOP: u8 = 1;
/// How many calls one turn may have held back, across all its calls, before it stops. This caps
/// A,B,A,B and wider cycles whose every call is stale.
pub const MOST_HOLDS_PER_TURN: u8 = 4;
/// How many replies in a row the planner may make that cannot be read as a call before the turn
/// stops: the first and the second are told what was wrong, the third asks the person.
pub const MOST_UNREADABLE: u8 = 3;
/// How many distinct calls a turn remembers; the oldest are forgotten first.
pub const MOST_REMEMBERED: usize = 32;

/// A call as the guard tells two apart: its action, target and argument values (not their labels).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CallKey(String);

/// What a call returned, as text, so two answers compare.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Print(String);

/// What the last run of a call came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
enum Answer {
    /// Out, not yet ended.
    Waiting(CallId),
    /// It returned nothing.
    Empty,
    /// It was refused for its arguments or its name.
    Refused,
    /// It returned what it returned the time before.
    Unchanged,
    /// It returned something new.
    Fresh,
    /// It cannot be compared (a handle, a journaled change, another refusal).
    Opaque,
}

impl Answer {
    fn stale(&self) -> Option<Held> {
        match self {
            Answer::Empty => Some(Held::Empty),
            Answer::Unchanged => Some(Held::Unchanged),
            Answer::Refused => Some(Held::Refused),
            Answer::Waiting(_) | Answer::Fresh | Answer::Opaque => None,
        }
    }
}

/// One distinct call of the turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Seen {
    key: CallKey,
    action: ActionRef,
    answer: Answer,
    /// The last answer that could be compared.
    last: Option<Print>,
    /// How many times it has been held back since it last ran.
    held: u8,
}

/// Why the turn stops.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Stuck {
    /// The planner made this stale call again after being told.
    Repeating(Held, ActionRef),
    /// Many stale calls, never the same one twice running.
    Circling,
    /// Reply after reply that could not be read as a call, though each was told what was wrong.
    Unreadable,
}

impl Stuck {
    /// What the person is asked when the turn stops: what could not be found, or that the
    /// assistant is going round in circles, and what it needs from them.
    pub fn question(&self) -> String {
        match self {
            Stuck::Repeating(Held::Empty, action) => format!(
                "I couldn't find anything with {}, and asking again will not change that. What should I look for instead?",
                action.name.as_str()
            ),
            Stuck::Repeating(Held::Unchanged, action) => format!(
                "I keep getting the same answer from {}. How would you like me to go on?",
                action.name.as_str()
            ),
            Stuck::Repeating(Held::Refused, action) => format!(
                "{} keeps being refused with what I give it. How would you like me to go on?",
                action.name.as_str()
            ),
            Stuck::Unreadable => "I keep writing steps I cannot get right, though I was told what was wrong. How would you like me to go on?".to_owned(),
            Stuck::Circling => "I am going round in circles without getting anywhere. How would you like me to go on?".to_owned(),
        }
    }
}

/// What the guard says of a call the planner wants made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Verdict {
    /// Make it.
    Run,
    /// Do not make it; tell the planner why.
    Hold(Held),
    /// End the turn by asking the person.
    Stop(Stuck),
}

/// The ledger of one turn.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guard {
    seen: Vec<Seen>,
    holds: u8,
    /// Replies in a row that could not be read as a call.
    #[serde(default)]
    unread: u8,
}

fn key_of(call: &CallRequest) -> CallKey {
    let args: Vec<(&str, &Value)> = call
        .args
        .iter()
        .map(|(name, held)| (name.as_str(), &held.value))
        .collect();
    let text = serde_json::to_string(&(&call.action, &call.target, args)).unwrap_or_default();
    CallKey(text)
}

fn nothing_in(value: &Value) -> bool {
    match value {
        Value::Text(text) => text.trim().is_empty(),
        Value::Entities(things) => things.is_empty(),
        _ => false,
    }
}

/// What an end came to, given the last comparable answer; the new last answer comes with it.
fn judged(end: &StepEnd, last: &Option<Print>) -> (Answer, Option<Print>) {
    use docket_core::Reveal;
    match end {
        StepEnd::Done { said, value, undo } => {
            let plain = match value {
                Some(Reveal::Handle(_)) => return (Answer::Opaque, last.clone()),
                Some(Reveal::Plain(v)) => Some(v),
                None => None,
            };
            if undo.is_some() {
                return (Answer::Opaque, last.clone());
            }
            // An empty value is nothing whatever the app said of it ("Found threads"); no value
            // at all is nothing only when the app said nothing either.
            let nothing = match plain {
                Some(v) => nothing_in(v),
                None => said.is_none() && value.is_none(),
            };
            if nothing {
                return (Answer::Empty, last.clone());
            }
            let print = Print(serde_json::to_string(&(said, plain)).unwrap_or_default());
            let answer = if last.as_ref() == Some(&print) {
                Answer::Unchanged
            } else {
                Answer::Fresh
            };
            (answer, Some(print))
        }
        StepEnd::Refused(CallRefusal::BadArgs { .. } | CallRefusal::NoSuchAction(_)) => {
            (Answer::Refused, last.clone())
        }
        StepEnd::Refused(_)
        | StepEnd::Unconfirmed(_)
        | StepEnd::Held(_)
        | StepEnd::Unread(_)
        | StepEnd::Interrupted => (Answer::Opaque, last.clone()),
    }
}

impl Guard {
    /// Whether `call`, wanted as position `id` of a batch, is made, held back or stops the turn.
    pub fn admit(self, id: CallId, call: &CallRequest) -> (Guard, Verdict) {
        let key = key_of(call);
        let Guard {
            mut seen,
            holds,
            unread,
        } = self;
        let Some(at) = seen.iter().position(|s| s.key == key) else {
            seen.push(Seen {
                key,
                action: call.action.clone(),
                answer: Answer::Waiting(id),
                last: None,
                held: 0,
            });
            let excess = seen.len().saturating_sub(MOST_REMEMBERED);
            seen.drain(..excess);
            return (
                Guard {
                    seen,
                    holds,
                    unread,
                },
                Verdict::Run,
            );
        };
        let entry = &mut seen[at];
        let Some(why) = entry.answer.stale() else {
            entry.answer = Answer::Waiting(id);
            entry.held = 0;
            return (
                Guard {
                    seen,
                    holds,
                    unread,
                },
                Verdict::Run,
            );
        };
        if entry.held >= HOLDS_BEFORE_STOP {
            let stuck = Stuck::Repeating(why, entry.action.clone());
            return (
                Guard {
                    seen,
                    holds,
                    unread,
                },
                Verdict::Stop(stuck),
            );
        }
        if holds >= MOST_HOLDS_PER_TURN {
            return (
                Guard {
                    seen,
                    holds,
                    unread,
                },
                Verdict::Stop(Stuck::Circling),
            );
        }
        entry.held += 1;
        let holds = holds.saturating_add(1);
        (
            Guard {
                seen,
                holds,
                unread,
            },
            Verdict::Hold(why),
        )
    }

    /// The call at position `id` ended.
    pub fn ended(self, id: CallId, end: &StepEnd) -> Guard {
        let seen = self
            .seen
            .into_iter()
            .map(|s| match s.answer {
                Answer::Waiting(w) if w == id => {
                    let (answer, last) = judged(end, &s.last);
                    Seen { answer, last, ..s }
                }
                _ => s,
            })
            .collect();
        Guard { seen, ..self }
    }

    /// The planner's reply could not be read as a call: tells it so, or, when it has done so
    /// [`MOST_UNREADABLE`] times running, ends the turn by asking the person.
    pub fn unreadable(self) -> (Guard, Option<Stuck>) {
        let unread = self.unread.saturating_add(1);
        let stuck = (unread >= MOST_UNREADABLE).then_some(Stuck::Unreadable);
        (Guard { unread, ..self }, stuck)
    }

    /// A new batch is about to go out: calls of an older batch that never ended are forgotten as
    /// answers, so their positions cannot be taken for the new ones.
    pub fn settled(self) -> Guard {
        let seen = self
            .seen
            .into_iter()
            .map(|s| match s.answer {
                Answer::Waiting(_) => Seen {
                    answer: Answer::Opaque,
                    ..s
                },
                _ => s,
            })
            .collect();
        Guard {
            seen,
            unread: 0,
            ..self
        }
    }
}
