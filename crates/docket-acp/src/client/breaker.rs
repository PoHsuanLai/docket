//! The breaker for one agent connection: pure counts. Denials in a row trip it, and so does a
//! flood of requests in one turn (an agent that asks for permission hundreds of times is trying
//! to wear the person down). Once tripped every further request is refused, no "always" is
//! offered, and the turn ends paused until the person speaks again.

use docket_core::{BreakerState, BreakerTrip};

/// Requests in one turn after which the flood rule trips.
pub const FLOOD_MAX: u32 = 40;
/// Denials in a row after which the consecutive rule trips (`agent.breaker.consecutive`).
pub const DENIALS_MAX: u32 = 5;

/// The counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Breaker {
    denials: u32,
    requests: u32,
    tripped: Option<BreakerTrip>,
}

impl Breaker {
    /// One more request came in.
    pub fn request(&mut self) {
        self.requests = self.requests.saturating_add(1);
        if self.requests > FLOOD_MAX {
            self.tripped.get_or_insert(BreakerTrip::Recent);
        }
    }

    /// A request was refused.
    pub fn denied(&mut self) {
        self.denials = self.denials.saturating_add(1);
        if self.denials >= DENIALS_MAX {
            self.tripped.get_or_insert(BreakerTrip::Consecutive);
        }
    }

    /// A request was let through: the run of denials is over.
    pub fn allowed(&mut self) {
        self.denials = 0;
    }

    /// The person spoke: counts start again and a trip is cleared.
    pub fn new_turn(&mut self) {
        *self = Self::default();
    }

    /// Where it stands.
    pub fn state(&self) -> BreakerState {
        match self.tripped {
            Some(_) => BreakerState::Tripped,
            None => BreakerState::Running,
        }
    }

    /// Why it tripped, if it has.
    pub fn trip(&self) -> Option<BreakerTrip> {
        self.tripped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denials_in_a_row_trip_it_and_an_allow_in_between_does_not() {
        let mut b = Breaker::default();
        for _ in 0..DENIALS_MAX - 1 {
            b.denied();
        }
        b.allowed();
        b.denied();
        assert_eq!(b.state(), BreakerState::Running);
        for _ in 0..DENIALS_MAX {
            b.denied();
        }
        assert_eq!(b.trip(), Some(BreakerTrip::Consecutive));
    }

    #[test]
    fn a_flood_trips_it_and_a_new_turn_clears_it() {
        let mut b = Breaker::default();
        for _ in 0..=FLOOD_MAX {
            b.request();
        }
        assert_eq!(b.trip(), Some(BreakerTrip::Recent));
        b.new_turn();
        assert_eq!(b.state(), BreakerState::Running);
    }
}
