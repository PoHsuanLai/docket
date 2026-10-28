//! What the router knows about standing grants for one call, and how a held grant or an
//! approved permission request lifts an ask. The pure part of `standing`: facts in, decision out.

use crate::gate::Pending;
use crate::standing_facts::{exec_of, facts_of};
use action_review::plan;
use docket_core::{
    ActionDecl, AlwaysOffer, ArgSink, AskFacts, AskReason, BreakerState, BudgetState, CallFacts,
    CallRequest, ExecFacts, GrantCaller, Impact, Limited, Ruling, StandingGrant, StandingGrantId,
    Withheld, blocker, covers_approval, find_standing_for, holds_standing, may_offer,
};
use prov::Integrity;

/// What the router knows about standing grants for one call.
#[derive(Debug, Clone)]
pub(crate) struct StandingCtx {
    /// Who is calling.
    pub caller: GrantCaller,
    /// What the call is, for matching.
    pub facts: CallFacts,
    /// The sinks fed by untrusted arguments.
    pub untrusted: Vec<ArgSink>,
    /// What the host said of a terminal command; none for any other call.
    pub exec: Option<ExecFacts>,
    /// The breaker as the call arrived.
    pub breaker: BreakerState,
    /// The budgets as the call arrived.
    pub budget: BudgetState,
    /// The grant that stood in for the ask, if one did.
    pub applied: Option<StandingGrantId>,
    /// The permission request the person said yes to that stood in for the ask, if one did.
    pub approved: Option<CallFacts>,
    /// The limit that covers the call, if one does: the reason it asks, or the reason it was
    /// refused. No grant or approval stands in for the question while one does.
    pub limited: Limited,
}

impl StandingCtx {
    /// The context for `request` made by `caller`.
    pub(crate) fn new(
        caller: GrantCaller,
        decl: &ActionDecl,
        request: &CallRequest,
        breaker: BreakerState,
        budget: BudgetState,
    ) -> Self {
        let untrusted = decl
            .params
            .iter()
            .filter(|d| d.sink != ArgSink::Inert)
            .filter(|d| {
                request
                    .args
                    .get(&d.name)
                    .is_some_and(|a| a.label.integrity == Integrity::Untrusted)
            })
            .map(|d| d.sink)
            .collect();
        Self {
            caller,
            facts: facts_of(decl, request),
            exec: exec_of(request),
            untrusted,
            breaker,
            budget,
            applied: None,
            approved: None,
            limited: Limited::Free,
        }
    }

    /// The same context under `limited`.
    pub(crate) fn limited(self, limited: Limited) -> Self {
        Self { limited, ..self }
    }

    fn ask_facts<'a>(&'a self, decl: &ActionDecl, why: &'a [AskReason]) -> AskFacts<'a> {
        AskFacts {
            effect: decl.effect,
            undo: decl.undo,
            reach: decl.reach,
            why,
            untrusted: &self.untrusted,
            breaker: self.breaker,
            budget: self.budget,
            exec: self.exec,
        }
    }

    /// The pending step after standing grants: an ask that a held grant may replace becomes a
    /// review at the plan the call's impact calls for; everything else is untouched. The
    /// reviewers' own ask still reaches the person.
    pub(crate) fn lifted(
        mut self,
        grants: &[StandingGrant],
        approvals: &[CallFacts],
        decl: &ActionDecl,
        impact: Impact,
        pending: Pending,
    ) -> (Pending, Self) {
        let Pending::Confirm(why) = &pending else {
            return (pending, self);
        };
        let held = find_standing_for(grants, &self.caller, &self.facts, decl.effect);
        let stages = plan(&Ruling::AllowJudged(vec![]), impact);
        match held {
            Some(grant) if blocker(&self.ask_facts(decl, why)).is_none() => {
                self.applied = Some(grant.id.clone());
                return (Pending::NeedsReview(stages), self);
            }
            _ => {}
        }
        // A permission request the person already said yes to covers the very thing it named,
        // once, unless the breaker or a budget says no (a yes is not a way round either). It is
        // the person's own answer to this call, so the call runs as it would on the sheet's yes,
        // which no reviewer follows; a grant, which is only standing, is reviewed.
        let open = self.breaker == BreakerState::Running && self.budget == BudgetState::Within;
        match approvals.iter().find(|a| covers_approval(a, &self.facts)) {
            Some(approval) if open => {
                self.approved = Some(approval.clone());
                (Pending::Run, self)
            }
            _ => (pending, self),
        }
    }

    /// Whether the sheet for `why` offers "allow always", and for what.
    pub(crate) fn offer(&self, decl: &ActionDecl, why: &[AskReason]) -> AlwaysOffer {
        match &self.applied {
            Some(_) => AlwaysOffer::Withheld(Withheld::AlreadyHeld),
            None => may_offer(&self.caller, &self.facts, &self.ask_facts(decl, why)),
        }
    }

    /// Whether this caller's "always" is a standing grant rather than the broad class grant.
    pub(crate) fn holds(&self) -> bool {
        holds_standing(&self.caller)
    }
}
