//! What a call shows the person and how their answer is taken: the app's dry run, the
//! invocation an app gets, the sheet, and a sheet's answer borrowed by a waiting call.

use crate::router::Router;
use crate::seams::{AppLink, Seams};
use docket_context::asks::{AskKey, Seat, Settled, seat, settled};
use docket_core::{
    AppRefusal, AuditRecord, ConfirmAnswer, ConfirmAnswerKind, ConfirmEnd, ConfirmId,
    ConfirmRequest, Confirmer, EventSink, Invocation,
};
use docket_gate::brief_sheet::brief_answer;
use docket_gate::call::CallEvent;
use docket_gate::confirm::confirm_request;
use docket_gate::driven::{Next, Run};
use docket_gate::gate::Pending;
use docket_gate::prepared::Prepared;
use docket_permissions_core::{AskReason, PolicyId};
use docket_vocab::Clock;

impl<S: Seams> Router<S> {
    pub(crate) async fn dry_run(&self, p: &Prepared, run: &mut Run) -> Next {
        let answer = match p.decl.dry_run {
            docket_manifest::DryRun::None => Err(AppRefusal::Unsupported),
            docket_manifest::DryRun::Preview => {
                self.seams
                    .link()
                    .dry_run(&p.request.action.app, self.invocation(p))
                    .await
            }
        };
        run.preview = answer.as_ref().ok().cloned();
        match self.sheet_for(p, run) {
            Some(request) => Next::Event(CallEvent::Previewed(Box::new(request))),
            None => Next::Event(CallEvent::Answered(ConfirmAnswer::Ended(
                ConfirmEnd::Cancelled,
            ))),
        }
    }

    pub(crate) fn invocation(&self, p: &Prepared) -> Invocation {
        Invocation {
            call: p.id,
            action: p.decl.name.clone(),
            target: p.request.target.clone(),
            // What the caller sent, then what the router filled (`check_call` kept the two apart).
            args: p
                .request
                .args
                .iter()
                .chain(&p.filled)
                .map(|(name, arg)| (name.clone(), arg.clone()))
                .collect(),
            actor: p.who.actor.clone(),
            origin: p.request.origin,
            space: p.space.clone(),
        }
    }

    /// The sheet for a call that goes to the person: its reasons, the preview the app gave (or
    /// the plain argument lines), and a freshly minted id.
    pub(crate) fn sheet_for(&self, p: &Prepared, run: &Run) -> Option<ConfirmRequest> {
        let mut st = self.locked();
        let n = st.mint();
        let id = ConfirmId::parse(&format!("c-{n}")).ok()?;
        let record = p.who.session.as_ref().and_then(|s| st.sessions.get(s));
        let reasons = asked_why(p);
        Some(confirm_request(
            id,
            p,
            record,
            &reasons,
            run.preview.as_ref(),
            self.agent_config().confirm_expiry,
        ))
    }

    pub(crate) async fn confirm(
        &self,
        p: &Prepared,
        run: &mut Run,
        request: &ConfirmRequest,
    ) -> Next {
        let id = request.id.clone();
        let mut opened = None;
        let answer = loop {
            match seat(&self.state, AskKey::of(request, p.who.session.as_ref())) {
                Seat::Waiter(rx) => match settled(rx).await {
                    Settled::Take(answer) => return self.taken(p, run, answer),
                    Settled::AskAgain => continue,
                },
                Seat::Asker(seat) => opened = Some(seat),
                Seat::Alone => {}
            }
            self.locked().pending.insert(id.clone(), p.space.clone());
            run.confirm = Some(id.clone());
            let answer = self.seams.confirmer().confirm(request.clone()).await;
            self.locked().pending.remove(&id);
            if let Some(seat) = opened.as_mut() {
                seat.settle(&answer);
            }
            break answer;
        };
        drop(opened);
        if self.halt_scope(&p.space).is_some() {
            self.seams.confirmer().cancel(&id).await;
            return Next::Event(CallEvent::Halted);
        }
        let answer = Self::brief_settled(p, run, answer);
        let yes = match &answer {
            ConfirmAnswer::Allowed { scope, receipt, .. } => {
                Some((ConfirmAnswerKind::Allowed(scope.clone()), receipt))
            }
            ConfirmAnswer::AllowedFromTerminal { receipt } => {
                Some((ConfirmAnswerKind::AllowedFromTerminal, receipt))
            }
            ConfirmAnswer::Never { receipt } => Some((ConfirmAnswerKind::Never, receipt)),
            ConfirmAnswer::Ended(_) => None,
        };
        if let Some((kind, receipt)) = yes {
            // A "never" proves the person said so; it is no proof for running the call.
            if !matches!(answer, ConfirmAnswer::Never { .. }) {
                run.receipt = Some(receipt.clone());
            }
            self.seams.sink().append(AuditRecord::Confirm {
                at: self.seams.clock().now(),
                id: id.clone(),
                answer: kind,
                input: Some(receipt.input),
            });
        }
        Next::Event(CallEvent::Answered(answer))
    }

    /// A call that waited on another's sheet takes that sheet's answer: the person's yes is
    /// the receipt it runs on, and a refusal refuses it too.
    pub(crate) fn taken(&self, p: &Prepared, run: &mut Run, answer: ConfirmAnswer) -> Next {
        if self.halt_scope(&p.space).is_some() {
            return Next::Event(CallEvent::Halted);
        }
        let answer = Self::brief_settled(p, run, answer);
        if let ConfirmAnswer::Allowed { receipt, .. } = &answer {
            run.receipt = Some(receipt.clone());
        }
        Next::Event(CallEvent::Answered(answer))
    }

    /// A brief sheet's answer as it counts (see `brief_answer`), with the words the person
    /// confirmed noted on the run.
    pub(crate) fn brief_settled(
        p: &Prepared,
        run: &mut Run,
        answer: ConfirmAnswer,
    ) -> ConfirmAnswer {
        let answer = brief_answer(p, answer);
        if let ConfirmAnswer::Allowed {
            written: Some(words),
            ..
        } = &answer
        {
            run.written = Some(words.clone());
        }
        answer
    }
}

/// Why the person is asked: the gate's reasons, or the reviewers' tightening.
pub(crate) fn asked_why(p: &Prepared) -> Vec<AskReason> {
    match &p.pending {
        Pending::Confirm(r) => r.clone(),
        _ => vec![AskReason::Rule(PolicyId("review".into()))],
    }
}
