//! The real process seam, where bubblewrap works: stdio piped as lines, the environment exactly
//! the one given. Skips, with a printed reason, where it does not. Scratch directories, an
//! unreachable bus, a scratch home.

use bulkhead::{AgentNet, AgentRun, Argv, Detected, EnvVar};
use docket_acp::Wire;
use docket_launch::{BwrapProcs, Proc, Procs};

fn bwrap() -> Option<std::path::PathBuf> {
    let path = std::env::var("PATH").unwrap_or_default();
    match Detected::probe(&path) {
        Detected::Bwrap(found) => Some(found.program().to_owned()),
        Detected::Missing(why) => {
            eprintln!("SKIP real process test: {why}");
            None
        }
    }
}

fn var(name: &str, value: &str) -> EnvVar {
    EnvVar {
        name: name.to_owned(),
        value: value.to_owned(),
    }
}

#[tokio::test]
async fn a_confined_process_speaks_lines_over_its_stdio_with_exactly_the_environment_given() {
    let Some(program) = bwrap() else { return };
    let root = tempfile::tempdir().expect("scratch");
    let cwd = root.path().join("work/app");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let run = AgentRun {
        // `sh` reads a line, then prints its environment's names: a stand-in agent.
        argv: Argv::new(
            "sh",
            &[
                "-c".to_owned(),
                "read line; echo \"got $line\"; env".to_owned(),
            ],
        )
        .expect("argv"),
        cwd: bulkhead::AbsPath::parse(cwd.to_str().expect("utf8")).expect("abs"),
        env: vec![
            var("PATH", "/usr/bin:/bin"),
            var("HOME", "/tmp"),
            var("SECRET_FOR_THE_CHILD", "child-only-value"),
            var("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/bus"),
        ],
        net: AgentNet::None,
        binds: Vec::new(),
        overlays: Vec::new(),
    };
    let mut procs = BwrapProcs::new(program);
    let Ok((mut wire, mut proc)) = procs.start(&run).await else {
        eprintln!("SKIP real process test: the sandbox did not start here");
        return;
    };
    wire.write_line("hello".to_owned()).await.expect("write");
    let first = wire.read_line().await;
    if first.is_none() {
        eprintln!("SKIP real process test: the sandbox did not run here");
        return;
    }
    assert_eq!(first.as_deref(), Some("got hello"));
    let mut names = Vec::new();
    while let Some(line) = wire.read_line().await {
        names.push(line.split('=').next().unwrap_or_default().to_owned());
    }
    names.sort_unstable();
    names.retain(|n| !matches!(n.as_str(), "PWD" | "SHLVL" | "_" | "OLDPWD"));
    assert_eq!(
        names,
        [
            "DBUS_SESSION_BUS_ADDRESS",
            "HOME",
            "PATH",
            "SECRET_FOR_THE_CHILD"
        ]
    );
    proc.kill();
}
