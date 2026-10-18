//! An agent app as one helper's (swarm H1): a board job is one turn, the agent's words come back
//! as untrusted data, a missing sign-in is the helper waiting and never a failed job, the agent
//! signs in its own way, and a crash ends that job only. Every agent is scripted and every world
//! is the fakes: no process, no login, no network.

use super::agent::{Act, Auth, View, call, say};
use super::rig::{
    CWD, Setup, Wired, abs, always, once, opening, program, read, tool, wired_over, write,
};
use super::world::{Fakes, World};
use docket_acp::client::fake::{FakeFiles, SpawnSeen};
use docket_acp::client::{
    AgentEnd, AgentHost, HelperAgent, Hosts, SignInStep, SignOutStep, Start, Trouble,
};
use docket_core::GrantCaller;
use docket_core::helper::{SignInHow, SignInWay};
use docket_inapp::EditorDesk;
use docket_session::Opening;
use prov::{HelperId, UnixSeconds};
use serde_json::json;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

const AT: UnixSeconds = UnixSeconds(1_760_000_000);

/// The worlds a helper's processes start in, in order, and what each start asked for.
struct Worlds {
    queue: VecDeque<Wired<Fakes>>,
    helper: HelperId,
    starts: Arc<Mutex<Vec<Start>>>,
}

impl Hosts for Worlds {
    type Seams = Fakes;
    type Desk = EditorDesk;

    async fn host(&mut self, start: Start) -> AgentHost<Fakes, EditorDesk> {
        self.starts.lock().expect("starts").push(start.clone());
        let world = self.queue.pop_front().expect("a world was prepared");
        let mut backend = world.backend;
        if let Some(way) = start.sign_in {
            backend = backend.with_sign_in(way);
        }
        if let Some(id) = &start.resume {
            backend = backend.resuming(id);
        }
        AgentHost::new(backend, world.court, world.desk, world.fallback)
            .for_helper(self.helper.clone())
    }

    fn opening(&self) -> Opening {
        opening(CWD)
    }
}

/// What a test keeps of a prepared world.
struct Kept {
    agent: View,
    spawned: SpawnSeen,
    router: Arc<World>,
}

fn helper(n: &str) -> HelperId {
    serde_json::from_value(json!(n)).expect("helper id")
}

fn world(files: &FakeFiles, setup: Setup) -> (Wired<Fakes>, Kept) {
    let wired = wired_over::<Fakes>(files.clone(), setup);
    let kept = Kept {
        agent: wired.agent.clone(),
        spawned: wired.spawned.clone(),
        router: wired.router.clone(),
    };
    (wired, kept)
}

fn agent_over(
    id: &str,
    setups: Vec<Setup>,
    files: &FakeFiles,
) -> (HelperAgent<Worlds>, Vec<Kept>, Arc<Mutex<Vec<Start>>>) {
    let (queue, kept): (VecDeque<_>, Vec<_>) =
        setups.into_iter().map(|setup| world(files, setup)).unzip();
    let starts = Arc::new(Mutex::new(Vec::new()));
    let worlds = Worlds {
        queue,
        helper: helper(id),
        starts: starts.clone(),
    };
    (HelperAgent::new(worlds), kept, starts)
}

fn turns(scripts: Vec<Vec<Act>>) -> Setup {
    Setup {
        turns: scripts,
        ..Setup::default()
    }
}

fn needing_sign_in() -> Setup {
    Setup {
        auth: Auth::Needed,
        turns: vec![vec![say("Signed in and done."), Act::Stop("end_turn")]],
        ..Setup::default()
    }
}

fn agent_way(id: &str) -> SignInWay {
    SignInWay {
        id: id.to_owned(),
        name: id.to_owned(),
        how: SignInHow::Agent,
    }
}

#[tokio::test]
async fn a_board_job_is_one_turn_and_the_agents_words_come_back_untrusted() {
    let files = FakeFiles::new();
    let setup = turns(vec![
        vec![say("Three offers."), Act::Stop("end_turn")],
        vec![say("The second is best."), Act::Stop("end_turn")],
    ]);
    let (mut helper, kept, _) = agent_over("h-1", vec![setup], &files);

    let first = helper.run("Compare the offers", AT).await;
    let AgentEnd::Done(words) = first else {
        panic!("expected the agent's answer, got {first:?}");
    };
    assert_eq!(words.as_untrusted_str(), "Three offers.");
    // Nothing of the agent's own is in the prompt: the goal, and only the goal.
    assert_eq!(
        kept[0].agent.prompts()[0]["prompt"],
        json!([{"type": "text", "text": "Compare the offers"}])
    );

    // The same process takes the next job; none is started for it.
    let second = helper.run("Which one?", AT).await;
    assert!(matches!(second, AgentEnd::Done(w) if w.as_untrusted_str() == "The second is best."));
    assert_eq!(kept[0].agent.prompts().len(), 2);
    assert_eq!(kept[0].spawned.plans().len(), 1, "one process per helper");
}

#[tokio::test]
async fn a_missing_sign_in_is_the_helper_waiting_with_the_agents_own_names_not_a_failed_job() {
    let files = FakeFiles::new();
    let (mut helper, kept, _) = agent_over("h-1", vec![needing_sign_in()], &files);

    let ended = helper.run("Compare the offers", AT).await;
    let AgentEnd::NeedsSignIn(ways) = ended else {
        panic!("expected a wait for sign-in, got {ended:?}");
    };
    let names: Vec<_> = ways.iter().map(|w| w.name.as_str()).collect();
    assert_eq!(names, ["Personal sign-in", "API key", "Terminal sign-in"]);
    assert!(matches!(ways[0].how, SignInHow::Agent));
    let SignInHow::Terminal(login) = &ways[2].how else {
        panic!("the third is a terminal way: {ways:?}");
    };
    assert_eq!(login.args, ["--login"]);
    assert_eq!(login.env.get("BROWSER").map(String::as_str), Some("none"));

    // We said we can run a sign-in in a terminal; no turn was given; the process was ended.
    assert_eq!(kept[0].agent.client_caps()["auth"]["terminal"], json!(true));
    assert!(kept[0].agent.prompts().is_empty());
    assert_eq!(kept[0].spawned.killed(), 1);
    assert!(!helper.is_open());
}

#[tokio::test]
async fn an_agent_way_calls_authenticate_and_then_the_job_proceeds() {
    let files = FakeFiles::new();
    let (mut helper, kept, starts) = agent_over("h-1", vec![needing_sign_in()], &files);

    let step = helper.sign_in(&agent_way("oauth-personal")).await;
    assert_eq!(step, SignInStep::SignedIn);
    assert_eq!(
        kept[0].agent.authenticated(),
        vec![json!({"methodId": "oauth-personal"})]
    );
    assert_eq!(
        starts.lock().expect("starts")[0]
            .sign_in
            .as_ref()
            .map(|s| s.as_str().to_owned()),
        Some("oauth-personal".to_owned())
    );

    let ended = helper.run("Compare the offers", AT).await;
    assert!(matches!(ended, AgentEnd::Done(w) if w.as_untrusted_str() == "Signed in and done."));
    assert_eq!(
        kept[0].spawned.plans().len(),
        1,
        "the process that signed in took the job"
    );
}

#[tokio::test]
async fn a_way_the_agent_would_not_take_leaves_the_helper_waiting() {
    let files = FakeFiles::new();
    let (mut helper, kept, _) = agent_over("h-1", vec![needing_sign_in()], &files);
    let step = helper.sign_in(&agent_way("gemini-api-key")).await;
    assert_eq!(step, SignInStep::Refused);
    assert_eq!(kept[0].agent.authenticated().len(), 1);
    assert!(!helper.is_open());
}

#[tokio::test]
async fn a_terminal_way_is_handed_to_the_shell_and_the_agent_is_asked_again_after() {
    let files = FakeFiles::new();
    // The second world is the agent after the person signed in in the terminal: it opens.
    let after = turns(vec![vec![say("Back to it."), Act::Stop("end_turn")]]);
    let (mut helper, kept, starts) = agent_over("h-1", vec![needing_sign_in(), after], &files);

    let ended = helper.run("Compare the offers", AT).await;
    let AgentEnd::NeedsSignIn(ways) = ended else {
        panic!("expected a wait for sign-in, got {ended:?}");
    };
    let step = helper.sign_in(&ways[2]).await;
    let SignInStep::OpenTerminal(login) = step else {
        panic!("a terminal way is the shell's to run: {step:?}");
    };
    assert_eq!(login.args, ["--login"]);
    // Nothing was sent to the agent for it, and no process started for it.
    assert!(kept[0].agent.authenticated().is_empty());
    assert_eq!(starts.lock().expect("starts").len(), 1);

    // The shell says the window closed: the agent is asked again, with no way chosen.
    assert!(helper.check().await.is_ok());
    assert!(starts.lock().expect("starts")[1].sign_in.is_none());
    let ended = helper.run("Compare the offers", AT).await;
    assert!(matches!(ended, AgentEnd::Done(w) if w.as_untrusted_str() == "Back to it."));
}

#[tokio::test]
async fn signing_out_uses_the_agents_own_logout_only_when_it_advertised_it() {
    let files = FakeFiles::new();
    // The fake agent advertises no `logout`: the caller runs the logout command instead.
    let (mut helper, _kept, _) = agent_over("h-1", vec![turns(Vec::new())], &files);
    assert_eq!(helper.sign_out().await, SignOutStep::UseLogoutCommand);
    assert!(!helper.is_open());
}

#[tokio::test]
async fn a_crash_ends_that_job_failed_and_the_next_one_starts_a_new_process() {
    let files = FakeFiles::new();
    let crashing = turns(vec![vec![Act::Exit]]);
    let healthy = turns(vec![vec![say("Again."), Act::Stop("end_turn")]]);
    let (mut helper, kept, starts) = agent_over("h-1", vec![crashing, healthy], &files);
    let (mut other, other_kept, _) = agent_over(
        "h-2",
        vec![turns(vec![vec![say("Fine."), Act::Stop("end_turn")]])],
        &files,
    );

    let ended = helper.run("First", AT).await;
    assert_eq!(ended, AgentEnd::Failed(Trouble::StoppedAnswering));
    assert!(!helper.is_open());
    assert_eq!(kept[0].spawned.killed(), 1);

    // Another helper's agent is untouched by it.
    let fine = other.run("Second", AT).await;
    assert!(matches!(fine, AgentEnd::Done(w) if w.as_untrusted_str() == "Fine."));
    assert_eq!(other_kept[0].spawned.killed(), 0);

    // The crashed helper's next job starts a new process, asking to pick its session up again.
    let again = helper.run("First again", AT).await;
    assert!(matches!(again, AgentEnd::Done(w) if w.as_untrusted_str() == "Again."));
    assert_eq!(
        starts.lock().expect("starts")[1].resume.as_deref(),
        Some(super::agent::AGENT_SESSION)
    );
}

#[tokio::test]
async fn a_standing_grant_belongs_to_the_helper_that_made_it() {
    let files = FakeFiles::new();
    files.put(&abs("/work/app/README.md"), "# readme\n");
    let script = vec![
        call("read_in", "fs/read_text_file", read("/work/app/README.md")),
        call(
            "write_in",
            "fs/write_text_file",
            write("/work/app/out.txt", "hello"),
        ),
        call(
            "perm1",
            "session/request_permission",
            tool("edit", "Edit notes", &["/work/app/notes.md"], json!({})),
        ),
        Act::Stop("end_turn"),
    ];
    let setup = Setup {
        turns: vec![script],
        // The write once; the permission always.
        answers: vec![once(), always()],
        ..Setup::default()
    };
    let (mut agent, kept, _) = agent_over("h-1", vec![setup], &files);
    let ended = agent.run("Tidy the notes", AT).await;
    assert!(matches!(ended, AgentEnd::Done(_)), "{ended:?}");

    let held = kept[0].router.standing_grants();
    assert_eq!(held.len(), 1);
    assert_eq!(
        held[0].caller,
        GrantCaller::AcpHelper {
            program: program(),
            helper: helper("h-1"),
        }
    );
    // Neither the program's own grants nor another helper's are this one.
    assert_ne!(held[0].caller, GrantCaller::AcpAgent(program()));
    assert_ne!(
        held[0].caller,
        GrantCaller::AcpHelper {
            program: program(),
            helper: helper("h-2"),
        }
    );
}
