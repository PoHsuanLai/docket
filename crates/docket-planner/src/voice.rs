//! The voice a planner speaks in: the role it works under and who asks the model for it. The
//! assistant has one; a helper's conversation has the helper's.

use crate::role::RoleText;
use porter_infer::OpenOptions;

/// The role a planner works under and the options its inference sessions open with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voice {
    /// The agent's role after the fixed rules; none for the assistant's own.
    pub role: Option<RoleText>,
    /// Who asks (a helper), and the one model it must run on.
    pub options: OpenOptions,
}
