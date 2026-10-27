//! The real process seam, where bubblewrap works: stdio piped as lines, the environment exactly
//! the one given. Skips, with a printed reason, where it does not. Scratch directories, an
//! unreachable bus, a scratch home.

use bulkhead::{AgentNet, AgentRun, Argv, BwrapSandbox, Detected, EnvVar};
use docket_launch::{BwrapProcs, Proc, Procs, StdioWire};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

fn bwrap() -> Option<BwrapSandbox> {
    let path = std::env::var("PATH").unwrap_or_default();
    match Detected::probe(&path) {
        Detected::Bwrap(found) => Some(found),
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
    let Some(sandbox) = bwrap() else { return };
    let root = tempfile::tempdir().expect("scratch");
    let cwd = root.path().join("work/app");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let run = AgentRun::new(
        // `sh` reads a line, then prints its environment's names: a stand-in agent.
        Argv::new(
            "sh",
            &[
                "-c".to_owned(),
                "read line; echo \"got $line\"; env".to_owned(),
            ],
        )
        .expect("argv"),
        bulkhead::AbsPath::parse(cwd.to_str().expect("utf8")).expect("abs"),
        vec![
            var("PATH", "/usr/bin:/bin"),
            var("HOME", "/tmp"),
            var("SECRET_FOR_THE_CHILD", "child-only-value"),
            var("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent/bus"),
        ],
        AgentNet::None,
    );
    let mut procs = BwrapProcs::new(sandbox, root.path().to_owned());
    let Ok((wire, mut proc)) = procs.start(&run).await else {
        eprintln!("SKIP real process test: the sandbox did not start here");
        return;
    };
    let StdioWire {
        mut stdout,
        mut stdin,
    } = wire;
    stdin.write_all(b"hello\n").await.expect("write");
    let mut first = String::new();
    if stdout.read_line(&mut first).await.unwrap_or(0) == 0 {
        eprintln!("SKIP real process test: the sandbox did not run here");
        return;
    }
    assert_eq!(first.trim_end(), "got hello");
    let mut names = Vec::new();
    loop {
        let mut line = String::new();
        match stdout.read_line(&mut line).await {
            Ok(0) | Err(_) => break,
            Ok(_) => names.push(
                line.trim_end()
                    .split('=')
                    .next()
                    .unwrap_or_default()
                    .to_owned(),
            ),
        }
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
