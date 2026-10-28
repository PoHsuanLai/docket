//! Which agent-app helpers are signed in, and the ways of signing in the app listed for those
//! that are not. An app has one login, so every helper that runs the same app shares it.

use companion_model::helper::{Board, HelperState, Need, SignInWay};
use prov::{AgentProgram, HelperId};
use std::collections::{BTreeMap, BTreeSet};

/// What is known of the helpers' logins. Known only from the app, and not kept across a restart.
#[derive(Debug, Default)]
pub struct Logins {
    /// For a helper whose agent app wants signing in: the ways the app listed, by its own names.
    ways: BTreeMap<HelperId, Vec<SignInWay>>,
    /// The helpers whose agent app opened a session, so is signed in.
    signed: BTreeSet<HelperId>,
}

impl Logins {
    /// The ways the app listed for `helper`, none when it listed none.
    pub fn ways_of(&self, helper: &HelperId) -> Vec<SignInWay> {
        self.ways.get(helper).cloned().unwrap_or_default()
    }

    /// The way `helper`'s app listed under the id `way`.
    pub fn way(&self, helper: &HelperId, way: &str) -> Option<SignInWay> {
        self.ways.get(helper)?.iter().find(|w| w.id == way).cloned()
    }

    /// Whether `helper`'s app is known to be signed in.
    pub fn is_signed(&self, helper: &HelperId) -> bool {
        self.signed.contains(helper)
    }

    /// `helper`'s app wants the person to sign in, in one of `ways`.
    pub fn wants_sign_in(&mut self, helper: &HelperId, ways: Vec<SignInWay>) {
        self.signed.remove(helper);
        self.ways.insert(helper.clone(), ways);
    }

    /// These helpers' app is signed in: what it listed is no longer asked for.
    pub fn signed_in(&mut self, helpers: impl IntoIterator<Item = HelperId>) {
        for id in helpers {
            self.ways.remove(&id);
            self.signed.insert(id);
        }
    }

    /// The person signed out of the app these helpers run.
    pub fn signed_out(&mut self, helpers: impl IntoIterator<Item = HelperId>) {
        for id in helpers {
            self.signed.remove(&id);
        }
    }

    /// `helper` is gone: nothing is known of its login.
    pub fn forget(&mut self, helper: &HelperId) {
        self.signed.remove(helper);
        self.ways.remove(helper);
    }
}

/// The helpers that run the same agent app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SameApp {
    /// All of them.
    pub all: Vec<HelperId>,
    /// Those that wait for the person to sign in.
    pub waiting: Vec<HelperId>,
}

/// The helpers on `board` that run `program`.
pub fn same_app(board: &Board, program: &AgentProgram) -> SameApp {
    let same: Vec<_> = board
        .cards()
        .iter()
        .filter(|card| card.kind.program() == Some(program))
        .collect();
    SameApp {
        all: same.iter().map(|card| card.id.clone()).collect(),
        waiting: same
            .iter()
            .filter(|card| card.state == HelperState::Waiting(Need::SignIn))
            .map(|card| card.id.clone())
            .collect(),
    }
}
