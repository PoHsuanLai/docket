//! The host's own count of refusals that never reached the router: a path outside the session's
//! directory, one that holds secrets, a request that is not a call. The router's breaker counts
//! what it ruled on; an agent that keeps trying places it may not reach is never ruled on, so
//! this counts them, and the turn pauses until the person speaks. Pure counts.

use docket_core::BreakerTrip;

/// Refusals in a row after which the turn pauses.
pub const STRIKES_MAX: u32 = 5;

/// The count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Strikes {
    run: u32,
    tripped: bool,
}

impl Strikes {
    /// A request was refused before it was a call.
    pub fn refused(&mut self) {
        self.run = self.run.saturating_add(1);
        if self.run >= STRIKES_MAX {
            self.tripped = true;
        }
    }

    /// A call reached the router and was let through: the run is over.
    pub fn let_through(&mut self) {
        self.run = 0;
    }

    /// The person spoke: start again.
    pub fn new_turn(&mut self) {
        *self = Self::default();
    }

    /// Why the turn pauses, if it does.
    pub fn trip(&self) -> Option<BreakerTrip> {
        self.tripped.then_some(BreakerTrip::Consecutive)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_in_a_row_trip_it_and_a_call_let_through_in_between_does_not() {
        let mut s = Strikes::default();
        for _ in 0..STRIKES_MAX - 1 {
            s.refused();
        }
        s.let_through();
        s.refused();
        assert_eq!(s.trip(), None);
        for _ in 0..STRIKES_MAX {
            s.refused();
        }
        assert_eq!(s.trip(), Some(BreakerTrip::Consecutive));
        s.new_turn();
        assert_eq!(s.trip(), None);
    }
}
