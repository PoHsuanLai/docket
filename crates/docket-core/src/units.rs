//! Units: every number that means something has a type. Instants, counts and money are
//! porter-core's (`UnixSeconds`, `Count`, `MicroUsd`, `Permille`); these are the ones docket adds.

use serde::{Deserialize, Serialize};

macro_rules! unit {
    ($(#[$doc:meta])* $name:ident($inner:ty)) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub $inner);
    };
}

unit!(
    /// A length of time in whole seconds (a confirmation's expiry, a wall budget).
    Seconds(u32)
);
unit!(
    /// A length of time in milliseconds (a reviewer's latency, a stage timeout).
    Millis(u32)
);
unit!(
    /// A length of time in whole days (how long a restore point is kept).
    Days(u32)
);
unit!(
    /// A number of characters (a text limit, a size shown to a reviewer).
    CharCount(u32)
);
unit!(
    /// How deep a follow-up chain is.
    Depth(u8)
);
unit!(
    /// The generation of a search: a newer query supersedes an older one.
    Generation(u64)
);
unit!(
    /// Digits after the point of a [`Decimal`](crate::Decimal).
    Scale(u8)
);
