//! Per-call effect, carried out: for an action that opts in (`per_call = "classified"`), ask the
//! provider what this call does, then prepare the call on the effect the pure
//! [`classify_step`] allows. The declared effect is the ceiling; a provider that fails, times
//! out or has no `Classify` leaves the ceiling in force.

use crate::classify::{at_ceiling, classify_step, delegation_end};
use crate::driven::Driven;
use crate::prepared::{Early, Prepared};
use crate::router::Router;
use crate::seams::{AppLink, Clock, EventSink, Seams};
use crate::who::Who;
use docket_core::{
    AppRefusal, AuditRecord, CallClass, CallEnd, CallRefusal, CallRequest, Classification, Depth,
    PerCall, WindowKey,
};

impl<S: Seams> Router<S> {
    /// Prepares a call and, when its action classifies per call and an agent made it, asks the
    /// provider first. The person's own calls are not gated, so they are not classified.
    pub(crate) async fn prepare_classified(
        &self,
        who: &Who,
        request: CallRequest,
        window: Option<WindowKey>,
        depth: Depth,
    ) -> Result<Prepared, Box<Early>> {
        let first = self.prepare(who, request.clone(), window.clone(), depth)?;
        if first.decl.per_call != PerCall::Classified || who.session.is_none() {
            return Ok(first);
        }
        let answer = self
            .seams
            .link()
            .classify(&first.request.action.app, self.invocation(&first))
            .await;
        let class = classify_step(first.decl.effect, &first.request.action, answer, |a| {
            self.locked().registry.action(a).is_some()
        });
        self.note_classified(&first, &class);
        self.prepare_as(Some(first.id), Some(class), who, request, window, depth)
    }

    /// The same call again, gated at the declared ceiling: what follows `ClassificationChanged`.
    /// The call gets a new id; the first one ended, and its audit record says why.
    pub(crate) fn prepare_at_ceiling(
        &self,
        was: &Prepared,
        window: Option<WindowKey>,
        depth: Depth,
    ) -> Result<Prepared, Box<Early>> {
        let ceiling = was
            .classified
            .as_ref()
            .map_or(was.decl.effect, |c| c.ceiling);
        let retry = at_ceiling(ceiling);
        let again = self.prepare_as(
            None,
            Some(retry.clone()),
            &was.who,
            was.request.clone(),
            window,
            depth,
        )?;
        self.note_classified(&again, &retry);
        Ok(again)
    }

    fn note_classified(&self, p: &Prepared, classification: &Classification) {
        self.seams.sink().append(AuditRecord::Classified {
            at: self.seams.clock().now(),
            call: p.id,
            action: p.request.action.clone(),
            classification: classification.clone(),
        });
    }

    /// Whether the app said, at `Perform`, that the call is no longer what was classified, and
    /// a retry at the ceiling is therefore due (it is not, if the call already ran at it).
    pub(crate) fn changed_since_classified(p: &Prepared, driven: &Driven) -> bool {
        let reduced = p
            .classified
            .as_ref()
            .is_some_and(|c| c.sent != CallClass::Effect(c.ceiling));
        reduced
            && matches!(
                driven.end,
                CallEnd::Refused(CallRefusal::App(AppRefusal::ClassificationChanged))
            )
    }

    /// Audits how a delegation ended (a call that was gated as `Read` because it forwards to
    /// another action). Only a call that ran has one.
    pub(crate) fn note_delegation(&self, p: &Prepared, driven: &Driven) {
        let (
            Some(Classification {
                sent: CallClass::Delegates(to),
                ..
            }),
            CallEnd::Done,
            Some(outcome),
        ) = (&p.classified, &driven.end, &driven.outcome)
        else {
            return;
        };
        self.seams.sink().append(AuditRecord::Delegation {
            at: self.seams.clock().now(),
            call: p.id,
            to: to.clone(),
            end: delegation_end(to, outcome),
        });
    }
}
