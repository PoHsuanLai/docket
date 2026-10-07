//! Shared rows and helpers of the policy grid tests.
#![allow(dead_code)]

use docket_core::*;
use policy_point::*;
use porter_core::consent::Usage;
use porter_core::{AppName, Count, DataClass, Isolation};
use prov::{ActionName, ActorKind, Confidentiality, Effect, Integrity, SpaceId};
use std::collections::BTreeSet;

pub const CLEAN: SessionSaw = SessionSaw {
    private: Saw::NotSeen,
    untrusted: Saw::NotSeen,
};

/// A call that is trusted, inside the task policy, in Default strictness: the starting point
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
            strictness: Strictness::Default,
            saw: CLEAN,
            sinks: SinkIntegrity {
                recipient: Integrity::Trusted,
                destination: Integrity::Trusted,
                body: Integrity::Trusted,
                path: Integrity::Trusted,
            },
            impact: Impact::High,
        },
    }
}

pub fn with(effect: Effect, edit: impl FnOnce(&mut PolicyRequest)) -> PolicyRequest {
    let mut request = base(effect);
    edit(&mut request);
    request
}

pub fn pdp() -> Pdp {
    Pdp::standard().expect("loads")
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

pub fn strictness(s: Strictness) -> impl FnOnce(&mut PolicyRequest) {
    move |r| r.context.strictness = s
}
