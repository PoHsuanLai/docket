//! May a confirmation offer "allow always"? One pure answer with the reason it is withheld, and
//! the same rule decides whether a grant already held may stand in for the ask. R1 lists what is
//! never grantable: untrusted-derived arguments into an outbound or non-undoable sink, an action
//! outside the task policy, a tripped breaker, an exhausted budget, and effect classes marked
//! never-grantable (permanent delete). For those the offer is simply absent.

use crate::grant::GrantCaller;
use crate::manifest::{AgentReach, ArgSink, UndoSupport};
use crate::review::AskReason;
use crate::standing::{CommandPrefix, NarrowState, StandingScope};
use crate::standing_match::{ArgFacts, CallFacts};
use prov::Effect;
use serde::{Deserialize, Serialize};

/// Where the session's breaker stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakerState {
    /// Taking calls.
    Running,
    /// Tripped: the session is paused until the person resumes it.
    Tripped,
}

/// Where the session's budgets stand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetState {
    /// Room for this call.
    Within,
    /// This call would exceed a budget.
    Over,
}

/// Why "allow always" is not offered (and a held grant does not skip the ask).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Withheld {
    /// This caller cannot hold a standing grant (only an editor or an ACP agent can).
    CallerCannotHold,
    /// The breaker is tripped.
    BreakerTripped,
    /// A budget is exhausted.
    OverBudget,
    /// The effect class is never grantable (permanent delete).
    NeverGrantable(Effect),
    /// The action asks every time, by its own declaration.
    AsksEveryTime,
    /// The call is outside the task the person's words set.
    OutsideTask,
    /// Untrusted content feeds an outbound or non-undoable sink.
    UntrustedIntoSink,
    /// Something else asks, and no standing grant lifts it (a mass act, another Space, a named
    /// policy rule, lasting memory, reviewers who disagreed).
    StillAsks(AskReason),
    /// The call has no path, command or recipient to scope a grant to.
    Unscoped,
    /// An argument that would be the scope could not be read, or the scope would be `/`.
    TooBroad,
    /// A grant already stands for this call, so the ask that remains is a reviewer's.
    AlreadyHeld,
}

/// What a confirmation may offer beyond "this once".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum AlwaysOffer {
    /// "Always" is offered, for exactly this scope.
    Offered(StandingScope),
    /// It is not, for this reason.
    Withheld(Withheld),
}

impl Default for AlwaysOffer {
    /// An older sheet carried no offer: none is made.
    fn default() -> Self {
        AlwaysOffer::Withheld(Withheld::CallerCannotHold)
    }
}

/// Everything the rule reads about one ask.
#[derive(Debug, Clone, Copy)]
pub struct AskFacts<'a> {
    /// The effect the call is gated on.
    pub effect: Effect,
    /// Whether the app can take it back.
    pub undo: UndoSupport,
    /// Whether the action asks every time.
    pub reach: AgentReach,
    /// Why the router asks.
    pub why: &'a [AskReason],
    /// The sinks that untrusted arguments feed.
    pub untrusted: &'a [ArgSink],
    /// The breaker.
    pub breaker: BreakerState,
    /// The budgets.
    pub budget: BudgetState,
}

/// Whether the effect or the lack of undo makes untrusted input dangerous.
fn risky(effect: Effect, undo: UndoSupport) -> bool {
    effect >= Effect::Outbound || undo == UndoSupport::NotUndoable
}

/// Whether the sink sends the value out of the machine.
fn leaves(sink: ArgSink) -> bool {
    matches!(
        sink,
        ArgSink::Recipient | ArgSink::Destination | ArgSink::Query
    )
}

fn reason(reason: &AskReason, effect: Effect, undo: UndoSupport) -> Result<(), Withheld> {
    match reason {
        AskReason::Effect(_) | AskReason::FirstUse | AskReason::FromTerminal => Ok(()),
        AskReason::AskAlways => Err(Withheld::AsksEveryTime),
        AskReason::OutsideTask => Err(Withheld::OutsideTask),
        AskReason::Tainted if risky(effect, undo) => Err(Withheld::UntrustedIntoSink),
        AskReason::Tainted => Ok(()),
        AskReason::UntrustedSink(sink) if risky(effect, undo) || leaves(*sink) => {
            Err(Withheld::UntrustedIntoSink)
        }
        AskReason::UntrustedSink(_) => Ok(()),
        AskReason::RuleOfTwo => Err(Withheld::UntrustedIntoSink),
        other @ (AskReason::CrossSpace
        | AskReason::Mass(_)
        | AskReason::LastingFromUntrusted
        | AskReason::Disagreement
        | AskReason::Rule(_)) => Err(Withheld::StillAsks(other.clone())),
    }
}

/// The first reason a standing grant cannot replace this ask, in a fixed order: breaker, budget,
/// effect class, the action's own rule, then each reason the router gave, then the labels.
pub fn blocker(f: &AskFacts<'_>) -> Option<Withheld> {
    if f.breaker == BreakerState::Tripped {
        return Some(Withheld::BreakerTripped);
    }
    if f.budget == BudgetState::Over {
        return Some(Withheld::OverBudget);
    }
    if f.effect == Effect::Destructive {
        return Some(Withheld::NeverGrantable(f.effect));
    }
    if f.reach == AgentReach::AskAlways {
        return Some(Withheld::AsksEveryTime);
    }
    if let Some(why) = f.why.iter().find_map(|r| reason(r, f.effect, f.undo).err()) {
        return Some(why);
    }
    f.untrusted
        .iter()
        .any(|sink| risky(f.effect, f.undo) || leaves(*sink))
        .then_some(Withheld::UntrustedIntoSink)
}

/// Whether a caller can hold standing grants: an editor or an ACP agent. The terminal, the
/// companion, computer use and apps have their own consent; this model leaves them as they are.
pub fn holds_standing(caller: &GrantCaller) -> bool {
    matches!(caller, GrantCaller::Editor(_) | GrantCaller::AcpAgent(_))
}

/// The narrowest scope that covers this call, or why there is none.
pub fn scope_for(call: &CallFacts) -> Result<StandingScope, Withheld> {
    let action = call.action.clone();
    let scope = match &call.args {
        ArgFacts::Paths(paths) => {
            let first = paths.first().ok_or(Withheld::Unscoped)?;
            let common = paths.iter().fold(first.clone(), |acc, p| acc.common(p));
            // A file is granted by the directory it sits in; a directory by itself.
            let under = if paths.len() == 1 {
                common.parent().unwrap_or(common)
            } else {
                common
            };
            StandingScope::Files { action, under }
        }
        ArgFacts::Command { line, cwd } => StandingScope::Terminal {
            action,
            command: command_prefix(line).ok_or(Withheld::TooBroad)?,
            cwd: cwd.clone(),
        },
        ArgFacts::Recipients(who) => match who.as_slice() {
            [one] => StandingScope::Outbound {
                action,
                to: one.clone(),
            },
            [] => return Err(Withheld::Unscoped),
            many => {
                let same = many.iter().all(|r| r == &many[0]);
                if !same {
                    return Err(Withheld::TooBroad);
                }
                StandingScope::Outbound {
                    action,
                    to: many[0].clone(),
                }
            }
        },
        ArgFacts::Unscoped => return Err(Withheld::Unscoped),
        ArgFacts::Opaque => return Err(Withheld::TooBroad),
    };
    match scope.narrow() {
        NarrowState::Narrow => Ok(scope),
        NarrowState::TooBroad => Err(Withheld::TooBroad),
    }
}

/// The program and its subcommand: `cargo test`, or just `ls` when the second word is a flag
/// or path.
fn command_prefix(line: &str) -> Option<CommandPrefix> {
    let mut words = line.split_whitespace();
    let first = words.next()?;
    let text = match words.next() {
        Some(second)
            if second
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
                && second
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') =>
        {
            format!("{first} {second}")
        }
        _ => first.to_owned(),
    };
    CommandPrefix::parse(&text).ok()
}

/// May this confirmation offer "allow always"? `Offered` carries the exact scope the grant would
/// have; `Withheld` says why not.
pub fn may_offer(caller: &GrantCaller, call: &CallFacts, f: &AskFacts<'_>) -> AlwaysOffer {
    if !holds_standing(caller) {
        return AlwaysOffer::Withheld(Withheld::CallerCannotHold);
    }
    if let Some(why) = blocker(f) {
        return AlwaysOffer::Withheld(why);
    }
    match scope_for(call) {
        Ok(scope) => AlwaysOffer::Offered(scope),
        Err(why) => AlwaysOffer::Withheld(why),
    }
}
