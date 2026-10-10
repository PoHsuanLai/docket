//! The caller side: typed methods over a transport. Each sends one request and reads the one
//! reply it expects; a refusal or an unexpected reply is an error the caller can act on.

use crate::transport::{Transport, TransportError};
use docket_core::{
    ActionRef, ActivationToken, CallRefusal, CallRequest, ContextView, CuaAsk, Delivery, Displayed,
    EntityRef, GateAnswer, GrantAnswer, GrantAsk, HaltCause, Handle, Hit, InboundLine, InboxAsk,
    IntentsReply, IntentsRequest, JournalFilter, KillSwitch, MessageDraft, Outcome, Preview,
    Resolved, SearchAsk, SessionOpen, SessionOpened, SuggestAsk, TurnId, TurnIn, UndoEntry, UndoId,
    UndoReport, UndoScope, ValidManifest, WidenAnswer, WidenAsk, WindowKey, WireRefusal,
};
use prov::{AppName, EntityId, SessionId, SpaceScope};

/// Why a request did not give the reply its caller wanted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClientError {
    /// The request did not reach the router, or its reply did not come back.
    #[error("transport: {0}")]
    Transport(TransportError),
    /// The router refused the request.
    #[error("refused: {0:?}")]
    Refused(WireRefusal),
    /// The router answered, but not with the reply this request gets.
    #[error("unexpected reply")]
    Unexpected,
}

impl From<TransportError> for ClientError {
    fn from(error: TransportError) -> Self {
        ClientError::Transport(error)
    }
}

/// A caller of the router.
///
/// Over the router in this process (the `in_process` feature), with the neutral fake apps:
///
/// ```
/// # #[cfg(feature = "in_process")]
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use docket_client::{InProcess, Intents};
/// use docket_core::{AgentConfig, CallerId, CallerRole};
/// use docket_fake::fake_router;
/// use porter_core::{AppId, AppName, Isolation};
/// use std::{collections::BTreeSet, sync::Arc};
///
/// let router = Arc::new(fake_router(AgentConfig::default())?);
/// let caller = CallerId {
///     app: AppId {
///         name: AppName::parse("org.quire.Shell")?,
///         isolation: Isolation::Unsandboxed,
///     },
///     roles: BTreeSet::from([CallerRole::Editor]),
/// };
/// let link = Intents::over(InProcess::new(router, caller));
/// assert!(!link.manifests().await?.is_empty());
/// # Ok(()) }
/// # #[cfg(not(feature = "in_process"))]
/// # fn main() {}
/// ```
#[derive(Debug)]
pub struct Intents<T: Transport> {
    transport: T,
}

impl<T: Transport> Intents<T> {
    /// Calls through `transport`.
    pub fn over(transport: T) -> Self {
        Self { transport }
    }

    pub(crate) fn transport(&self) -> &T {
        &self.transport
    }

    pub(crate) async fn ask<R>(
        &self,
        request: IntentsRequest,
        read: impl FnOnce(IntentsReply) -> Option<R>,
    ) -> Result<R, ClientError> {
        match self.transport.call(request).await? {
            IntentsReply::Refused(why) => Err(ClientError::Refused(why)),
            reply => read(reply).ok_or(ClientError::Unexpected),
        }
    }

    /// Performs a call. The outer error is the transport or a refused request; the inner result
    /// is the call's own end.
    pub async fn perform(
        &self,
        call: CallRequest,
        session: Option<SessionId>,
        parent_window: Option<WindowKey>,
    ) -> Result<Result<Outcome, CallRefusal>, ClientError> {
        self.perform_activated(call, session, parent_window, None)
            .await
    }

    /// `perform` with the launcher's activation token, which intentd passes to the app when the
    /// caller is the launcher and drops for every other role.
    pub async fn perform_activated(
        &self,
        call: CallRequest,
        session: Option<SessionId>,
        parent_window: Option<WindowKey>,
        activation: Option<ActivationToken>,
    ) -> Result<Result<Outcome, CallRefusal>, ClientError> {
        let request = IntentsRequest::Perform {
            call,
            session,
            parent_window,
            activation,
        };
        self.ask(request, |r| match r {
            IntentsReply::Performed(end) => Some(*end),
            _ => None,
        })
        .await
    }

    /// What the app would change if `call` ran, through the same checks and policy as
    /// `perform`: nobody is asked, nothing runs. A call policy refuses comes back refused.
    pub async fn dry_run(
        &self,
        call: CallRequest,
        session: Option<SessionId>,
    ) -> Result<Preview, ClientError> {
        self.ask(IntentsRequest::DryRun { call, session }, |r| match r {
            IntentsReply::Preview(p) => Some(p),
            _ => None,
        })
        .await
    }

    /// The installed manifests.
    pub async fn manifests(&self) -> Result<Vec<ValidManifest>, ClientError> {
        self.ask(IntentsRequest::Manifests, |r| match r {
            IntentsReply::Manifests(all) => Some(all),
            _ => None,
        })
        .await
    }

    /// The undo journal, newest first. A terminal sees only the rows of its own acts.
    pub async fn journal(&self, filter: JournalFilter) -> Result<Vec<UndoEntry>, ClientError> {
        self.ask(IntentsRequest::ControlJournal(filter), |r| match r {
            IntentsReply::Journal(rows) => Some(rows),
            _ => None,
        })
        .await
    }

    /// The actions the terminal may run without asking, until logout (control centre).
    pub async fn terminal_grants(&self) -> Result<Vec<ActionRef>, ClientError> {
        self.ask(IntentsRequest::ControlTerminalGrants, |r| match r {
            IntentsReply::TerminalGrants(actions) => Some(actions),
            _ => None,
        })
        .await
    }

    /// Withdraws one standing terminal grant (control centre).
    pub async fn revoke_terminal_grant(&self, action: ActionRef) -> Result<(), ClientError> {
        self.ask(IntentsRequest::ControlTerminalRevoke(action), |r| match r {
            IntentsReply::Done => Some(()),
            _ => None,
        })
        .await
    }

    /// The standing "allow always" grants held (Settings).
    pub async fn standing_grants(&self) -> Result<Vec<docket_core::StandingGrant>, ClientError> {
        self.ask(IntentsRequest::ControlStandingGrants, |r| match r {
            IntentsReply::StandingGrants(grants) => Some(grants),
            _ => None,
        })
        .await
    }

    /// Revokes one standing grant (Settings): the next call it covered asks again.
    pub async fn revoke_standing_grant(
        &self,
        id: docket_core::StandingGrantId,
    ) -> Result<(), ClientError> {
        self.ask(IntentsRequest::ControlStandingRevoke(id), |r| match r {
            IntentsReply::Done => Some(()),
            _ => None,
        })
        .await
    }

    /// Resumes `scope` after a halt (control centre). It also reopens the terminal sessions the
    /// breaker paused in that scope: a terminal has no way to say it is the person.
    pub async fn resume(&self, scope: SpaceScope) -> Result<(), ClientError> {
        self.ask(IntentsRequest::ControlResume { scope }, |r| match r {
            IntentsReply::Done => Some(()),
            _ => None,
        })
        .await
    }

    /// Halts `scope` (control centre, compositor).
    pub async fn halt(&self, scope: SpaceScope, cause: HaltCause) -> Result<(), ClientError> {
        self.ask(IntentsRequest::ControlHalt { scope, cause }, |r| match r {
            IntentsReply::Done => Some(()),
            _ => None,
        })
        .await
    }

    /// The kill switch (control centre, compositor).
    pub async fn kill_switch(&self) -> Result<KillSwitch, ClientError> {
        self.ask(IntentsRequest::ControlState, |r| match r {
            IntentsReply::State(kill) => Some(kill),
            _ => None,
        })
        .await
    }

    /// A preview of one thing.
    pub async fn preview(&self, id: EntityId) -> Result<Preview, ClientError> {
        self.ask(IntentsRequest::Preview(id), |r| match r {
            IntentsReply::Preview(p) => Some(p),
            _ => None,
        })
        .await
    }

    /// Options for a parameter.
    pub async fn suggest(&self, ask: SuggestAsk) -> Result<Vec<EntityRef>, ClientError> {
        self.ask(IntentsRequest::Suggest(ask), |r| match r {
            IntentsReply::Suggestions(s) => Some(s),
            _ => None,
        })
        .await
    }

    /// Searches; late hits arrive on the transport's signal path.
    pub async fn search(&self, ask: SearchAsk) -> Result<Vec<Hit>, ClientError> {
        self.ask(IntentsRequest::Search(ask), |r| match r {
            IntentsReply::Hits(h) => Some(h),
            _ => None,
        })
        .await
    }

    /// Undoes one journal row.
    pub async fn undo(
        &self,
        entry: UndoId,
    ) -> Result<Result<(), docket_core::UndoFault>, ClientError> {
        self.ask(IntentsRequest::Undo(entry), |r| match r {
            IntentsReply::Undone(end) => Some(end),
            _ => None,
        })
        .await
    }

    /// Undoes a run, a task or an entry.
    pub async fn undo_all(&self, scope: UndoScope) -> Result<UndoReport, ClientError> {
        self.ask(IntentsRequest::UndoAll(scope), |r| match r {
            IntentsReply::UndoneAll(report) => Some(report),
            _ => None,
        })
        .await
    }

    /// What the person is doing, as a planner may read it.
    pub async fn context(
        &self,
        session: SessionId,
        app: AppName,
    ) -> Result<ContextView, ClientError> {
        self.ask(IntentsRequest::Context { session, app }, |r| match r {
            IntentsReply::Context(view) => Some(*view),
            _ => None,
        })
        .await
    }

    /// Computer use: may a run use this app in this Space? The person is asked inside the router.
    pub async fn gate_grant(&self, ask: GrantAsk) -> Result<GrantAnswer, ClientError> {
        self.ask(IntentsRequest::GateGrant(ask), |r| match r {
            IntentsReply::Granted(answer) => Some(answer),
            _ => None,
        })
        .await
    }

    /// Computer use: may this pixel step run? A confirmation is resolved inside the router
    /// before it answers.
    pub async fn gate_check(&self, ask: CuaAsk) -> Result<GateAnswer, ClientError> {
        self.ask(IntentsRequest::GateCheck(ask), |r| match r {
            IntentsReply::Gate(answer) => Some(answer),
            _ => None,
        })
        .await
    }

    /// Opens a session.
    pub async fn session_open(&self, open: SessionOpen) -> Result<SessionOpened, ClientError> {
        self.ask(IntentsRequest::SessionOpen(open), |r| match r {
            IntentsReply::SessionOpened(opened) => Some(opened),
            _ => None,
        })
        .await
    }

    /// Records the person's turn (launcher, field and terminal roles only).
    pub async fn session_turn(
        &self,
        session: SessionId,
        turn: TurnIn,
    ) -> Result<TurnId, ClientError> {
        self.ask(IntentsRequest::SessionTurn { session, turn }, |r| match r {
            IntentsReply::TurnRecorded(id) => Some(id),
            _ => None,
        })
        .await
    }

    /// Closes a session.
    pub async fn session_close(&self, session: SessionId) -> Result<(), ClientError> {
        self.ask(IntentsRequest::SessionClose { session }, |r| match r {
            IntentsReply::Done => Some(()),
            _ => None,
        })
        .await
    }

    /// The text behind a handle, for the screen.
    pub async fn session_display(
        &self,
        session: SessionId,
        handle: Handle,
    ) -> Result<String, ClientError> {
        self.ask(
            IntentsRequest::SessionDisplay { session, handle },
            |r| match r {
                IntentsReply::Text(text) => Some(text),
                _ => None,
            },
        )
        .await
    }

    /// The text behind a handle with its label, for the screen: the label says whether the
    /// content is the person's own or came from outside, so the screen can mark it.
    pub async fn session_display_labelled(
        &self,
        session: SessionId,
        handle: Handle,
    ) -> Result<Displayed, ClientError> {
        self.ask(
            IntentsRequest::SessionDisplayLabelled { session, handle },
            |r| match r {
                IntentsReply::Displayed(shown) => Some(shown),
                _ => None,
            },
        )
        .await
    }

    /// The text behind a handle and its label, for the quarantined reader (the `reader` role's
    /// alone): the session now counts as having shown it to a reader.
    pub async fn session_resolve(
        &self,
        session: SessionId,
        handle: Handle,
    ) -> Result<Resolved, ClientError> {
        self.ask(
            IntentsRequest::SessionResolve { session, handle },
            |r| match r {
                IntentsReply::Resolved(resolved) => Some(resolved),
                _ => None,
            },
        )
        .await
    }

    /// Asks the person to widen the task policy.
    pub async fn session_widen(
        &self,
        session: SessionId,
        widen: WidenAsk,
    ) -> Result<WidenAnswer, ClientError> {
        self.ask(
            IntentsRequest::SessionWiden { session, widen },
            |r| match r {
                IntentsReply::Widened(answer) => Some(answer),
                _ => None,
            },
        )
        .await
    }

    /// Sends a message: a request, a note or a report, to an agent in any Space.
    pub async fn send(
        &self,
        session: SessionId,
        draft: MessageDraft,
    ) -> Result<Delivery, ClientError> {
        self.ask(
            IntentsRequest::MessageSend { session, draft },
            |r| match r {
                IntentsReply::Delivered(delivery) => Some(delivery),
                _ => None,
            },
        )
        .await
    }

    /// The messages that wait for an agent.
    pub async fn inbox(&self, ask: InboxAsk) -> Result<Vec<InboundLine>, ClientError> {
        self.ask(IntentsRequest::MessageInbox(ask), |r| match r {
            IntentsReply::Inbox(lines) => Some(lines),
            _ => None,
        })
        .await
    }
}
