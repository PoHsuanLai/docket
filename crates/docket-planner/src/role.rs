//! Role text: what one agent is for, appended to the fixed rules. It comes after `RULES` in the
//! system message and can neither replace nor precede them.

/// The most bytes of role text. It is read at every turn, so it is short on purpose.
pub const ROLE_LIMIT: usize = 4_000;

/// Why a text is not a role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RoleFault {
    /// Nothing but whitespace.
    #[error("the role is empty")]
    Empty,
    /// Longer than [`ROLE_LIMIT`].
    #[error("the role is longer than {ROLE_LIMIT} bytes")]
    TooLong,
}

/// The agent-specific instructions of one planner: written by the code that declares the agent,
/// never by anything the model reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleText(String);

impl RoleText {
    /// A role from `text`, trimmed.
    pub fn parse(text: &str) -> Result<Self, RoleFault> {
        let text = text.trim();
        if text.is_empty() {
            return Err(RoleFault::Empty);
        }
        if text.len() > ROLE_LIMIT {
            return Err(RoleFault::TooLong);
        }
        Ok(Self(text.to_owned()))
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_role_is_trimmed_and_bounded() {
        assert_eq!(
            RoleText::parse("  review mail \n").expect("role").as_str(),
            "review mail"
        );
        assert_eq!(RoleText::parse(" \n"), Err(RoleFault::Empty));
        assert_eq!(
            RoleText::parse(&"x".repeat(ROLE_LIMIT + 1)),
            Err(RoleFault::TooLong)
        );
    }
}
