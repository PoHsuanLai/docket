//! Shared rows and helpers of the policy grid tests.
#![allow(dead_code)]

use docket_manifest::{AgentReach, Lasting};
use docket_permissions_core::{
    AskReason, Impact, Mode, ModeRows, Ruling, Saw, SessionSaw, SinkIntegrity,
};
use docket_session::Taint;
use docket_vocab::Origin;
use policy_point::*;
use porter_core::consent::Usage;
use porter_core::{AppName, Count, DataClass, Isolation};
use prov::{ActionName, ActorKind, Confidentiality, Effect, Integrity, SpaceId};
use std::collections::BTreeSet;

pub const CLEAN: SessionSaw = SessionSaw {
    private: Saw::NotSeen,
    untrusted: Saw::NotSeen,
};

/// A call that is trusted, inside the task policy, under Auto: the starting point
/// every row edits.
pub fn base(effect: Effect) -> PolicyRequest {
    let mail = AppName::parse("org.quire.Mail").expect("app");
    PolicyRequest {
        principal: PrincipalFacts {
            kind: ActorKind::Companion,
            caller: mail.clone(),
            isolation: Isolation::InProcess,
        },
        op: Op::Perform,
        resource: ActionFacts {
            app: mail,
            action: ActionName::parse("mail.message.send").expect("action"),
            effect,
            classes: BTreeSet::from([DataClass::Mail]),
            reach: AgentReach::Offered,
            lasting: Lasting::No,
        },
        context: PolicyContext {
            space: SpaceId::parse("work").expect("space"),
            target_space: SpaceRelation::Same,
            args: Integrity::Trusted,
            planner: Integrity::Trusted,
            confidentiality: Confidentiality::Public,
            usage: Usage::Interactive,
            origin: Origin::Companion,
            count: Count(1),
            mass_at: Count(20),
            grant: GrantState::Always,
            terminal: TerminalGrant::NotGranted,
            coverage: CoverageState::Inside,
            task_ceiling: Effect::Destructive,
            saw: CLEAN,
            sinks: SinkIntegrity {
                recipient: Integrity::Trusted,
                destination: Integrity::Trusted,
                body: Integrity::Trusted,
                path: Integrity::Trusted,
            },
            impact: Impact::High,
            halt: HaltFact::Running,
            budget: BudgetFact::Within,
            repeat: RepeatFact::Fresh,
            limit: LimitFact::Free,
            taint: Taint::Clean,
            run: RunFact::NotAsked,
            mode: Mode::Auto,
            rows: ModeRows::AUTO,
        },
    }
}

pub fn with(effect: Effect, edit: impl FnOnce(&mut PolicyRequest)) -> PolicyRequest {
    let mut request = base(effect);
    edit(&mut request);
    request
}

/// Puts the request in `mode`, with the rows that mode starts with.
pub fn in_mode(r: &mut PolicyRequest, mode: Mode) {
    r.context.mode = mode;
    r.context.rows = match mode {
        Mode::Auto => ModeRows::AUTO,
        Mode::Manual => ModeRows::MANUAL,
    };
}

/// The one built-in set, asked in one mode: `decide` puts each request in that mode first, so a
/// table can run the same rows under both.
pub struct InMode {
    pub pdp: Pdp,
    pub mode: Mode,
}

impl InMode {
    pub fn decide(&self, request: &PolicyRequest) -> Ruling {
        let mut request = request.clone();
        in_mode(&mut request, self.mode);
        self.pdp.decide(&request)
    }
}

/// The built-in set, asked in Auto.
pub fn pdp() -> InMode {
    InMode {
        pdp: Pdp::built_in().expect("loads"),
        mode: Mode::Auto,
    }
}

/// The built-in set, asked in Manual.
pub fn manual() -> InMode {
    InMode {
        pdp: Pdp::built_in().expect("loads"),
        mode: Mode::Manual,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    Final,
    Judged,
    Ask,
    Deny,
}

pub fn cell(ruling: &Ruling) -> Cell {
    match ruling {
        Ruling::AllowFinal(_) => Cell::Final,
        Ruling::AllowJudged(_) => Cell::Judged,
        Ruling::Ask(_) => Cell::Ask,
        Ruling::Deny(_) => Cell::Deny,
    }
}

pub fn asks(ruling: &Ruling) -> &[AskReason] {
    match ruling {
        Ruling::Ask(why) => why,
        Ruling::Deny(_) | Ruling::AllowJudged(_) | Ruling::AllowFinal(_) => &[],
    }
}

pub fn untrust_recipient(r: &mut PolicyRequest) {
    r.context.sinks.recipient = Integrity::Untrusted;
}

pub fn untrust_planner(r: &mut PolicyRequest) {
    r.context.planner = Integrity::Untrusted;
}

pub fn outside(r: &mut PolicyRequest) {
    r.context.coverage = CoverageState::Outside;
}

pub fn allowed_run(r: &mut PolicyRequest) {
    r.context.run = RunFact::Allowed;
}
