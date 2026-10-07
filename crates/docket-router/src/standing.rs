//! Standing grants in the router (acp-sessions.md section 12, R1). A grant replaces only the
//! confirmation step: it turns a call that would ask into one that goes to the reviewers, which
//! can still refuse or ask, after the halt, budget, repeat, consent and policy refusals have
//! already run. Whether it may do so, and whether "always" is offered, is the pure rule in
//! `docket_core`; this file reads the facts, carries out the decision and keeps the audit.

use crate::gate::Pending;
use crate::prepared::Prepared;
use crate::router::Router;
use crate::seams::{Clock, EventSink, GrantStore, Seams};
use crate::standing_facts::facts_of;
use action_review::plan;
use docket_core::{
    ActionDecl, AlwaysOffer, ArgSink, AskFacts, AskReason, AuditRecord, BreakerState, BudgetState,
    CallFacts, CallRequest, GrantCaller, Impact, Revocation, Ruling, StandingGrant,
    StandingGrantId, StandingScope, Withheld, blocker, find_standing, holds_standing, may_offer,
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
    /// The breaker as the call arrived.
    pub breaker: BreakerState,
    /// The budgets as the call arrived.
    pub budget: BudgetState,
    /// The grant that stood in for the ask, if one did.
    pub applied: Option<StandingGrantId>,
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
            untrusted,
            breaker,
            budget,
            applied: None,
        }
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
        }
    }

    /// The pending step after standing grants: an ask that a held grant may replace becomes a
    /// review at the plan the call's impact calls for; everything else is untouched. The
    /// reviewers' own ask still reaches the person.
    pub(crate) fn lifted(
        mut self,
        grants: &[StandingGrant],
        decl: &ActionDecl,
        impact: Impact,
        pending: Pending,
    ) -> (Pending, Self) {
        let Pending::Confirm(why) = &pending else {
            return (pending, self);
        };
        let held = find_standing(grants, &self.caller, &self.facts);
        match held {
            Some(grant) if blocker(&self.ask_facts(decl, why)).is_none() => {
                self.applied = Some(grant.id.clone());
                let stages = plan(&Ruling::AllowJudged(vec![]), impact);
                (Pending::NeedsReview(stages), self)
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

impl<S: Seams> Router<S> {
    /// The person answered "always" on a sheet that offered `scope`: it is held from now on.
    pub(crate) fn record_standing(&self, caller: GrantCaller, scope: StandingScope) {
        let at = self.seams.clock().now();
        let grant = StandingGrant::new(caller.clone(), scope, at);
        self.seams.sink().append(AuditRecord::StandingGranted {
            at,
            grant: grant.id.clone(),
            caller,
            kind: grant.scope.kind(),
            action: grant.scope.action().clone(),
        });
        self.seams.grants().add_standing(grant);
    }

    /// The standing grants held: what Settings lists.
    pub fn standing_grants(&self) -> Vec<StandingGrant> {
        self.seams.grants().standing()
    }

    /// Revokes a standing grant: the next call it would have covered asks again.
    pub fn revoke_standing(&self, id: &StandingGrantId) -> Revocation {
        let done = self.seams.grants().revoke_standing(id);
        if done == Revocation::Revoked {
            self.seams.sink().append(AuditRecord::StandingRevoked {
                at: self.seams.clock().now(),
                grant: id.clone(),
            });
        }
        done
    }

    /// Just before dispatch: a call that skipped its ask on a grant finds the grant still held
    /// (a revocation in the meantime counts), and the use is audited. Nothing to do for a call
    /// the person answered. `Err` means the grant is gone and the call must not run.
    pub(crate) fn standing_use(&self, p: &Prepared, answered: bool) -> Result<(), ()> {
        let (Some(ctx), false) = (&p.standing, answered) else {
            return Ok(());
        };
        let Some(id) = &ctx.applied else {
            return Ok(());
        };
        if !self.standing_grants().iter().any(|g| g.id == *id) {
            return Err(());
        }
        self.seams.sink().append(AuditRecord::StandingUsed {
            at: self.seams.clock().now(),
            call: p.id,
            grant: id.clone(),
            caller: ctx.caller.clone(),
        });
        Ok(())
    }
}
