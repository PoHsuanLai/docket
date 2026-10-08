//! The agent process's bubblewrap arguments, as plain strings.

use docket_core::AbsPath;
use docket_shell::{
    Access, AgentNet, AgentRun, Argv, Bind, EndpointBind, EnvVar, INSIDE_FORWARDER, INSIDE_SOCKET,
    NetworkMode, agent_bwrap_args,
};

fn abs(text: &str) -> AbsPath {
    AbsPath::parse(text).expect("abs")
}

fn run(net: AgentNet, binds: Vec<Bind>) -> AgentRun {
    AgentRun {
        argv: Argv::new("claude-agent-acp", &["--stdio".to_owned()]).expect("argv"),
        cwd: abs("/work/project"),
        env: vec![EnvVar {
            name: "HOME".to_owned(),
            value: "/home/u".to_owned(),
        }],
        net,
        binds,
    }
}

fn has(args: &[String], seq: &[&str]) -> bool {
    args.windows(seq.len())
        .any(|w| w.iter().map(String::as_str).eq(seq.iter().copied()))
}

#[test]
fn with_no_network_the_namespace_is_new_and_nothing_is_shared() {
    let args = agent_bwrap_args(&run(AgentNet::None, Vec::new()), &["/home", "/tmp"]);
    assert!(has(&args, &["--unshare-all"]));
    assert!(!args.iter().any(|a| a == "--share-net"));
    assert!(has(&args, &["--cap-drop", "ALL"]));
    assert!(has(&args, &["--clearenv"]));
    assert!(has(&args, &["--setenv", "HOME", "/home/u"]));
    assert!(has(&args, &["--bind", "/work/project", "/work/project"]));
    assert!(has(&args, &["--tmpfs", "/home"]));
    assert_eq!(args.last().map(String::as_str), Some("--stdio"));
}

#[test]
fn only_host_mode_shares_the_host_network() {
    let args = agent_bwrap_args(&run(AgentNet::Host, Vec::new()), &[]);
    assert!(has(&args, &["--share-net"]));
}

#[test]
fn endpoint_only_starts_the_forwarder_first_and_binds_one_socket() {
    let bind = EndpointBind {
        forwarder: abs("/opt/docket/docket-net-forward"),
        socket: abs("/run/user/1000/docket/ep.sock"),
        port: 40123,
    };
    let args = agent_bwrap_args(&run(AgentNet::Endpoint(bind), Vec::new()), &[]);
    assert!(!args.iter().any(|a| a == "--share-net"));
    assert!(has(
        &args,
        &[
            "--ro-bind",
            "/opt/docket/docket-net-forward",
            INSIDE_FORWARDER
        ]
    ));
    assert!(has(
        &args,
        &["--bind", "/run/user/1000/docket/ep.sock", INSIDE_SOCKET]
    ));
    let tail: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .skip_while(|a| *a != "--")
        .collect();
    assert_eq!(
        tail,
        [
            "--",
            INSIDE_FORWARDER,
            "run",
            "--listen",
            "127.0.0.1:40123",
            "--socket",
            INSIDE_SOCKET,
            "--",
            "claude-agent-acp",
            "--stdio"
        ]
    );
}

#[test]
fn extra_binds_come_after_the_emptied_directories_and_keep_their_access() {
    let binds = vec![
        Bind {
            path: abs("/home/u/.local/node"),
            access: Access::ReadOnly,
        },
        Bind {
            path: abs("/home/u/.claude"),
            access: Access::ReadWrite,
        },
    ];
    let args = agent_bwrap_args(&run(AgentNet::None, binds), &["/home"]);
    let at = |seq: &[&str]| {
        args.windows(seq.len())
            .position(|w| w.iter().map(String::as_str).eq(seq.iter().copied()))
            .expect("present")
    };
    let hide = at(&["--tmpfs", "/home"]);
    let ro = at(&["--ro-bind", "/home/u/.local/node", "/home/u/.local/node"]);
    let rw = at(&["--bind", "/home/u/.claude", "/home/u/.claude"]);
    assert!(hide < ro && ro < rw);
}

#[test]
fn a_mode_and_its_endpoint_must_agree() {
    let bind = EndpointBind {
        forwarder: abs("/opt/f"),
        socket: abs("/run/s"),
        port: 1,
    };
    assert!(AgentNet::of(NetworkMode::EndpointOnly, None).is_err());
    assert!(AgentNet::of(NetworkMode::None, Some(bind.clone())).is_err());
    assert!(AgentNet::of(NetworkMode::Host, Some(bind.clone())).is_err());
    assert_eq!(
        AgentNet::of(NetworkMode::EndpointOnly, Some(bind))
            .expect("plan")
            .mode(),
        NetworkMode::EndpointOnly
    );
    assert_eq!(NetworkMode::default(), NetworkMode::None);
}
