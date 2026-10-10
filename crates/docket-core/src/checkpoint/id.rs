//! A restore point's number.

use serde::{Deserialize, Serialize};

/// A restore point's number within one session: 1, 2, 3 ... never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CheckpointId(pub u32);
