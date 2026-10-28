//! Who may make which call.

use companion_core::{Call, Speaker, permits};
use docket_wire::TerminalScope;

fn scope() -> TerminalScope {
    TerminalScope::from_cgroup("vte-spawn-1.scope").expect("scope")
}

#[test]
fn the_shell_may_do_everything_and_a_terminal_only_the_conversation() {
    let all = [
        Call::Open,
        Call::Ask,
        Call::Close,
        Call::Told,
        Call::Act,
        Call::Cancel,
        Call::Read,
        Call::Helpers,
        Call::HelpersRead,
        Call::Converse,
        Call::Recognise,
    ];
    let terminal: Vec<Call> = all
        .into_iter()
        .filter(|c| permits(&Speaker::Terminal(scope()), *c))
        .collect();
    assert_eq!(
        terminal,
        [
            Call::Open,
            Call::Ask,
            Call::Close,
            Call::Cancel,
            Call::Read,
            Call::Recognise
        ]
    );
    assert!(all.into_iter().all(|c| permits(&Speaker::Shell, c)));
    let editor: Vec<Call> = all
        .into_iter()
        .filter(|c| permits(&Speaker::Editor, *c))
        .collect();
    assert_eq!(editor, [Call::HelpersRead, Call::Converse]);
}
