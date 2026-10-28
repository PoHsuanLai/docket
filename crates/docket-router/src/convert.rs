//! The router's `CloseCause` and the durable record's `EndCause` (`docket-session` does not
//! depend on the router, so each side has its type and these are the only crossings). Each
//! match names every variant, so adding one to either side stops the build here.

use crate::session::CloseCause;
use docket_session::EndCause;

impl From<CloseCause> for EndCause {
    fn from(cause: CloseCause) -> Self {
        match cause {
            CloseCause::Closed => EndCause::Closed,
            CloseCause::SpaceHalted => EndCause::SpaceHalted,
            CloseCause::WallExhausted => EndCause::WallExhausted,
        }
    }
}

impl From<EndCause> for CloseCause {
    fn from(cause: EndCause) -> Self {
        match cause {
            EndCause::Closed => CloseCause::Closed,
            EndCause::SpaceHalted => CloseCause::SpaceHalted,
            EndCause::WallExhausted => CloseCause::WallExhausted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_causes_cross_both_ways_unchanged() {
        for cause in [
            CloseCause::Closed,
            CloseCause::SpaceHalted,
            CloseCause::WallExhausted,
        ] {
            assert_eq!(CloseCause::from(EndCause::from(cause)), cause);
        }
    }
}
