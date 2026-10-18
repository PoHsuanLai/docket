//! The agent apps that are helpers, over the session bus: `org.quire.AgentHost1`'s `AppsBegin`
//! and `AppsWait`, served by `docket-agent --serve` under the name `org.quire.AcpAgent`. The host
//! keeps one agent process per helper and opens each one's session at the router as its own
//! role; companiond opens none and gains no way to. This carries the board's calls there and
//! waits for the reply, which the host gives within a minute or says to ask again (a job
//! outlasts what the bus lets one call wait). Asking the name starts the host if it is not
//! running. A host that cannot be reached answers every call as the failure it is, so a helper
//! says so and the others go on.

use docket_core::helper::{
    AgentApps, AgentOutcome, AgentPending, AppsAsk, AppsReply, CheckOutcome, HelperCard, Job,
    JobFault, SignInOutcome, SignInWay, SignOutOutcome,
};
use docket_dbus::{AgentHostProxy, BusConnection};
use prov::HelperId;

/// The host of the agent apps, on the session bus.
#[derive(Debug, Clone)]
pub struct AppsLink {
    connection: BusConnection,
}

impl AppsLink {
    /// A link over `connection`.
    pub fn new(connection: BusConnection) -> Self {
        Self { connection }
    }

    /// The host's reply to `ask`, or `None` when the host could not be reached or answered
    /// something that is not a reply.
    async fn ask(&self, ask: &AppsAsk) -> Option<AppsReply> {
        let reply = self.asked(ask).await;
        if let Err(cause) = &reply {
            tracing::warn!(%cause, "the host of the agent apps did not answer");
        }
        reply.ok()
    }

    async fn asked(&self, ask: &AppsAsk) -> Result<AppsReply, String> {
        let body = serde_json::to_string(ask).map_err(|e| e.to_string())?;
        let host = AgentHostProxy::new(&self.connection)
            .await
            .map_err(|e| e.to_string())?;
        let call = host.apps_begin(&body).await.map_err(|e| e.to_string())?;
        loop {
            let text = host.apps_wait(&call).await.map_err(|e| e.to_string())?;
            if !text.is_empty() {
                return serde_json::from_str(&text).map_err(|e| e.to_string());
            }
        }
    }
}

impl AgentApps for AppsLink {
    fn run<'a>(&'a self, card: &'a HelperCard, job: &'a Job) -> AgentPending<'a, AgentOutcome> {
        Box::pin(async move {
            let ask = AppsAsk::Run {
                card: Box::new(card.clone()),
                job: Box::new(job.clone()),
            };
            match self.ask(&ask).await {
                Some(AppsReply::Run(outcome)) => outcome,
                _ => AgentOutcome::Failed(JobFault::Crashed),
            }
        })
    }

    fn stop<'a>(&'a self, helper: &'a HelperId) -> AgentPending<'a, ()> {
        Box::pin(async move {
            let helper = helper.clone();
            let _ = self.ask(&AppsAsk::Stop { helper }).await;
        })
    }

    fn sign_in<'a>(
        &'a self,
        card: &'a HelperCard,
        way: &'a SignInWay,
    ) -> AgentPending<'a, SignInOutcome> {
        Box::pin(async move {
            let ask = AppsAsk::SignIn {
                card: Box::new(card.clone()),
                way: way.clone(),
            };
            match self.ask(&ask).await {
                Some(AppsReply::SignIn(outcome)) => outcome,
                _ => SignInOutcome::Failed,
            }
        })
    }

    fn check<'a>(&'a self, card: &'a HelperCard) -> AgentPending<'a, CheckOutcome> {
        Box::pin(async move {
            let ask = AppsAsk::Check {
                card: Box::new(card.clone()),
            };
            match self.ask(&ask).await {
                Some(AppsReply::Check(outcome)) => outcome,
                _ => CheckOutcome::Failed,
            }
        })
    }

    fn sign_out<'a>(&'a self, card: &'a HelperCard) -> AgentPending<'a, SignOutOutcome> {
        Box::pin(async move {
            let ask = AppsAsk::SignOut {
                card: Box::new(card.clone()),
            };
            match self.ask(&ask).await {
                Some(AppsReply::SignOut(outcome)) => outcome,
                _ => SignOutOutcome::Failed,
            }
        })
    }

    fn forget<'a>(&'a self, helper: &'a HelperId) -> AgentPending<'a, ()> {
        Box::pin(async move {
            let helper = helper.clone();
            let _ = self.ask(&AppsAsk::Forget { helper }).await;
        })
    }
}
