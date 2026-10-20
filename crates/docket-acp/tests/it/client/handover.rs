//! A helper's conversation with its agent app moving to the app's own terminal program and back
//! (swarm §7, "Open in Terminal" and "Bring Back"): the process is closed and the session at the
//! router becomes a watch session, then the app is started again on the same session and picks
//! its own conversation up. Every agent is scripted and every world is the fakes.

use super::agent::{AGENT_SESSION, Act, Auth, Offer, Resumes, View, agent_resuming, say};
use super::rig::{CWD, Setup, opening, program, wired_over};
use super::world::{Fakes, TheCourt};
use docket_acp::client::fake::{FakeFiles, FakeSpawn};
use docket_acp::client::{
    AcpBackend, AgentEnd, AgentHost, Fallback, HelperAgent, Hosts, Parts, Start,
};
use docket_client::{InProcess, Intents};
use docket_core::helper::HandFault;
use docket_core::{AppSessionId, CallerId, CallerRole, WatchedAgent};
use docket_inapp::EditorDesk;
use docket_session::Opening;
use prov::{SessionId, UnixSeconds};
use std::collections::{BTreeSet, VecDeque};

const AT: UnixSeconds = UnixSeconds(1_760_000_000);

/// The hosts of one helper's agent processes, all over the one router: each start takes the next
/// agent that was prepared.
struct OneRouter {
    court: TheCourt,
    desk: EditorDesk,
    queue: VecDeque<AcpBackend<Fakes>>,
    resume: Option<Vec<String>>,
}

impl Hosts for OneRouter {
    type Seams = Fakes;
    type Desk = EditorDesk;

    async fn host(&mut self, start: Start) -> AgentHost<Fakes, EditorDesk> {
        let mut backend = self.queue.pop_front().expect("an agent was prepared");
        if let Some(id) = &start.resume {
            backend = backend.resuming(id);
        }
        AgentHost::new(
            backend,
            self.court.clone(),
            self.desk.clone(),
            Fallback::Off,
        )
    }

    fn opening(&self) -> Opening {
        opening(CWD)
    }

    fn resume_args(&self) -> Option<Vec<String>> {
        self.resume.clone()
    }
}

type Watching = Intents<InProcess<super::world::WorldSeams>>;

fn answering(words: &'static str) -> Vec<Vec<Act>> {
    vec![vec![say(words), Act::Stop("end_turn")]]
}

/// A helper's agent over one router: the first agent is the world's own, the rest are the
/// `later` ones (each says whether it can pick a session up again).
fn helper_over(
    first: Resumes,
    later: Vec<Resumes>,
    resume: Option<Vec<String>>,
) -> (HelperAgent<OneRouter>, Vec<View>, Watching) {
    let setup = Setup {
        turns: answering("First."),
        resumes: first,
        ..Setup::default()
    };
    let wired = wired_over::<Fakes>(FakeFiles::new(), setup);
    let mut views = vec![wired.agent.clone()];
    let mut queue = VecDeque::from([wired.backend]);
    for resumes in later {
        let (wire, view) = agent_resuming(answering("Again."), (Auth::Open, Offer::None), resumes);
        let (spawn, _) = FakeSpawn::new(vec![wire]);
        queue.push_back(AcpBackend::<Fakes>::new(Parts {
            program: program(),
            session: SessionId::parse("s-1").expect("session"),
            spawn,
            performer: wired.performer.clone(),
            court: wired.court.clone(),
            tools: None,
        }));
        views.push(view);
    }
    let companion = CallerId {
        app: porter_core::AppId {
            name: crate::support::app("org.quire.Companiond"),
            isolation: porter_core::Isolation::Unsandboxed,
        },
        roles: BTreeSet::from([CallerRole::Companion]),
    };
    let watching = Intents::over(InProcess::new(wired.router.clone(), companion));
    let hosts = OneRouter {
        court: wired.court,
        desk: wired.desk,
        queue,
        resume,
    };
    (HelperAgent::new(hosts), views, watching)
}

fn resume_flag() -> Option<Vec<String>> {
    Some(vec!["--resume".to_owned(), "{session}".to_owned()])
}

#[tokio::test]
async fn a_conversation_goes_to_the_terminal_and_comes_back_on_the_same_session() {
    let (mut helper, views, watching) =
        helper_over(Resumes::Yes, vec![Resumes::Yes], resume_flag());
    let done = helper.run("Compare the offers", AT).await;
    assert!(matches!(done, AgentEnd::Done(_)), "{done:?}");
    assert_eq!(
        helper.terminal_ready().as_ref().map(AppSessionId::as_str),
        Some(AGENT_SESSION)
    );
    let (_, router_session) = helper.handle().expect("open");

    // Open in Terminal: the app is closed, and the session is a watch session that names the app
    // and its own id for the conversation.
    let away = helper.to_terminal().await.expect("to the terminal");
    assert_eq!(away.session, router_session);
    assert_eq!(
        away.agent,
        WatchedAgent {
            program: program(),
            session: AppSessionId::parse(AGENT_SESSION).expect("id"),
        }
    );
    assert!(!helper.is_open());
    let watched = watching.checkpoint_watched().await.expect("watched");
    assert_eq!(watched.len(), 1);
    assert_eq!(watched[0].session, router_session);
    assert_eq!(watched[0].agent, away.agent);

    // Bring Back: a new process picks the conversation up and the session is the same one.
    helper.bring_back().await.expect("back");
    assert!(helper.is_open() && helper.away().is_none());
    assert_eq!(helper.handle().expect("open").1, router_session);
    assert!(
        watching
            .checkpoint_watched()
            .await
            .expect("watched")
            .is_empty()
    );
    let resumed = views[1].resumed();
    assert_eq!(resumed.len(), 1);
    assert_eq!(resumed[0]["sessionId"], AGENT_SESSION);
    let again = helper.run("And the best one?", AT).await;
    assert!(matches!(again, AgentEnd::Done(_)), "{again:?}");
}

#[tokio::test]
async fn a_conversation_that_cannot_be_picked_up_again_or_has_not_begun_stays_where_it_is() {
    // No conversation of the agent's own yet.
    let (mut idle, _, _) = helper_over(Resumes::Yes, vec![], resume_flag());
    assert_eq!(idle.to_terminal().await, Err(HandFault::NoConversation));

    // The app has no flag to pick a conversation up with.
    let (mut flagless, _, _) = helper_over(Resumes::Yes, vec![], None);
    flagless.run("Job", AT).await;
    assert_eq!(flagless.to_terminal().await, Err(HandFault::NotResumable));
    assert!(flagless.is_open(), "nothing was closed");

    // The agent cannot pick a session up (no resume, no load).
    let (mut unable, _, _) = helper_over(Resumes::No, vec![], resume_flag());
    unable.run("Job", AT).await;
    assert_eq!(unable.to_terminal().await, Err(HandFault::NotResumable));
    assert!(unable.is_open());
}

#[tokio::test]
async fn an_app_that_begins_another_conversation_leaves_this_one_in_the_terminal() {
    // The second process cannot pick the conversation up: it opens a new one instead.
    let (mut helper, _, watching) = helper_over(Resumes::Yes, vec![Resumes::No], resume_flag());
    helper.run("Job", AT).await;
    helper.to_terminal().await.expect("to the terminal");
    assert_eq!(helper.bring_back().await, Err(HandFault::NotResumable));
    assert!(helper.away().is_some() && !helper.is_open());
    assert_eq!(
        watching.checkpoint_watched().await.expect("watched").len(),
        1
    );
}
