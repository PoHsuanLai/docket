//! Which calls a standing grant covers: the pure key match. The facts are what the router read
//! off the call's typed arguments; a fact it could not read (a held handle, a relative path) is
//! `Opaque`, which no scope covers.

use crate::agent_app::is_agent_action;
use crate::grant::GrantCaller;
use crate::ids::ActionRef;
use crate::standing::{AbsPath, Cover, Recipient, ScopeKind, StandingGrant, StandingScope};
use prov::Effect;
use serde::{Deserialize, Serialize};

/// The arguments of a call, reduced to what a scope can be compared with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ArgFacts {
    /// Every path the call writes or removes.
    Paths(Vec<AbsPath>),
    /// A terminal command and where it runs.
    Command {
        /// The command line as given.
        line: String,
        /// The working directory.
        cwd: AbsPath,
    },
    /// Every recipient or host the call sends to.
    Recipients(Vec<Recipient>),
    /// Nothing a scope can name (the action takes things, not paths or recipients).
    Unscoped,
    /// An argument that should have been a path, command or recipient could not be read.
    Opaque,
}

/// What a call is, for matching.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallFacts {
    /// The action called.
    pub action: ActionRef,
    /// Its arguments, reduced.
    pub args: ArgFacts,
}

impl StandingScope {
    /// Whether this scope covers the call: the same action, and every argument inside the scope.
    /// A call with no path, command or recipient is never covered by a scope that names one.
    pub fn covers(&self, call: &CallFacts) -> Cover {
        if self.action() != &call.action {
            return Cover::Misses;
        }
        match (self, &call.args) {
            (StandingScope::Files { under, .. }, ArgFacts::Paths(paths)) => {
                all_cover(paths.iter().map(|p| under.covers(p)))
            }
            (StandingScope::Terminal { command, cwd, .. }, ArgFacts::Command { line, cwd: at }) => {
                all_cover([command.covers(line), cwd.covers(at)])
            }
            (StandingScope::Outbound { to, .. }, ArgFacts::Recipients(who)) => {
                all_cover(who.iter().map(|r| to.covers(r)))
            }
            (StandingScope::Reads { action }, _) if !is_agent_action(action) => Cover::Covers,
            _ => Cover::Misses,
        }
    }
}

/// `Covers` when there is at least one item and every item covers.
fn all_cover(items: impl IntoIterator<Item = Cover>) -> Cover {
    let mut any = Cover::Misses;
    for item in items {
        if item == Cover::Misses {
            return Cover::Misses;
        }
        any = Cover::Covers;
    }
    any
}

impl StandingGrant {
    /// Whether this grant covers the call made by `caller`. A grant belongs to one caller only.
    pub fn covers(&self, caller: &GrantCaller, call: &CallFacts) -> Cover {
        if &self.caller == caller {
            self.scope.covers(call)
        } else {
            Cover::Misses
        }
    }
}

/// The first of `grants` that covers the call made by `caller` to an action of this `effect`. A
/// reads grant stands only for a read: if the action ever declares more, it covers nothing.
pub fn find_standing_for<'a>(
    grants: &'a [StandingGrant],
    caller: &GrantCaller,
    call: &CallFacts,
    effect: Effect,
) -> Option<&'a StandingGrant> {
    grants.iter().find(|g| {
        let fits = effect == Effect::Read || g.scope.kind() != ScopeKind::Reads;
        fits && g.covers(caller, call) == Cover::Covers
    })
}
