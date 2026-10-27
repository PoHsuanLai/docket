//! Memory comes to the planner. When the person types a request, the companion searches memory
//! for those words before the first planner call, and the answer is a step of the task like any
//! search the planner could have made: the same entities with the same labels, readable only
//! through the reader. The router rules on the call as on any other (budget, audit, taint); the
//! one difference is that it does not put a first-use sheet in front of a search the person did
//! not ask for (`Origin::TurnStart`). A memory that does not answer leaves the turn as it was.

use crate::drive::refusal_of;
use crate::runtime::Companion;
use crate::seams::Surface;
use docket_client::Transport as IntentsTransport;
use docket_core::Clock;
use docket_core::{
    ActionRef, Args, CallRequest, Origin, ParamName, RECALL_ACTION, RECALL_APP, TargetValue,
    UserTurn, Value,
};
use docket_planner::Planner;
use prov::{ActionName, Effect, Labelled, TaskId};

/// The longest search text the action takes (its declared limit), in characters.
const QUERY_CHARS: usize = 500;

/// The search of memory for what the person typed: one line, cut to what the action takes.
/// Nothing when there is nothing to search for.
pub(crate) fn recall_call(turn: &UserTurn) -> Option<CallRequest> {
    let line = turn.text.split_whitespace().collect::<Vec<_>>().join(" ");
    let query: String = line.chars().take(QUERY_CHARS).collect();
    if query.is_empty() {
        return None;
    }
    let mut args = Args::new();
    args.insert(
        ParamName::parse("query").ok()?,
        Labelled {
            value: Value::Text(query),
            label: docket_planner::planner_label(),
        },
    );
    Some(CallRequest {
        action: ActionRef {
            app: porter_core::AppName::parse(RECALL_APP).ok()?,
            name: ActionName::parse(RECALL_ACTION).ok()?,
        },
        target: TargetValue::Nothing,
        args,
        origin: Origin::TurnStart,
    })
}

impl<P: Planner, I: IntentsTransport, K: Clock, S: Surface> Companion<P, I, K, S> {
    /// Searches memory for the person's newest words and keeps the answer as a step of the task.
    /// The recall budget of the assembler is the switch: none, no search. A refusal, a router
    /// without memory or a search that fails leaves the task as it was.
    pub(crate) async fn prefetch(&mut self, task: &TaskId) {
        if self.config.assembler.recall.0 == 0 || self.running.contains(task) {
            return;
        }
        let Some(rt) = self.runtimes.get(task) else {
            return;
        };
        let Some(call) = rt.turns.last().and_then(recall_call) else {
            return;
        };
        let (session, window) = (rt.session.clone(), rt.window.clone());
        let performed = self
            .intents
            .perform(call.clone(), Some(session), window)
            .await
            .unwrap_or_else(|error| Err(refusal_of(error)));
        if let (Ok(outcome), Some(rt)) = (performed, self.runtimes.get_mut(task)) {
            rt.record_call(&call, Effect::Read, Ok(outcome));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docket_core::{TurnId, TurnSource, TurnVia};
    use prov::UnixSeconds;

    fn turn(text: &str) -> UserTurn {
        UserTurn {
            id: TurnId(1),
            text: text.to_owned(),
            at: UnixSeconds(0),
            from: TurnSource::Launcher,
            via: TurnVia::Typed,
            mode: None,
        }
    }

    #[test]
    fn the_search_is_the_persons_words_on_one_line_and_never_empty() {
        let table = [
            (
                "What is the capital of Taiwan?",
                Some("What is the capital of Taiwan?"),
            ),
            ("  line one\n\nline   two ", Some("line one line two")),
            ("   \n ", None),
        ];
        for (typed, want) in table {
            let call = recall_call(&turn(typed));
            let got = call.as_ref().and_then(|c| {
                c.args.values().next().and_then(|a| match &a.value {
                    Value::Text(t) => Some(t.as_str()),
                    _ => None,
                })
            });
            assert_eq!(got, want, "{typed:?}");
            assert!(call.iter().all(|c| c.origin == Origin::TurnStart));
        }
        let long = "x".repeat(QUERY_CHARS + 50);
        let call = recall_call(&turn(&long)).expect("a call");
        let Some(Value::Text(t)) = call.args.values().next().map(|a| a.value.clone()) else {
            panic!("text")
        };
        assert_eq!(t.chars().count(), QUERY_CHARS);
    }
}
