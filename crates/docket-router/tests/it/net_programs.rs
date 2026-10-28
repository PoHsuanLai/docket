//! The shipped list of network programs: what it makes of the commands people run.

use docket_permissions_core::{NetAccess, NetPrograms, NetReach};

fn shipped() -> NetPrograms {
    toml::from_str(include_str!("../../../../dist/network-programs.toml"))
        .expect("the shipped list reads")
}

#[test]
fn the_shipped_list_sorts_the_commands() {
    use NetReach::{None as No, Possible as Yes};
    let cases = [
        ("curl https://a.test/x", Yes),
        ("/usr/bin/curl -s a.test", Yes),
        ("wget a.test", Yes),
        ("ssh host ls", Yes),
        ("scp a host:b", Yes),
        ("rsync -a . host:/b", Yes),
        ("nc -l 9", Yes),
        ("git push origin main", Yes),
        ("git -C /home/u/proj push", Yes),
        ("git fetch", Yes),
        ("git clone x", Yes),
        ("git pull", Yes),
        ("git status", No),
        ("git diff HEAD~1", No),
        ("git log --oneline", No),
        ("cargo publish", Yes),
        ("cargo install ripgrep", Yes),
        ("cargo build", No),
        ("cargo test --lib", No),
        ("cargo clippy", No),
        ("npm publish", Yes),
        ("npm install left-pad", Yes),
        ("npm test", No),
        ("npm run build", No),
        ("pip install x", Yes),
        ("pip list", No),
        ("ls -la", No),
        ("cat Cargo.toml", No),
        ("echo hello", No),
        ("cd src", No),
        ("pwd", No),
        ("sh -c 'curl a.test'", Yes),
        ("env FOO=1 wget a.test", Yes),
        ("echo hi; curl a.test", Yes),
        ("ls | nc host 9", Yes),
        ("ls && cargo test", No),
        ("cat curl.txt", No),
        ("echo $(ssh h)", Yes),
    ];
    let programs = shipped();
    for (line, want) in cases {
        assert_eq!(
            NetReach::of(line, NetAccess::Closed, &programs),
            want,
            "{line}"
        );
    }
}
