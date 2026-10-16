//! The repeat guard: a pure ledger of the calls one turn has made and what each came to. A small
//! model that gets nothing back tends to make the same call again, word for word, until the
//! budget ends the turn with nothing for the person. The guard holds the second identical call
//! back (the planner is told so in its history), and on the third stops the turn so the person is
//! asked. A call is never held until its last answer was empty, unchanged or refused for its
//! arguments, so asking again after something changed is free.
//!
//! One more answer is held for good: a call known to only read (its effect is `Read`) that
//! returned a handle, and a reader ask that was answered. Asking for the same thing again gets
//! the same handle, so the repeat is not run: the planner is told it already holds the answer.
//! Such a repeat is held [`HOLDS_BEFORE_STOP_ANSWERED`] times, then the turn ends in words the
//! person can read. A call that wrote something makes the held reads of the world stale, and a
//! call that may write is never held this way.

use crate::declined::declined_words;
use docket_core::{ActionRef, CallId, CallRefusal, CallRequest, Held, ReaderAsk, StepEnd, Value};
use prov::Effect;
use serde::{Deserialize, Serialize};

/// How many times a stale call is held back, with a note, before the turn stops: the second
/// identical call is held, the third stops the turn.
pub const HOLDS_BEFORE_STOP: u8 = 1;
/// How many times a call whose answer is already held (a read that returned a handle, or a reader
/// ask that was answered) is skipped, with a note, before the turn stops: the person asked for an
/// answer, and a model that keeps asking for what it holds is told three times, then the turn
/// ends in words the person can read.
pub const HOLDS_BEFORE_STOP_ANSWERED: u8 = 3;
/// How many calls one turn may have held back, across all its calls, before it stops. This caps
/// A,B,A,B and wider cycles whose every call is stale.
pub const MOST_HOLDS_PER_TURN: u8 = 4;
/// How many replies in a row the planner may make that cannot be read as a call before the turn
/// stops: the first and the second are told what was wrong, the third asks the person.
pub const MOST_UNREADABLE: u8 = 3;
/// How many times running the same call, the app refuses it before the turn stops: the planner is
/// told why after the first, and the second ends the turn with the reason in words.
pub const MOST_DECLINED: u8 = 2;
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
    /// A read-only call returned a handle: the answer is held and asking again gets nothing new
    /// until something is written. (A reader ask that was answered is the same.)
    Answered,
    /// It cannot be compared (a journaled change, a handle from a call that may write, another
    /// refusal).
    Opaque,
}

impl Answer {
    fn stale(&self) -> Option<Held> {
        match self {
            Answer::Empty => Some(Held::Empty),
            Answer::Unchanged | Answer::Answered => Some(Held::Unchanged),
            Answer::Refused => Some(Held::Refused),
            Answer::Waiting(_) | Answer::Fresh | Answer::Opaque => None,
        }
    }
}

/// One distinct call of the turn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Seen {
    key: CallKey,
    /// The action; none for a reader ask.
    action: Option<ActionRef>,
    /// What the action does, when the planner's catalogue says; a call of unknown effect is
    /// taken to write.
    #[serde(default)]
    effect: Option<Effect>,
    answer: Answer,
    /// The last answer that could be compared.
    last: Option<Print>,
    /// How many times it has been held back since it last ran.
    held: u8,
    /// How many times in a row the app has refused it (not the person, not the router).
    #[serde(default)]
    declined: u8,
    /// What the app said of the last refusal.
    #[serde(default)]
    reason: String,
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
    /// The planner kept asking for what it already holds, though each time it was told so.
    Answered,
    /// The app refused the same call [`MOST_DECLINED`] times in a row; this is what it said.
    Declined(String),
}

impl Stuck {
    /// What the person is asked when the turn stops: what could not be found, or that the
    /// assistant is going round in circles, and what it needs from them.
    pub fn question(&self) -> String {
        // The person's words: never an action's id, which is a developer word.
        match self {
            Stuck::Repeating(Held::Empty, _) => "I couldn't find anything, and looking again won't change that. What should I look for instead?".to_owned(),
            Stuck::Repeating(Held::Unchanged, _) => "I keep getting the same answer. How would you like me to go on?".to_owned(),
            Stuck::Repeating(Held::Refused, _) => "That step keeps being refused. How would you like me to go on?".to_owned(),
            Stuck::Unreadable => "I keep writing steps I cannot get right, though I was told what was wrong. How would you like me to go on?".to_owned(),
            Stuck::Answered => "I could not finish this. I kept going back to what I already had. How would you like me to go on?".to_owned(),
            Stuck::Declined(reason) if reason.is_empty() => "Couldn't do that: the app wouldn't do it.".to_owned(),
            Stuck::Declined(reason) => format!("Couldn't do that: {reason}"),
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
    /// The reader ask in flight, until the reader answers it.
    #[serde(default)]
    reading: Option<CallKey>,
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

fn read_key_of(ask: &ReaderAsk) -> CallKey {
    let text =
        serde_json::to_string(&("read", &ask.inputs, &ask.task, &ask.want)).unwrap_or_default();
    CallKey(text)
}

fn nothing_in(value: &Value) -> bool {
    match value {
        Value::Text(text) => text.trim().is_empty(),
        Value::Entities(things) => things.is_empty(),
        _ => false,
    }
}

/// What an end came to, given the last comparable answer; the new last answer comes with it. A
/// handle returned by a call known to only read is an answer held; one from any other call
/// cannot be compared.
fn judged(end: &StepEnd, last: &Option<Print>, effect: Option<Effect>) -> (Answer, Option<Print>) {
    use docket_core::Reveal;
    match end {
        StepEnd::Done { said, value, undo } => {
            let plain = match value {
                Some(Reveal::Handle(_)) if effect == Some(Effect::Read) && undo.is_none() => {
                    return (Answer::Answered, last.clone());
                }
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

/// Remembers `entry`, forgetting the oldest beyond [`MOST_REMEMBERED`].
fn remember(seen: &mut Vec<Seen>, entry: Seen) {
    seen.push(entry);
    let excess = seen.len().saturating_sub(MOST_REMEMBERED);
    seen.drain(..excess);
}

impl Guard {
    /// Whether `call`, wanted as position `id` of a batch, is made, held back or stops the turn.
    /// `effect` is what the call's action does, when it is known.
    pub fn admit(self, id: CallId, call: &CallRequest, effect: Option<Effect>) -> (Guard, Verdict) {
        let key = key_of(call);
        let mut guard = self;
        let Some(at) = guard.seen.iter().position(|s| s.key == key) else {
            remember(
                &mut guard.seen,
                Seen {
                    key,
                    action: Some(call.action.clone()),
                    effect,
                    answer: Answer::Waiting(id),
                    last: None,
                    held: 0,
                    declined: 0,
                    reason: String::new(),
                },
            );
            return (guard, Verdict::Run);
        };
        let entry = &mut guard.seen[at];
        if entry.declined >= MOST_DECLINED {
            let stuck = Stuck::Declined(entry.reason.clone());
            return (guard, Verdict::Stop(stuck));
        }
        match entry.answer.stale() {
            None => {
                entry.answer = Answer::Waiting(id);
                entry.effect = effect;
                entry.held = 0;
                (guard, Verdict::Run)
            }
            Some(why) => guard.stale(at, why),
        }
    }

    /// Whether the reader ask is made, held back (it was answered already) or stops the turn.
    pub fn admit_read(self, ask: &ReaderAsk) -> (Guard, Verdict) {
        let key = read_key_of(ask);
        let mut guard = self;
        guard.reading = Some(key.clone());
        let Some(at) = guard.seen.iter().position(|s| s.key == key) else {
            remember(
                &mut guard.seen,
                Seen {
                    key,
                    action: None,
                    effect: None,
                    answer: Answer::Opaque,
                    last: None,
                    held: 0,
                    declined: 0,
                    reason: String::new(),
                },
            );
            return (guard, Verdict::Run);
        };
        match guard.seen[at].answer.stale() {
            None => (guard, Verdict::Run),
            Some(why) => {
                guard.reading = None;
                guard.stale(at, why)
            }
        }
    }

    /// The reader answered the ask in flight.
    pub fn read_answered(self) -> Guard {
        let Guard {
            seen,
            holds,
            unread,
            reading,
        } = self;
        let seen = seen
            .into_iter()
            .map(|s| match &reading {
                Some(key) if *key == s.key => Seen {
                    answer: Answer::Answered,
                    held: 0,
                    ..s
                },
                _ => s,
            })
            .collect();
        Guard {
            seen,
            holds,
            unread,
            reading: None,
        }
    }

    /// The entry at `at` is stale for `why`: held back with a note, or the turn stops.
    fn stale(self, at: usize, why: Held) -> (Guard, Verdict) {
        let mut guard = self;
        let entry = &mut guard.seen[at];
        let limit = match entry.answer {
            Answer::Answered => HOLDS_BEFORE_STOP_ANSWERED,
            _ => HOLDS_BEFORE_STOP,
        };
        if entry.held >= limit {
            let stuck = match (&entry.answer, &entry.action) {
                (Answer::Answered, _) | (_, None) => Stuck::Answered,
                (_, Some(action)) => Stuck::Repeating(why, action.clone()),
            };
            return (guard, Verdict::Stop(stuck));
        }
        if guard.holds >= MOST_HOLDS_PER_TURN {
            return (guard, Verdict::Stop(Stuck::Circling));
        }
        entry.held += 1;
        guard.holds = guard.holds.saturating_add(1);
        (guard, Verdict::Hold(why))
    }

    /// The call at position `id` ended. A call that wrote something makes the answers held from
    /// earlier reads of the world stale: they are asked again, once.
    pub fn ended(self, id: CallId, end: &StepEnd) -> Guard {
        let wrote = matches!(end, StepEnd::Done { .. })
            && self
                .seen
                .iter()
                .any(|s| s.answer == Answer::Waiting(id) && s.effect != Some(Effect::Read));
        let seen = self
            .seen
            .into_iter()
            .map(|s| match s.answer {
                Answer::Waiting(w) if w == id => {
                    let (answer, last) = judged(end, &s.last, s.effect);
                    let (declined, reason) = match end {
                        StepEnd::Refused(CallRefusal::App(refusal)) => {
                            (s.declined.saturating_add(1), declined_words(refusal))
                        }
                        _ => (0, String::new()),
                    };
                    Seen {
                        answer,
                        last,
                        declined,
                        reason,
                        ..s
                    }
                }
                Answer::Answered if wrote && s.action.is_some() => Seen {
                    answer: Answer::Opaque,
                    ..s
                },
                _ => s,
            })
            .collect();
        Guard { seen, ..self }
    }

    /// The call the app refused [`MOST_DECLINED`] times in a row, if any: the turn stops on it,
    /// with what the app said.
    pub fn declined(&self) -> Option<Stuck> {
        self.seen
            .iter()
            .find(|s| s.declined >= MOST_DECLINED)
            .map(|s| Stuck::Declined(s.reason.clone()))
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
