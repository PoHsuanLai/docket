//! Asking the person about something an external agent wants. The seam is `Ask`; the host
//! implements it with its sheet (the desktop's, or the editor's permission prompt). What it is
//! shown comes from the agent and is data: `Shown` strips control characters and bounds the
//! length, and the sheet says the text is the agent's.

use crate::terminal_ask::{Answer, Decide, TerminalAsk};
use docket_core::{AbsPath, AlwaysOffer};
use std::future::Future;

/// The longest text shown of an agent's wording.
pub const SHOWN_MAX: usize = 200;

/// Text the agent wrote, made safe to show: no control characters, at most [`SHOWN_MAX`]
/// characters. It is never an instruction and never a label we computed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown(String);

impl Shown {
    /// `text`, cleaned and cut.
    pub fn of(text: &str) -> Self {
        Self(
            text.chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .take(SHOWN_MAX)
                .collect(),
        )
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What the agent wants to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum What {
    /// Replace or create a file.
    Write(AbsPath),
    /// Run a command in a sandbox.
    Command {
        /// The command line.
        line: String,
        /// Where.
        cwd: AbsPath,
    },
    /// A tool of its own, by its kind (`delete`, `fetch`, `other`) and the paths it names.
    Tool {
        /// The kind's word.
        kind: &'static str,
        /// The paths it names, if any.
        paths: Vec<AbsPath>,
    },
}

/// One question for the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAsk {
    /// The question's number in this connection; asking it again continues the same wait.
    pub id: u64,
    /// What.
    pub what: What,
    /// The agent's own title for it, as data.
    pub title: Shown,
    /// Whether "always" may be offered, and for what.
    pub offer: AlwaysOffer,
}

/// Whoever asks the person. `Clone` because the terminal methods keep a copy.
///
/// **Cancel safety.** The backend may drop the future of `ask` and ask the same question (same
/// `id`) again; an implementation must then go on waiting for the one answer and put no second
/// question on the screen.
pub trait Ask: Send + Clone {
    /// Asks, and waits for the answer.
    fn ask(&mut self, ask: &AgentAsk) -> impl Future<Output = Answer> + Send;
}

/// The number of the call being run, shared with the terminal methods' question adapter so that
/// running the same staged call again asks the same question (see [`Ask`]).
#[derive(Debug, Clone, Default)]
pub struct Epoch(std::sync::Arc<std::sync::atomic::AtomicU64>);

impl Epoch {
    /// Names the call about to run.
    pub fn set(&self, n: u64) {
        self.0.store(n, std::sync::atomic::Ordering::SeqCst);
    }

    fn get(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// `Decide` for the terminal methods, over our `Ask`.
#[derive(Debug, Clone)]
pub struct AsTerminal<A> {
    ask: A,
    epoch: Epoch,
}

impl<A: Ask> AsTerminal<A> {
    /// Terminal questions put through `ask`, numbered by `epoch`.
    pub fn new(ask: A, epoch: Epoch) -> Self {
        Self { ask, epoch }
    }
}

impl<A: Ask> Decide for AsTerminal<A> {
    async fn decide(&mut self, ask: &TerminalAsk) -> Answer {
        let question = AgentAsk {
            id: self.epoch.get(),
            what: What::Command {
                line: ask.line.clone(),
                cwd: ask.cwd.clone(),
            },
            title: Shown::of(&ask.line),
            offer: ask.offer.clone(),
        };
        self.ask.ask(&question).await
    }
}
