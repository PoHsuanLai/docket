//! The session machine (call lifecycle §4.3 with the breaker): a session is open, clean or
//! tainted (taint never goes back), paused after the breaker trips until the person speaks, or
//! closed.

use docket_core::{BreakerTrip, CallRefusal};

/// Whether the session read anything untrusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Taint {
    /// Nothing untrusted was revealed.
    Clean,
    /// Something was, and it stays so.
    Tainted,
}

/// Why a session closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CloseCause {
    /// The caller closed it.
    Closed,
    /// Its Space was halted.
    SpaceHalted,
    /// The wall budget ran out.
    WallExhausted,
}

/// Where a session stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionState {
    /// Taking calls.
    Open(Taint),
    /// The breaker tripped: only a new user turn resumes it.
    Paused {
        /// What it read before.
        taint: Taint,
        /// Why it paused.
        trip: BreakerTrip,
    },
    /// Over.
    Closed(CloseCause),
}

/// What happens to a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionEvent {
    /// The first plain untrusted value reached a model.
    UntrustedReveal,
    /// The breaker tripped.
    BreakerTrip(BreakerTrip),
    /// The person said something.
    UserTurn,
    /// A call arrived.
    Perform,
    /// The caller closed it.
    Close,
    /// Its Space was halted.
    SpaceHalted,
    /// The wall budget ran out.
    WallExhausted,
}

/// What the router does about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEffect {
    /// Signal `BreakerTripped`; the orb shows waiting-for-you.
    EmitBreakerTripped(BreakerTrip),
    /// A resume after a trip resets the breaker.
    ResetBreaker,
    /// Refuse the call that arrived.
    Refuse(CallRefusal),
}

/// One transition. Total and pure.
pub fn session_step(
    state: SessionState,
    event: SessionEvent,
) -> (SessionState, Vec<SessionEffect>) {
    use SessionEffect as E;
    use SessionEvent as V;
    use SessionState as S;
    match (state, event) {
        (S::Closed(cause), _) => (S::Closed(cause), vec![]),
        (_, V::Close) => (S::Closed(CloseCause::Closed), vec![]),
        (_, V::SpaceHalted) => (S::Closed(CloseCause::SpaceHalted), vec![]),
        (_, V::WallExhausted) => (S::Closed(CloseCause::WallExhausted), vec![]),
        (S::Open(_), V::UntrustedReveal) => (S::Open(Taint::Tainted), vec![]),
        (S::Paused { trip, .. }, V::UntrustedReveal) => (
            S::Paused {
                taint: Taint::Tainted,
                trip,
            },
            vec![],
        ),
        (S::Open(taint), V::BreakerTrip(trip)) => {
            (S::Paused { taint, trip }, vec![E::EmitBreakerTripped(trip)])
        }
        (paused @ S::Paused { .. }, V::BreakerTrip(_)) => (paused, vec![]),
        (S::Paused { taint, .. }, V::UserTurn) => (S::Open(taint), vec![E::ResetBreaker]),
        (open @ S::Open(_), V::UserTurn | V::Perform) => (open, vec![]),
        (paused @ S::Paused { trip, .. }, V::Perform) => {
            (paused, vec![E::Refuse(CallRefusal::Paused(trip))])
        }
    }
}
