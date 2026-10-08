//! Where a turn's progress is told as it happens, and the one place a call can be held back. The
//! drive loop runs the same way whoever listens: companiond's `NoTap` hears nothing and lets
//! every call go; the native session backend (`native`) hears each step, and holds a call at its
//! gate until the one who reads the events has seen it announced.

use companion_wire::NeedsYou;
use docket_core::StepLine;
use docket_session::CallOpen;
use std::future::Future;

/// What the gate says of a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Go {
    /// Make the call: it still goes to the router's gate.
    Run,
    /// The turn was stopped before the call began: it is never made.
    Stop,
}

/// What hears a turn. Every method is called by the drive loop with the companion held, so none
/// of them may wait on the companion.
pub trait Tap: Send {
    /// The planner said something.
    fn said(&mut self, words: &str);

    /// The person is needed.
    fn needs(&mut self, need: &NeedsYou);

    /// A call is about to be made. It is made when this answers `Go::Run` and not before.
    fn gate(&mut self, open: CallOpen) -> impl Future<Output = Go> + Send;

    /// A call ended, as the planner's history shows it.
    fn ended(&mut self, line: &StepLine);
}

/// Hears nothing; lets every call go.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoTap;

impl Tap for NoTap {
    fn said(&mut self, _words: &str) {}

    fn needs(&mut self, _need: &NeedsYou) {}

    fn gate(&mut self, _open: CallOpen) -> impl Future<Output = Go> + Send {
        std::future::ready(Go::Run)
    }

    fn ended(&mut self, _line: &StepLine) {}
}
