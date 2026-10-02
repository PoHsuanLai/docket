//! The decision grid, one row per cell of addendum A2, waits for `Pdp::decide` and the permit
//! rules of the default policy set (FINDINGS: policy-point fill).

use docket_core::*;
use policy_point::*;
use porter_core::consent::Usage;
use porter_core::{AppName, Count, DataClass, Isolation};
use prov::{ActionName, ActorKind, Confidentiality, Effect, Integrity, SpaceId};
use std::collections::BTreeSet;

struct Row {
    name: &'static str,
    effect: Effect,
    saw: SessionSaw,
    sinks: Integrity,
    coverage: CoverageState,
    strictness: Strictness,
    planner: Integrity,
    want: fn(&Ruling) -> bool,
}

fn request(row: &Row) -> PolicyRequest {
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
            effect: row.effect,
            classes: BTreeSet::from([DataClass::Mail]),
            reach: AgentReach::Offered,
            lasting: Lasting::No,
        },
        context: PolicyContext {
            space: SpaceId::parse("work").expect("space"),
            target_space: SpaceRelation::Same,
            args: row.sinks,
            planner: row.planner,
            confidentiality: Confidentiality::Public,
            usage: Usage::Interactive,
            origin: Origin::Companion,
            count: Count(1),
            mass_at: Count(20),
            grant: GrantState::Always,
            coverage: row.coverage,
            task_ceiling: Effect::Outbound,
            strictness: row.strictness,
            saw: row.saw,
            sinks: SinkIntegrity {
                recipient: row.sinks,
                destination: Integrity::Trusted,
                body: Integrity::Trusted,
                path: Integrity::Trusted,
            },
            impact: Impact::High,
        },
    }
}

#[test]
#[ignore = "Pdp::decide and the grid's permit rules are the policy-point fill"]
fn default_policy_grid() {
    let seen = SessionSaw {
        private: Saw::Seen,
        untrusted: Saw::Seen,
    };
    let clean = SessionSaw {
        private: Saw::NotSeen,
        untrusted: Saw::NotSeen,
    };
    let rows = [
        Row {
            name: "rule of two asks in every strictness",
            effect: Effect::Outbound,
            saw: seen,
            sinks: Integrity::Trusted,
            coverage: CoverageState::Inside,
            strictness: Strictness::TrustMore,
            planner: Integrity::Trusted,
            want: |r| matches!(r, Ruling::Ask(why) if why.contains(&AskReason::RuleOfTwo)),
        },
        Row {
            name: "an untrusted recipient asks even inside the task policy",
            effect: Effect::Outbound,
            saw: clean,
            sinks: Integrity::Untrusted,
            coverage: CoverageState::Inside,
            strictness: Strictness::TrustMore,
            planner: Integrity::Trusted,
            want: |r| matches!(r, Ruling::Ask(why) if why.contains(&AskReason::UntrustedSink(ArgSink::Recipient))),
        },
        Row {
            name: "outbound, trusted and inside, is judged by default",
            effect: Effect::Outbound,
            saw: clean,
            sinks: Integrity::Trusted,
            coverage: CoverageState::Inside,
            strictness: Strictness::Default,
            planner: Integrity::Trusted,
            want: |r| matches!(r, Ruling::AllowJudged(_)),
        },
        Row {
            name: "outbound outside the task policy asks",
            effect: Effect::Outbound,
            saw: clean,
            sinks: Integrity::Trusted,
            coverage: CoverageState::Outside,
            strictness: Strictness::Default,
            planner: Integrity::Trusted,
            want: |r| matches!(r, Ruling::Ask(_)),
        },
        Row {
            name: "a read in the same Space is final",
            effect: Effect::Read,
            saw: clean,
            sinks: Integrity::Trusted,
            coverage: CoverageState::Inside,
            strictness: Strictness::Default,
            planner: Integrity::Trusted,
            want: |r| matches!(r, Ruling::AllowFinal(_)),
        },
    ];
    let pdp = Pdp::standard().expect("loads");
    for row in &rows {
        let got = pdp.decide(&request(row));
        assert!((row.want)(&got), "row {}: {got:?}", row.name);
    }
}
