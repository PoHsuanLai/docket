//! Standing grants in the router (acp-sessions.md section 12, R1). A grant replaces only the
//! confirmation step: it turns a call that would ask into one that goes to the reviewers, which
//! can still refuse or ask, after the halt, budget, repeat, consent and policy refusals have
//! already run. Whether it may do so, and whether "always" is offered, is the pure rule in
//! `docket_core`; this file reads the facts, carries out the decision and keeps the audit.

use crate::prepared::Prepared;
use crate::router::Router;
use crate::seams::{GrantStore, Seams};
use docket_core::EventSink;
use docket_core::{
    AuditRecord, CallFacts, GrantCaller, Revocation, StandingGrant, StandingGrantId, StandingScope,
};
use docket_vocab::Clock;

impl<S: Seams> Router<S> {
    /// Spends the approval a call ran on: gone if it was spent in the meantime (the call must not
    /// run), and audited when it was used.
    fn approval_use(&self, p: &Prepared, approval: &CallFacts) -> Result<(), ()> {
        let spent = {
            let mut st = self.locked();
            p.who
                .session
                .as_ref()
                .and_then(|s| st.sessions.get_mut(s))
                .and_then(|r| {
                    let at = r.approvals.iter().position(|a| a == approval)?;
                    Some(r.approvals.remove(at))
                })
        };
        spent
            .map(|_| {
                self.seams.sink().append(AuditRecord::ApprovalUsed {
                    at: self.seams.clock().now(),
                    call: p.id,
                    action: p.request.action.clone(),
                });
            })
            .ok_or(())
    }

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
            // It may have been a rule: the policy then drops it at once.
            self.reload_rules();
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
        if let Some(approval) = &ctx.approved {
            return self.approval_use(p, approval);
        }
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
