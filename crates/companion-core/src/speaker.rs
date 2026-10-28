//! Who may make which call on the companion: the caller policy, as values. How a caller is
//! identified (the callers table, the cgroup behind a bus connection) is companiond's.
//!
//! The shell may do everything. A terminal (`quire-do ask`) may only open, ask and close, stop its
//! own answer and read answers: it is the person at a keyboard, or whatever is typing in one, and
//! that is why it gets no more than the conversation. The editor bridge may read the helpers and
//! speak in a helper's conversation, and nothing else.

use docket_wire::TerminalScope;

/// Who a caller is, as far as the person's voice goes: the policy subject of the companion's bus,
/// not the caller of a request (`docket_vocab::Caller`). It is filled from the callers table
/// and a terminal's scope, and `companiond` turns it into a `Caller` for the turns it records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Speaker {
    /// The shell.
    Shell,
    /// A terminal's child, in this scope.
    Terminal(TerminalScope),
    /// Our own editor bridge: it may read the helpers and speak in their conversations.
    Editor,
}

/// What a caller asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    /// `Open`.
    Open,
    /// `Ask`.
    Ask,
    /// `Close`.
    Close,
    /// `Told`: words to a subagent.
    Told,
    /// `Act`: presses a card.
    Act,
    /// `Answer.Cancel`: stops the task.
    Cancel,
    /// Reading an answer: `View`, `Session` and `Routing`.
    Read,
    /// Anything on `Companion1.Helpers` and `.Helpers2` that changes something (making, editing,
    /// pausing, removing a helper, giving a job, signing in or out, opening a conversation in a
    /// terminal or bringing it back, the routines and what they read), and `.Terminals` but
    /// `Recognise`: they are the person's own, and the shell speaks for them; a terminal and the
    /// editor bridge do not.
    Helpers,
    /// Reading the helpers: the list and a helper's card. The shell and the editor bridge may.
    HelpersRead,
    /// `Helpers2.Say`: the person's words in a helper's conversation, run by the helper. The shell
    /// and the editor bridge may, and only for the conversation (the daemon refuses any other
    /// session): the editor carries the person's words, and the helper's own gate rules the turn.
    Converse,
    /// `Terminals.Recognise`: which listed app a command line is, and which conversation. Reads
    /// the list of apps only; the shell and a terminal may, the editor bridge may not.
    Recognise,
}

/// Whether `speaker` may make `call`: the shell anything, a terminal only the conversation, the
/// editor bridge only reading the helpers and speaking in a helper's conversation.
pub fn permits(speaker: &Speaker, call: Call) -> bool {
    match speaker {
        Speaker::Shell => true,
        Speaker::Editor => matches!(call, Call::HelpersRead | Call::Converse),
        Speaker::Terminal(_) => matches!(
            call,
            Call::Open | Call::Ask | Call::Close | Call::Cancel | Call::Read | Call::Recognise
        ),
    }
}
