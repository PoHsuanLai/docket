//! The `Execute` rules: when a terminal command runs, asks, or is refused (acp-sessions.md
//! section 7, S8). Pure: the facts come in, one ruling comes out.
//!
//! `prov::Effect` is porter's frozen enum and has no `Execute`, so docket gates the effect as
//! [`EXECUTE_AS`], which is `Outbound`: the severity at which untrusted input into the call is
//! never grantable. The rules, in the order they bite:
//!
//! 1. A reviewer's `Deny` refuses. A reviewer can only tighten: its `Ask` makes a command ask even
//!    where a standing grant stands, and its `Allow` changes nothing.
//! 2. A command that cannot run in the sandbox asks, with the reason, and no "always" is offered.
//! 3. A command whose arguments derive from untrusted content asks, and no "always" is offered.
//! 4. A tripped breaker or an exhausted budget asks, and no "always" is offered.
//! 5. A standing grant with a terminal scope (command prefix and working directory) of the same
//!    caller stands in for the confirmation, and only because the command runs in the sandbox.
//! 6. Otherwise the person is asked, with the "always" offer `may_offer` makes.

use crate::grant::GrantCaller;
use crate::manifest::{AgentReach, UndoSupport};
use crate::review::{AskReason, VerdictKind};
use crate::standing::{StandingGrant, StandingGrantId};
use crate::standing_match::{CallFacts, find_standing};
use crate::standing_offer::{
    AlwaysOffer, AskFacts, BreakerState, BudgetState, Withheld, holds_standing, may_offer,
};
use prov::Effect;
use serde::{Deserialize, Serialize};

/// The effect class an `Execute` call is gated as.
pub const EXECUTE_AS: Effect = Effect::Outbound;

/// Why a command cannot run in the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum CannotSandbox {
    /// No sandbox program (`bwrap`) is installed.
    #[error("no sandbox program is installed")]
    NotInstalled,
    /// The program is there but the kernel refuses it (user namespaces are off).
    #[error("the kernel refuses unprivileged sandboxes here")]
    NamespacesDenied,
    /// This platform has no sandbox backend.
    #[error("this platform has no sandbox")]
    Unsupported,
    /// The working directory cannot be the one writable place (`/`, or it is not a directory).
    #[error("the working directory cannot be sandboxed")]
    BadWorkingDir,
}

/// Whether this command can run in the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SandboxState {
    /// It can.
    Ready,
    /// It cannot, for this reason.
    Cannot(CannotSandbox),
}

/// Where a command's arguments came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgOrigin {
    /// The person's words, or a trusted source.
    Typed,
    /// Derived from untrusted content (a file, a page, a message, a tool result).
    Untrusted,
}

/// Why the person is asked.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ExecuteAsk {
    /// The default: every command asks.
    Confirm,
    /// It cannot run in the sandbox.
    CannotSandbox(CannotSandbox),
    /// Its arguments derive from untrusted content.
    UntrustedArgs,
    /// A reviewer asked.
    Reviewer,
    /// The breaker is tripped or a budget is spent.
    Limits,
}

/// What the gate rules for one command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ExecuteRuling {
    /// Run it: this standing grant stands in for the confirmation.
    Run(StandingGrantId),
    /// Ask the person, offering this.
    Ask {
        /// Why.
        why: ExecuteAsk,
        /// Whether "always" may be offered.
        offer: AlwaysOffer,
    },
    /// Refuse: a reviewer said no.
    Deny,
}

/// Everything the rule reads about one command.
#[derive(Debug, Clone, Copy)]
pub struct ExecuteFacts<'a> {
    /// Who runs it.
    pub caller: &'a GrantCaller,
    /// The command and where it runs.
    pub call: &'a CallFacts,
    /// Whether it can run in the sandbox.
    pub sandbox: SandboxState,
    /// Where its arguments came from.
    pub origin: ArgOrigin,
    /// The breaker.
    pub breaker: BreakerState,
    /// The budgets.
    pub budget: BudgetState,
    /// The reviewer's verdict, if one ran.
    pub review: Option<VerdictKind>,
}

fn ask(why: ExecuteAsk, no: Withheld) -> ExecuteRuling {
    ExecuteRuling::Ask {
        why,
        offer: AlwaysOffer::Withheld(no),
    }
}

/// The ruling for one command, given the grants the caller holds.
pub fn rule_execute(f: &ExecuteFacts<'_>, grants: &[StandingGrant]) -> ExecuteRuling {
    if f.review == Some(VerdictKind::Deny) {
        return ExecuteRuling::Deny;
    }
    if let SandboxState::Cannot(why) = f.sandbox {
        return ask(ExecuteAsk::CannotSandbox(why), Withheld::CannotSandbox(why));
    }
    if f.origin == ArgOrigin::Untrusted {
        return ask(ExecuteAsk::UntrustedArgs, Withheld::UntrustedIntoSink);
    }
    if f.breaker == BreakerState::Tripped || f.budget == BudgetState::Over {
        let no = match f.breaker {
            BreakerState::Tripped => Withheld::BreakerTripped,
            BreakerState::Running => Withheld::OverBudget,
        };
        return ask(ExecuteAsk::Limits, no);
    }
    let held = holds_standing(f.caller)
        .then(|| find_standing(grants, f.caller, f.call))
        .flatten();
    match (held, f.review) {
        (Some(grant), None | Some(VerdictKind::Allow)) => ExecuteRuling::Run(grant.id.clone()),
        (Some(_), _) => ask(ExecuteAsk::Reviewer, Withheld::AlreadyHeld),
        (None, review) => {
            let why = if review == Some(VerdictKind::Ask) {
                ExecuteAsk::Reviewer
            } else {
                ExecuteAsk::Confirm
            };
            ExecuteRuling::Ask {
                why,
                offer: offer(f, review),
            }
        }
    }
}

/// The "always" offer for a command that is going to ask anyway.
fn offer(f: &ExecuteFacts<'_>, review: Option<VerdictKind>) -> AlwaysOffer {
    let why = [AskReason::Effect(EXECUTE_AS)];
    let tightened = [AskReason::Disagreement];
    let facts = AskFacts {
        effect: EXECUTE_AS,
        undo: UndoSupport::NotUndoable,
        reach: AgentReach::Offered,
        // A reviewer's ask is a tightening: it is the person's decision this time, not a rule
        // a grant may later skip.
        why: if review == Some(VerdictKind::Ask) {
            &tightened
        } else {
            &why
        },
        untrusted: &[],
        breaker: f.breaker,
        budget: f.budget,
    };
    may_offer(f.caller, f.call, &facts)
}
