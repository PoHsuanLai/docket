//! An agent app as one helper's: `HelperAgent` keeps one agent process for the helper across its
//! jobs, gives each board job to it as a turn whose text is the job's goal, and returns what the
//! agent said as untrusted words. It decides nothing: every call the agent makes goes through the
//! router as for any hosted agent (`AgentHost`), and the sheets are the router's.
//!
//! **Sign-in is the agent's own.** A start that the agent refuses with "authentication required"
//! is not a failed job: it is [`AgentEnd::NeedsSignIn`], with the ways the agent listed by their
//! own names. A way the agent runs itself is chosen with [`HelperAgent::sign_in`], which starts the
//! agent with that way and keeps the process if it works; a way that needs a terminal comes back
//! as the arguments and variables for the shell to run the agent's own command with. We never read
//! the login: "signed in" is only that the agent opened a session.
//!
//! One agent process per helper, never shared; a process that ended is started again for the next
//! job and picks up its session where it can (`session/resume`).

use super::backend::Seams;
use super::host::AgentHost;
use super::spawn::SignIn;
use companion_model::helper::{HandFault, SignInHow, SignInWay, TerminalLogin, UntrustedWords};
use companion_model::{AppSessionId, WatchedAgent};
use docket_core::{Reveal, TurnSource, TurnVia, UserTurn};
use docket_session::{
    BackendEvent, BackendFault, BackendKind, HostFault, Opening, SessionHost, SheetDesk, TurnEnd,
};
use docket_vocab::TurnId;
use prov::{SessionId, UnixSeconds};
use std::future::Future;

/// What a new agent process is started with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Start {
    /// The way of signing in the person chose, to be sent with `authenticate` before the
    /// session opens. None when the agent already holds its login.
    pub sign_in: Option<SignIn>,
    /// The agent's own id for the session it had, to pick up again if it can.
    pub resume: Option<String>,
}

/// Where a helper's agent hosts come from: one new, not yet opened, per process the helper needs.
pub trait Hosts: Send {
    /// The seams a host is made of.
    type Seams: Seams;
    /// The desk its sheets land on.
    type Desk: SheetDesk;

    /// A host over a new backend for the helper's agent, started as `start` says, and made for
    /// this helper (`AgentHost::for_helper`), so its grants are the helper's alone.
    fn host(
        &mut self,
        start: Start,
    ) -> impl Future<Output = AgentHost<Self::Seams, Self::Desk>> + Send;

    /// How the helper's session opens at the router: its Space and the folder it works in.
    fn opening(&self) -> Opening;

    /// The arguments with which the app's own command-line program picks one conversation up
    /// (`{session}` stands for its id of it), when the app has such a flag. None by default.
    fn resume_args(&self) -> Option<Vec<String>> {
        None
    }
}

/// Why a job of an agent app did not finish, in the few ways the board tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trouble {
    /// The agent could not be started.
    CouldNotStart,
    /// It stopped answering, or its process ended.
    StoppedAnswering,
    /// It declined the turn, or its session paused for the person.
    Declined,
}

/// How a job of an agent app ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEnd {
    /// The agent answered. Its words are the model's own: untrusted.
    Done(UntrustedWords),
    /// The agent wants the person to sign in first: these are its ways, by its own names. The
    /// job did not fail and is not counted as a failure.
    NeedsSignIn(Vec<SignInWay>),
    /// The job did not finish.
    Failed(Trouble),
    /// The job was stopped before it finished.
    Stopped,
}

/// What choosing a way of signing in came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignInStep {
    /// The agent signed in and its session is open: the helper can take jobs.
    SignedIn,
    /// The way runs in a terminal: the shell is asked to open one running the agent's command
    /// with this added, in the helper's sandbox. When it closes, [`HelperAgent::check`] asks the
    /// agent again.
    OpenTerminal(TerminalLogin),
    /// The agent did not sign in with that way.
    Refused,
    /// The agent is not one of the ways it listed, or could not be started.
    Failed(Trouble),
}

/// What signing out came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignOutStep {
    /// The agent signed itself out, or was not signed in.
    SignedOut,
    /// The agent has no way of its own to sign out: the caller runs the agent's logout command.
    UseLogoutCommand,
    /// The agent could not be started to ask.
    Failed(Trouble),
}

/// An open agent process and its session at the router.
struct Link<H: Hosts> {
    host: AgentHost<H::Seams, H::Desk>,
    session: SessionId,
}

/// A conversation that went to the app's own terminal program: the session at the router, which
/// is a watch session meanwhile, and the app's id for the conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Away {
    /// The session at the router.
    pub session: SessionId,
    /// The app and its own id for the conversation.
    pub agent: WatchedAgent,
}

/// One helper's agent app.
pub struct HelperAgent<H: Hosts> {
    hosts: H,
    link: Option<Link<H>>,
    /// The agent's own id for its last session.
    resume: Option<String>,
    /// The conversation, while it is in the app's terminal program.
    away: Option<Away>,
    turns: u64,
}

impl<H: Hosts> std::fmt::Debug for HelperAgent<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HelperAgent")
    }
}

fn wants_sign_in(fault: &HostFault) -> bool {
    matches!(
        fault,
        HostFault::Backend(
            BackendFault::SignInNeeded
                | BackendFault::SignInChoose(_)
                | BackendFault::SignInUnsupported
        )
    )
}

impl<H: Hosts> HelperAgent<H> {
    /// A helper's agent over `hosts`, not yet started.
    pub fn new(hosts: H) -> Self {
        Self {
            hosts,
            link: None,
            resume: None,
            away: None,
            turns: 0,
        }
    }

    /// The same agent, its conversation already in a terminal: `session` is the watch session at
    /// the router and `agent` names the app and its id for it. [`HelperAgent::bring_back`] brings
    /// it into docket.
    #[must_use]
    pub fn already_away(self, session: SessionId, agent: WatchedAgent) -> Self {
        Self {
            away: Some(Away { session, agent }),
            ..self
        }
    }

    /// Where the conversation is, while it is in a terminal.
    pub fn away(&self) -> Option<&Away> {
        self.away.as_ref()
    }

    /// The app's own id for the conversation, when the person could pick it up in the app's own
    /// terminal program: the app has such a flag, can pick a conversation up again, and has
    /// opened one.
    pub fn terminal_ready(&self) -> Option<AppSessionId> {
        let link = self.link.as_ref()?;
        self.hosts.resume_args()?;
        let backend = link.host.backend();
        backend
            .picks_up_again()
            .then(|| backend.agent_session())
            .flatten()
            .and_then(|id| AppSessionId::parse(&id).ok())
    }

    /// Closes the agent and turns its session at the router into a watch session, so the
    /// conversation can go on in the app's own terminal program. Needs the agent open (a job
    /// has run), idle, and able to pick the conversation up again later.
    pub async fn to_terminal(&mut self) -> Result<Away, HandFault> {
        let own = self.terminal_ready().ok_or(match self.link {
            Some(_) => HandFault::NotResumable,
            None => HandFault::NoConversation,
        })?;
        let BackendKind::Acp(program) = self.hosts.opening().backend else {
            return Err(HandFault::Failed);
        };
        let Some(link) = self.link.as_mut() else {
            return Err(HandFault::NoConversation);
        };
        let away = Away {
            session: link.session.clone(),
            agent: WatchedAgent {
                program,
                session: own,
            },
        };
        link.host
            .hand_to_terminal(&away.session, away.agent.clone())
            .await
            .map_err(|_| HandFault::Busy)?;
        self.link = None;
        self.resume = Some(away.agent.session.to_string());
        self.away = Some(away.clone());
        Ok(away)
    }

    /// Starts the agent again on the session it left, on the conversation of its own that the
    /// terminal went on with. An agent that cannot pick the conversation up, or begins another
    /// instead, leaves it in the terminal.
    pub async fn bring_back(&mut self) -> Result<(), HandFault> {
        let Some(away) = self.away.clone() else {
            return Err(HandFault::NotThere);
        };
        let start = Start {
            sign_in: None,
            resume: Some(away.agent.session.to_string()),
        };
        let mut host = self.hosts.host(start).await;
        let taken = host
            .take_back(away.session.clone(), self.hosts.opening())
            .await;
        if taken.is_err() {
            host.abandon().await;
            return Err(HandFault::Failed);
        }
        // An agent that opened a conversation of its own instead did not pick this one up.
        if !host.backend().picked_up() {
            let _ = host
                .hand_to_terminal(&away.session, away.agent.clone())
                .await;
            host.abandon().await;
            return Err(HandFault::NotResumable);
        }
        self.resume = host.backend().agent_session();
        self.link = Some(Link {
            host,
            session: away.session,
        });
        self.away = None;
        Ok(())
    }

    /// Whether an agent process is running for the helper.
    pub fn is_open(&self) -> bool {
        self.link.is_some()
    }

    /// The task and the router session the agent's answers are given in, while it is running:
    /// where an answer's label comes from.
    pub fn handle(&self) -> Option<(prov::TaskId, SessionId)> {
        let link = self.link.as_ref()?;
        Some((self.hosts.opening().task, link.session.clone()))
    }

    /// Starts the agent as `start` says. A start that the agent refuses for want of a sign-in
    /// ends as [`AgentEnd::NeedsSignIn`] with its ways; the process is ended.
    async fn open(&mut self, sign_in: Option<SignIn>) -> Result<(), AgentEnd> {
        let start = Start {
            sign_in,
            resume: self.resume.clone(),
        };
        let mut host = self.hosts.host(start).await;
        match host.open(self.hosts.opening()).await {
            Ok(session) => {
                self.resume = host.backend().agent_session();
                self.link = Some(Link { host, session });
                Ok(())
            }
            Err(fault) if wants_sign_in(&fault) => {
                let ways = host.backend().auth_ways().to_vec();
                host.abandon().await;
                Err(AgentEnd::NeedsSignIn(ways))
            }
            Err(fault) => {
                tracing::warn!(%fault, "a helper's agent app did not start");
                host.abandon().await;
                Err(AgentEnd::Failed(Trouble::CouldNotStart))
            }
        }
    }

    /// Ends the agent process, if one is running. The next job starts a new one.
    pub async fn close(&mut self) {
        if let Some(Link { mut host, session }) = self.link.take() {
            // The router hears the session end; an agent already gone has nothing to say.
            let _ = host.close(&session, docket_session::EndCause::Closed).await;
        }
    }

    /// Starts the agent if it is not running, to learn whether it is signed in: `Ok` when its
    /// session opened, else why not.
    pub async fn check(&mut self) -> Result<(), AgentEnd> {
        match self.link {
            Some(_) => Ok(()),
            None => self.open(None).await,
        }
    }

    /// Gives the agent a board job: its goal is the text of one turn. The goal is trusted text (the
    /// board admits only the person's words and a planner's that read nothing untrusted); what the
    /// agent says back is not.
    pub async fn run(&mut self, goal: &str, at: UnixSeconds) -> AgentEnd {
        if let Err(end) = self.check().await {
            return end;
        }
        let Some(link) = self.link.as_mut() else {
            return AgentEnd::Failed(Trouble::CouldNotStart);
        };
        self.turns += 1;
        let turn = UserTurn {
            id: TurnId(self.turns),
            text: goal.to_owned(),
            at,
            from: TurnSource::Launcher,
            via: TurnVia::Typed,
            mode: None,
        };
        let played = Self::play(link, turn).await;
        match played {
            Played::Ended(TurnEnd::Done, said) => {
                self.resume = self
                    .link
                    .as_ref()
                    .and_then(|l| l.host.backend().agent_session());
                AgentEnd::Done(UntrustedWords::new(said.concat()))
            }
            Played::Ended(TurnEnd::Cancelled, _) => AgentEnd::Stopped,
            Played::Ended(TurnEnd::Refused | TurnEnd::Paused(_), _) => {
                AgentEnd::Failed(Trouble::Declined)
            }
            Played::Ended(TurnEnd::Failed, _) | Played::Broke => {
                // The process may be gone or confused: the next job starts a new one.
                self.close().await;
                AgentEnd::Failed(Trouble::StoppedAnswering)
            }
        }
    }

    /// Gives the turn and collects the agent's words until the turn ends.
    async fn play(link: &mut Link<H>, turn: UserTurn) -> Played {
        let Link { host, session } = link;
        if host.turn(session, turn).await.is_err() {
            return Played::Broke;
        }
        let mut said = Vec::new();
        loop {
            match host.next_event(session).await {
                Ok(Some(BackendEvent::Words(Reveal::Plain(text)))) => said.push(text),
                Ok(Some(BackendEvent::TurnEnd(end))) => return Played::Ended(end, said),
                // Calls, thoughts and usage are the router's to rule and record.
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => return Played::Broke,
            }
        }
    }

    /// Stops the job in progress: the agent is asked to cancel its turn.
    pub async fn stop(&mut self) {
        if let Some(Link { host, session }) = self.link.as_mut() {
            let _ = host.cancel(session).await;
        }
    }

    /// Signs in the way `way` says, one the agent listed. A way the agent runs itself starts the
    /// agent with it (`authenticate`, then the session) and keeps the process; a way that needs a
    /// terminal starts nothing and returns what the shell runs.
    pub async fn sign_in(&mut self, way: &SignInWay) -> SignInStep {
        match &way.how {
            SignInHow::Terminal(login) => SignInStep::OpenTerminal(login.clone()),
            SignInHow::Agent => {
                let Ok(id) = SignIn::parse(&way.id) else {
                    return SignInStep::Failed(Trouble::CouldNotStart);
                };
                self.close().await;
                match self.open(Some(id)).await {
                    Ok(()) => SignInStep::SignedIn,
                    Err(AgentEnd::NeedsSignIn(_)) => SignInStep::Refused,
                    Err(AgentEnd::Failed(trouble)) => SignInStep::Failed(trouble),
                    Err(AgentEnd::Done(_) | AgentEnd::Stopped) => {
                        SignInStep::Failed(Trouble::CouldNotStart)
                    }
                }
            }
        }
    }

    /// Signs the agent out with its own `logout`, when it advertised the method. The process is
    /// ended either way: a signed-out agent starts again, and asks to be signed in.
    pub async fn sign_out(&mut self) -> SignOutStep {
        match self.check().await {
            Ok(()) => {}
            // Not signed in: nothing to sign out of.
            Err(AgentEnd::NeedsSignIn(_)) => return SignOutStep::SignedOut,
            Err(_) => return SignOutStep::Failed(Trouble::CouldNotStart),
        }
        let asked = match self.link.as_mut() {
            Some(link) => link.host.log_out().await,
            None => Ok(false),
        };
        self.close().await;
        self.resume = None;
        match asked {
            Ok(true) => SignOutStep::SignedOut,
            Ok(false) | Err(_) => SignOutStep::UseLogoutCommand,
        }
    }
}

/// How a turn played out.
enum Played {
    /// The turn ended, with the words the agent said.
    Ended(TurnEnd, Vec<String>),
    /// The agent stopped answering or its process ended.
    Broke,
}
