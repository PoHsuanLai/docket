//! What the sandbox of a started agent binds: the tool edge, the buses it never sees and the
//! read-only git directories.

use super::support::{ALLOWLIST, ENDPOINT, HANDOFF, HANDOFF_FILE, LOGIN, plan, rig};
use bulkhead::{Access, AgentNet, Profile, agent_bwrap_args};
use docket_acp_host::{AgentChild, Spawn};
use docket_launch::fake::Mood;

#[tokio::test]
async fn the_tool_edge_is_bound_in_the_bridge_read_only_and_its_socket_read_write() {
    let mut rig = rig(LOGIN, Mood::Working, true);
    let mut with_edge = plan("claude-code", "s-1");
    with_edge.edge = Some(docket_acp_host::EdgeBind {
        socket: super::support::abs("/run/user/1000/docket-edge-x/tools.sock"),
        bridge: super::support::abs("/opt/docket/actions-mcp"),
    });
    let mut spawned = rig.spawn.spawn(&with_edge).await.expect("spawned");
    let runs = rig.procs.runs();
    let binds: Vec<(&str, Access)> = runs[0]
        .binds
        .iter()
        .map(|b| (b.path.as_str(), b.access))
        .collect();
    assert!(
        binds.contains(&("/opt/docket/actions-mcp", Access::ReadOnly)),
        "{binds:?}"
    );
    assert!(
        binds.contains(&("/run/user/1000/docket-edge-x/tools.sock", Access::ReadWrite)),
        "{binds:?}"
    );
    // Without an edge, neither is bound.
    spawned.child.close().await;
    let mut rig = super::support::rig(LOGIN, Mood::Working, true);
    let mut spawned = rig
        .spawn
        .spawn(&plan("claude-code", "s-2"))
        .await
        .expect("spawned");
    let runs = rig.procs.runs();
    assert!(
        runs[0]
            .binds
            .iter()
            .all(|b| !b.path.as_str().contains("docket-edge"))
    );
    spawned.child.close().await;
}

/// Whether a path inside the sandbox is the session bus socket, the system bus socket, or a
/// directory above one: binding any of them back would hand the agent a bus.
fn reaches_a_bus(inside: &str) -> bool {
    let session = inside
        .strip_prefix("/run/user/")
        .is_some_and(|rest| rest.split_once('/').is_none_or(|(_, below)| below == "bus"));
    let system = ["/run/dbus", "/run/dbus/system_bus_socket"].contains(&inside);
    ["/", "/run", "/run/user"].contains(&inside) || session || system
}

/// The line the owner set (2026-10-11): an agent never gets a bus. The real keys sit in the
/// Secret Service, which answers on the session bus, so hiding key files is not enough on its
/// own. In every network mode, with a tool edge bound under the person's runtime directory: `/run`
/// is emptied after the host is shown, nothing bound afterwards is a bus socket or a directory
/// above one, and the environment names no bus address.
#[tokio::test]
async fn no_network_mode_gives_the_agent_a_bus() {
    let quiet = LOGIN.replace("network = \"host\"\n", "");
    let cases = [
        (ENDPOINT, "claude-code"),
        (HANDOFF, "gemini-cli"),
        (HANDOFF_FILE, "gemini-cli"),
        (LOGIN, "claude-code"),
        (ALLOWLIST, "claude-code"),
        (quiet.as_str(), "claude-code"),
    ];
    let mut nets = Vec::new();
    for (n, (toml, program)) in cases.into_iter().enumerate() {
        let mut rig = rig(toml, Mood::Working, true);
        let mut with_edge = plan(program, &format!("s-{n}"));
        with_edge.edge = Some(docket_acp_host::EdgeBind {
            socket: super::support::abs("/run/user/1000/docket-edge-x/tools.sock"),
            bridge: super::support::abs("/opt/docket/actions-mcp"),
        });
        let mut spawned = rig.spawn.spawn(&with_edge).await.expect("spawned");
        let run = rig.procs.runs()[0].clone();
        nets.push(run.net.clone());

        for var in &run.env {
            assert!(!var.name.starts_with("DBUS_"), "{}: {}", n, var.name);
            assert!(!var.value.contains("unix:path="), "{}: {}", n, var.name);
        }

        let args = agent_bwrap_args(&run, &Profile::bare());
        assert!(
            !args.windows(3).any(|w| w == ["--ro-bind", "/", "/"]),
            "{n}: the host's whole root is never shown"
        );
        let emptied = args
            .windows(2)
            .position(|w| w == ["--tmpfs", "/run"])
            .expect("/run is emptied");
        let binds = [
            "--bind",
            "--ro-bind",
            "--bind-try",
            "--ro-bind-try",
            "--dev-bind",
        ];
        let end = args.iter().position(|a| a == "--").expect("the command");
        for (i, word) in args[..end].iter().enumerate().skip(emptied) {
            if binds.contains(&word.as_str()) {
                let inside = &args[i + 2];
                assert!(!reaches_a_bus(inside), "{n}: {inside} is bound: {args:?}");
            }
        }
        spawned.child.close().await;
    }
    // Every network mode was covered.
    assert!(nets.iter().any(|n| matches!(n, AgentNet::Endpoint(_))));
    assert!(nets.iter().any(|n| matches!(n, AgentNet::Allowlist(_))));
    assert!(nets.contains(&AgentNet::Host));
    assert!(nets.contains(&AgentNet::None));
}

/// Every `.git` in the writable workspace (its own and a nested one) is bound read-only after the
/// workspace is bound writable, so the agent cannot plant a hook that runs outside.
#[tokio::test]
async fn every_git_directory_in_the_workspace_is_read_only_over_the_writable_bind() {
    let mut rig = rig(LOGIN, Mood::Working, true);
    let work = rig.dir.path().join("proj");
    std::fs::create_dir_all(work.join(".git")).expect("git");
    std::fs::create_dir_all(work.join("vendor/lib/.git")).expect("nested git");
    let mut with_cwd = plan("claude-code", "s-9");
    with_cwd.cwd = super::support::abs(work.to_str().expect("utf8"));
    let mut spawned = rig.spawn.spawn(&with_cwd).await.expect("spawned");
    let run = rig.procs.runs()[0].clone();
    let args = agent_bwrap_args(&run, &Profile::bare());
    let cwd = work.to_str().expect("utf8");
    let writable = args
        .windows(3)
        .position(|w| w == ["--bind", cwd, cwd])
        .expect("the workspace is writable");
    for git in [work.join(".git"), work.join("vendor/lib/.git")] {
        let git = git.to_str().expect("utf8");
        let layered = args
            .windows(3)
            .position(|w| w == ["--ro-bind", git, git])
            .unwrap_or_else(|| panic!("{git} is read-only: {args:?}"));
        assert!(
            layered > writable,
            "{git} is layered over the writable bind"
        );
    }
    spawned.child.close().await;
}
