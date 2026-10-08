//! An external agent's commands after it read a file (acp-sessions.md section 12, R11): only a
//! command whose arguments derive from what was read loses "always"; a command that can send data
//! out always asks and is never granted, whether or not anything was read.

use crate::acp_agent::*;
use crate::support::*;
use docket_core::*;
use docket_fake::ScriptedConfirmer;
use docket_router::GrantStore;

const PROJ: &str = "/home/u/proj";

fn terminal_grant(w: &World, prefix: &str) {
    let scope = StandingScope::Terminal {
        action: acp_agent_action(TERMINAL_RUN).expect("action"),
        command: CommandPrefix::parse(prefix).expect("prefix"),
        cwd: AbsPath::parse(PROJ).expect("cwd"),
    };
    w.router.seams.grants.add_standing(StandingGrant::new(
        GrantCaller::AcpAgent(program(CLAUDE)),
        scope,
        prov::UnixSeconds(1),
    ));
}

#[tokio::test]
async fn after_a_read_cargo_test_is_granted_always_and_the_second_run_asks_nothing() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![always()]);
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    w.call(&session, read("/home/u/proj/a.rs"))
        .await
        .expect("read");
    w.call(&session, run("cargo test"))
        .await
        .expect("asked, always");
    assert!(
        matches!(w.sheets()[0].always, AlwaysOffer::Offered(_)),
        "{:?}",
        w.sheets()[0].always
    );
    assert_eq!(w.router.standing_grants().len(), 1);
    w.call(&session, run("cargo test --lib"))
        .await
        .expect("on the grant");
    assert_eq!(w.sheets().len(), 1, "no second question");
    assert_eq!(w.used(), 1);
}

#[tokio::test]
async fn a_command_that_uses_what_was_read_asks_with_no_always_even_with_a_grant() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![once(), once()]);
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    terminal_grant(&w, "echo");
    w.call(&session, read("/home/u/proj/a.rs"))
        .await
        .expect("read");
    // Its own words: the grant stands in.
    w.call(&session, run("echo hello"))
        .await
        .expect("on the grant");
    assert_eq!(w.sheets().len(), 0);
    // Derived from the read: it asks, offers no always, and the grant does not stand in.
    for line in ["echo sk_live_Ab12Cd34", "echo /home/u/proj/a.rs"] {
        w.call(&session, run_with(line, DERIVES_READ, NETWORK_CLOSED))
            .await
            .expect("asked, once");
    }
    let sheets = w.sheets();
    assert_eq!(sheets.len(), 2);
    for sheet in sheets {
        assert_eq!(
            sheet.always,
            AlwaysOffer::Withheld(Withheld::UntrustedIntoSink)
        );
    }
    assert_eq!(w.router.standing_grants().len(), 1, "no new grant");
}

#[tokio::test]
async fn a_command_that_can_send_data_out_always_asks_and_is_never_granted() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![once(), once(), once()]);
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    terminal_grant(&w, "curl");
    terminal_grant(&w, "git push");
    // Nothing read, and a matching grant held: it still asks, and offers no always.
    w.call(&session, run("curl https://a.test/x"))
        .await
        .expect("asked, once");
    w.call(&session, run("git push origin main"))
        .await
        .expect("asked, once");
    // After a read too.
    w.call(&session, read("/home/u/proj/a.rs"))
        .await
        .expect("read");
    w.call(&session, run("curl https://a.test/x"))
        .await
        .expect("asked, once");
    let sheets = w.sheets();
    assert_eq!(sheets.len(), 3);
    for sheet in sheets {
        assert_eq!(sheet.always, AlwaysOffer::Withheld(Withheld::CanSendOut));
    }
    assert_eq!(w.used(), 0, "no grant stood in");
}

#[tokio::test]
async fn a_sandbox_with_a_network_makes_every_command_one_that_can_send_data_out() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![once()]);
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    terminal_grant(&w, "cargo test");
    w.call(&session, run_with("cargo test", DERIVES_OWN, NETWORK_OPEN))
        .await
        .expect("asked, once");
    assert_eq!(
        w.sheets()[0].always,
        AlwaysOffer::Withheld(Withheld::CanSendOut)
    );
    assert_eq!(w.used(), 0);
}

#[tokio::test]
async fn a_command_with_a_shell_operator_is_never_covered_by_a_grant() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![once(), once()]);
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    terminal_grant(&w, "cargo test");
    w.call(&session, read("/home/u/proj/a.rs"))
        .await
        .expect("read");
    for line in ["cargo test; echo done", "cargo test && echo done"] {
        w.call(&session, run(line)).await.expect("asked, once");
    }
    assert_eq!(w.sheets().len(), 2);
    assert_eq!(w.used(), 0);
}

#[tokio::test]
async fn a_command_without_the_hosts_facts_is_refused() {
    let mut w = world();
    w.router.seams.confirmer = ScriptedConfirmer::answering(vec![once()]);
    let session = w.open(CLAUDE, SheetSurface::Desktop).await;
    w.call(&session, read("/home/u/proj/a.rs"))
        .await
        .expect("read");
    let mut bare = run("cargo test");
    bare.args.remove(&param(DERIVES_PARAM));
    bare.args.remove(&param(NETWORK_PARAM));
    let refused = w.call(&session, bare).await.expect_err("both are required");
    assert!(
        matches!(refused, CallRefusal::BadArgs { .. }),
        "{refused:?}"
    );
}
