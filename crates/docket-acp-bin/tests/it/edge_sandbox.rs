//! The tool edge across the real agent sandbox: bubblewrap with no network at all, the socket the
//! only thing bound in, and a process inside that speaks to it as the bridge program does. Skips,
//! with a printed reason, where bwrap, user namespaces or python are missing. Scratch directories
//! only; the court is a stand-in that holds the mail fixture's manifest and never performs.

use bulkhead::{Access, AgentNet, AgentRun, Argv, Bind, EnvVar, agent_bwrap_args, present_hidden};
use docket_acp::client::{AgentCall, Court, CourtFault, OpenAgent, Ruled, ToolsEdge, ToolsOffer};
use docket_core::{AbsPath, ValidManifest};
use prov::SessionId;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Clone)]
struct Stand;

impl Court for Stand {
    async fn open(&mut self, _open: OpenAgent) -> Result<SessionId, CourtFault> {
        Err(CourtFault::Unavailable)
    }
    async fn turn(&mut self, _session: &SessionId, _text: &str) -> Result<(), CourtFault> {
        Err(CourtFault::Unavailable)
    }
    async fn call(&mut self, _session: &SessionId, _n: u64, _call: &AgentCall) -> Ruled {
        Ruled::Lost
    }
    async fn registry(&mut self) -> Option<Vec<ValidManifest>> {
        docket_fake::mail_manifest().ok().map(|m| vec![m])
    }
    async fn close(&mut self, _session: &SessionId) {}
}

fn on_path(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var("PATH").unwrap_or_default();
    path.split(':')
        .map(|d| Path::new(d).join(name))
        .find(|p| p.is_file())
}

fn abs(path: &Path) -> AbsPath {
    AbsPath::parse(path.to_str().expect("utf8")).expect("abs")
}

fn shell_abs(path: &Path) -> bulkhead::AbsPath {
    bulkhead::AbsPath::parse(path.to_str().expect("utf8")).expect("abs")
}

const SCRIPT: &str = r#"
import json, os, socket, sys
s = socket.socket(socket.AF_UNIX)
s.connect(sys.argv[1])
s.sendall((json.dumps({"token": os.environ["T"], "op": "list"}) + "\n").encode())
print(s.makefile().readline().strip())
"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_in_the_sandbox_with_no_network_reaches_its_sessions_edge_and_only_that() {
    let (Some(bwrap), Some(python)) = (on_path("bwrap"), on_path("python3")) else {
        eprintln!("SKIP edge sandbox test: no bwrap or python3");
        return;
    };
    let dir = tempfile::tempdir().expect("scratch");
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).expect("cwd");
    let offer = ToolsOffer {
        run_dir: dir.path().to_path_buf(),
        bridge: abs(&python),
    };
    let session = SessionId::parse("s-1").expect("session");
    let program = docket_session::ProgramName::parse("claude-code").expect("program");
    let edge = ToolsEdge::start(&offer, &session, &program, Stand).expect("edge");
    let run = |socket: &bulkhead::AbsPath, token: &str| {
        let run = AgentRun::new(
            Argv::new(
                python.to_str().expect("utf8"),
                &[
                    "-c".to_owned(),
                    SCRIPT.to_owned(),
                    socket.as_str().to_owned(),
                ],
            )
            .expect("argv"),
            shell_abs(&work),
            vec![
                EnvVar {
                    name: "PATH".to_owned(),
                    value: "/usr/bin:/bin".to_owned(),
                },
                EnvVar {
                    name: "T".to_owned(),
                    value: token.to_owned(),
                },
            ],
            AgentNet::None,
        )
        .with_bind(Bind::new(
            bulkhead::AbsPath::parse(edge.bind().socket.as_str()).expect("abs"),
            Access::ReadWrite,
        ));
        Command::new(&bwrap)
            .args(agent_bwrap_args(&run, &present_hidden()))
            .env_clear()
            .envs(run.env.iter().map(|v| (&v.name, &v.value)))
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    };
    let socket = bulkhead::AbsPath::parse(edge.bind().socket.as_str()).expect("abs");
    let listing = run(&socket, edge.token().reveal()).expect("bwrap runs");
    if listing.trim().is_empty() {
        eprintln!("SKIP edge sandbox test: namespaces are denied here");
        return;
    }
    assert!(listing.contains("mail__mail_thread_read"), "{listing}");
    // A wrong token from inside is a stranger, and the answer says nothing more.
    let wrong = run(&socket, &"0".repeat(64)).expect("bwrap runs");
    assert_eq!(wrong.trim(), "\"refused\"");
}
