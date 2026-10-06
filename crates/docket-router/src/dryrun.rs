//! `.Run.DryRun`: what the app would change, asked through the same arguments check, labels,
//! consent and policy as a call. It asks nobody, charges nothing and runs nothing; a call the
//! gate would refuse is refused here with the same refusal, so `quire-do --dry-run` cannot be
//! used to probe around the gate.

use crate::gate::Pending;
use crate::router::Router;
use crate::seams::{AppLink, Seams};
use crate::who::Who;
use docket_core::{
    CallRefusal, CallRequest, Depth, DryRun, IntentsReply, Invocation, Preview, WireRefusal,
};

fn refused(why: CallRefusal) -> IntentsReply {
    IntentsReply::Refused(WireRefusal::Call(why))
}

impl<S: Seams> Router<S> {
    /// The preview of `request` for `who`, or the refusal the call itself would get.
    pub(crate) async fn run_dry(&self, who: Who, request: CallRequest) -> IntentsReply {
        let prepared = match self.prepare(&who, request, None, Depth(0)) {
            Ok(prepared) => prepared,
            Err(early) => return refused(early.refusal),
        };
        if let Pending::Refuse(why) = prepared.pending {
            return refused(why);
        }
        if prepared.decl.dry_run == DryRun::None {
            return IntentsReply::Preview(Preview::None);
        }
        let invocation = Invocation {
            call: prepared.id,
            action: prepared.decl.name.clone(),
            target: prepared.request.target.clone(),
            args: prepared.request.args.clone(),
            actor: prepared.who.actor.clone(),
            origin: prepared.request.origin,
            space: prepared.space.clone(),
        };
        match self
            .seams
            .link()
            .dry_run(&prepared.request.action.app, invocation)
            .await
        {
            Ok(preview) => IntentsReply::Preview(preview),
            Err(why) => refused(CallRefusal::App(why)),
        }
    }
}
