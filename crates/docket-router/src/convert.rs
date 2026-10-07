//! The router's `Taint` and `CloseCause` and the durable record's own (`docket-session` does not
//! depend on the router, so each side has its types and these are the only crossings). Each
//! match names every variant, so adding one to either side stops the build here.

use crate::session::{CloseCause, Taint};
use docket_session::{EndCause, Taint as Written};

impl From<Taint> for Written {
    fn from(taint: Taint) -> Self {
        match taint {
            Taint::Clean => Written::Clean,
            Taint::Tainted => Written::Tainted,
        }
    }
}

impl From<Written> for Taint {
    fn from(taint: Written) -> Self {
        match taint {
            Written::Clean => Taint::Clean,
            Written::Tainted => Taint::Tainted,
        }
    }
}

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
    fn taint_and_close_causes_cross_both_ways_unchanged() {
        for taint in [Taint::Clean, Taint::Tainted] {
            assert_eq!(Taint::from(Written::from(taint)), taint);
        }
        for cause in [
            CloseCause::Closed,
            CloseCause::SpaceHalted,
            CloseCause::WallExhausted,
        ] {
            assert_eq!(CloseCause::from(EndCause::from(cause)), cause);
        }
        assert!(Written::Clean < Written::Tainted && Taint::Clean != Taint::Tainted);
    }
}
