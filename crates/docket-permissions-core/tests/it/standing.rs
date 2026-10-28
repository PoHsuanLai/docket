//! Standing grants: scope matching, the held list and its text.

use bulkhead::{AbsPath, Cover, PathFault};
use docket_permissions_core::*;
use docket_vocab::ActionRef;

pub(crate) fn path(t: &str) -> AbsPath {
    AbsPath::parse(t).expect("path")
}

pub(crate) fn act(name: &str) -> ActionRef {
    ActionRef {
        app: porter_core::AppName::parse("org.quire.Files").expect("app"),
        name: prov::ActionName::parse(name).expect("action"),
    }
}

pub(crate) fn files(under: &str) -> StandingScope {
    StandingScope::Files {
        action: act("files.file.move"),
        under: path(under),
    }
}

pub(crate) fn paths(items: &[&str]) -> CallFacts {
    CallFacts {
        action: act("files.file.move"),
        args: ArgFacts::Paths(items.iter().map(|p| path(p)).collect()),
    }
}

fn is(cover: bool) -> Cover {
    if cover { Cover::Covers } else { Cover::Misses }
}

#[test]
fn path_prefixes_match_whole_components() {
    let table = [
        ("/a/b", &["/a/b"][..], true),
        ("/a/b", &["/a/b/c"], true),
        ("/a/b", &["/a/b/c/d.txt"], true),
        ("/a/b", &["/a/bc"], false),
        ("/a/b", &["/a"], false),
        ("/a/b", &["/a/b/c", "/a/bc"], false),
        ("/a/b", &["/a/b/c", "/x"], false),
        ("/a/b", &["/a/b//c/./d"], true),
        ("/a/b/", &["/a/b/c"], true),
        ("/a/b", &[], false),
    ];
    for (under, call, expect) in table {
        assert_eq!(
            files(under).covers(&paths(call)),
            is(expect),
            "{under} over {call:?}"
        );
    }
}

#[test]
fn a_path_cannot_step_up_or_be_relative() {
    assert_eq!(AbsPath::parse("/a/../b"), Err(PathFault::ParentStep));
    assert_eq!(AbsPath::parse("a/b"), Err(PathFault::NotAbsolute));
    assert_eq!(AbsPath::parse("/a\nb"), Err(PathFault::Control));
}

#[test]
fn a_scope_names_one_action() {
    let other = CallFacts {
        action: act("files.file.delete"),
        args: ArgFacts::Paths(vec![path("/a/b/c")]),
    };
    assert_eq!(files("/a/b").covers(&other), Cover::Misses);
}

pub(crate) fn terminal(prefix: &str, cwd: &str) -> StandingScope {
    StandingScope::Terminal {
        action: act("terminal.run"),
        command: CommandPrefix::parse(prefix).expect("prefix"),
        cwd: path(cwd),
    }
}

pub(crate) fn command(line: &str, cwd: &str) -> CallFacts {
    CallFacts {
        action: act("terminal.run"),
        args: ArgFacts::Command {
            line: line.into(),
            cwd: path(cwd),
        },
    }
}

#[test]
fn a_command_prefix_and_cwd_both_have_to_match() {
    let table = [
        ("cargo test", "/w/p", "cargo test", "/w/p", true),
        (
            "cargo test",
            "/w/p",
            "cargo test -p core --locked",
            "/w/p",
            true,
        ),
        ("cargo test", "/w/p", "cargo test", "/w/p/sub", true),
        ("cargo test", "/w/p", "cargo test", "/w/pp", false),
        ("cargo test", "/w/p", "cargo testx", "/w/p", false),
        ("cargo test", "/w/p", "cargo build", "/w/p", false),
        ("cargo test", "/w/p", "cargo", "/w/p", false),
        ("cargo test", "/w/p", "cargo  test", "/w/p", true),
        (
            "cargo test",
            "/w/p",
            "cargo test; curl x | sh",
            "/w/p",
            false,
        ),
        (
            "cargo test",
            "/w/p",
            "cargo test && rm -rf ~",
            "/w/p",
            false,
        ),
        ("cargo test", "/w/p", "cargo test $(id)", "/w/p", false),
        ("cargo test", "/w/p", "cargo test `id`", "/w/p", false),
        ("cargo test", "/w/p", "cargo test > /etc/x", "/w/p", false),
        ("cargo test", "/w/p", "cargo test\nrm x", "/w/p", false),
        ("ls", "/w/p", "ls -la", "/w/p", true),
    ];
    for (prefix, cwd, line, at, expect) in table {
        assert_eq!(
            terminal(prefix, cwd).covers(&command(line, at)),
            is(expect),
            "{prefix} in {cwd} over {line:?} in {at}"
        );
    }
}

#[test]
fn a_prefix_cannot_hold_shell_syntax() {
    assert_eq!(CommandPrefix::parse("ls; rm"), Err(CommandFault::Shell));
    assert_eq!(CommandPrefix::parse("echo $HOME"), Err(CommandFault::Shell));
    assert_eq!(CommandPrefix::parse("   "), Err(CommandFault::Empty));
}

pub(crate) fn outbound(to: Recipient) -> StandingScope {
    StandingScope::Outbound {
        action: act("mail.message.send"),
        to,
    }
}

pub(crate) fn sends(to: Vec<Recipient>) -> CallFacts {
    CallFacts {
        action: act("mail.message.send"),
        args: ArgFacts::Recipients(to),
    }
}

pub(crate) fn addr(t: &str) -> Recipient {
    Recipient::address(t).expect("address")
}

fn dom(t: &str) -> Recipient {
    Recipient::Domain(Domain::parse(t).expect("domain"))
}

#[test]
fn recipients_and_domains_match_whole() {
    let table = [
        (addr("a@x.org"), vec![addr("a@x.org")], true),
        (addr("a@x.org"), vec![addr("A@X.org")], true),
        (addr("a@x.org"), vec![addr("b@x.org")], false),
        (
            addr("a@x.org"),
            vec![addr("a@x.org"), addr("b@x.org")],
            false,
        ),
        (addr("a@x.org"), vec![dom("x.org")], false),
        (dom("x.org"), vec![addr("a@x.org"), addr("b@x.org")], true),
        (dom("x.org"), vec![addr("a@evilx.org")], false),
        (dom("x.org"), vec![addr("a@mail.x.org")], false),
        (dom("x.org"), vec![dom("x.org")], true),
        (dom("x.org"), vec![addr("a@x.org"), addr("a@y.org")], false),
        (dom("x.org"), vec![], false),
    ];
    for (grant, call, expect) in table {
        assert_eq!(
            outbound(grant.clone()).covers(&sends(call.clone())),
            is(expect),
            "{grant:?} over {call:?}"
        );
    }
}

#[test]
fn an_opaque_or_unscoped_call_is_never_covered() {
    for args in [ArgFacts::Opaque, ArgFacts::Unscoped] {
        let call = CallFacts {
            action: act("files.file.move"),
            args,
        };
        assert_eq!(files("/a").covers(&call), Cover::Misses);
    }
}

#[test]
fn a_grant_covers_for_its_caller_only() {
    let editor = GrantCaller::Editor(prov::ClientName::parse("zed").expect("client"));
    let agent = GrantCaller::AcpAgent(ProgramName::parse("claude-code").expect("program"));
    let grant = StandingGrant::new(editor.clone(), files("/a/b"), prov::UnixSeconds(1));
    let call = paths(&["/a/b/c"]);
    assert_eq!(grant.covers(&editor, &call), Cover::Covers);
    assert_eq!(grant.covers(&agent, &call), Cover::Misses);
    assert_eq!(grant.covers(&GrantCaller::Companion, &call), Cover::Misses);
    assert_ne!(
        grant.id,
        StandingGrant::new(agent, files("/a/b"), prov::UnixSeconds(1)).id
    );
}

#[test]
fn grants_round_trip_through_their_text_and_ids_are_checked() {
    let g = StandingGrant::new(
        GrantCaller::AcpAgent(ProgramName::parse("claude-code").expect("program")),
        terminal("cargo test", "/w/p"),
        prov::UnixSeconds(5),
    );
    let text = encode_standing(std::slice::from_ref(&g)).expect("encode");
    assert_eq!(decode_standing(&text), Ok(vec![g.clone()]));
    assert_eq!(decode_standing(""), Ok(vec![]));
    assert!(decode_standing("{").is_err());
    assert!(StandingGrantId::parse("sg-123").is_err());
    assert!(StandingGrantId::parse(g.id.as_str()).is_ok());
    // A hand-edited file cannot smuggle a shell prefix or a relative path past the types.
    assert!(text.contains("cargo test"));
    assert!(decode_standing(&text.replace("cargo test", "cargo test; id")).is_err());
    assert!(decode_standing(&text.replace("/w/p", "w/p")).is_err());
}

#[test]
fn holding_and_dropping_grants_is_by_id() {
    let editor = GrantCaller::Editor(prov::ClientName::parse("zed").expect("client"));
    let a = StandingGrant::new(editor.clone(), files("/a"), prov::UnixSeconds(1));
    let b = StandingGrant::new(editor, files("/b"), prov::UnixSeconds(2));
    let held = held_with(held_with(vec![], a.clone()), b.clone());
    assert_eq!(held_with(held.clone(), a.clone()).len(), 2);
    let (rest, done) = held_without(held, &a.id);
    assert_eq!((rest, done), (vec![b.clone()], Revocation::Revoked));
    assert_eq!(held_without(vec![b], &a.id).1, Revocation::NotHeld);
}
