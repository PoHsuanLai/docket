//! `org.quire.Intents1.Error.<Variant>`: the bus form of a request refused before it became a
//! call. A refused *call* (denied, over budget, unconfirmed) is an answer, not an error: it
//! travels in the `Response` body as a `CallRefusal`.

use docket_core::WireRefusal;

/// The errors the router's members return.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.quire.Intents1.Error")]
#[non_exhaustive]
pub enum IntentsError {
    /// zbus's own failures.
    #[zbus(error)]
    ZBus(zbus::Error),
    /// The caller's role may not make this request.
    NotAllowed(String),
    /// The session is unknown or closed.
    NoSuchSession(String),
    /// The request is malformed.
    Malformed(String),
}

impl IntentsError {
    /// The wire refusal this error carries, or `None` for a bus failure.
    pub fn refusal(&self) -> Option<WireRefusal> {
        match self {
            IntentsError::ZBus(_) => None,
            IntentsError::NotAllowed(_) => Some(WireRefusal::NotAllowed),
            IntentsError::NoSuchSession(_) => Some(WireRefusal::NoSuchSession),
            IntentsError::Malformed(_) => Some(WireRefusal::Malformed),
        }
    }

    /// The error for a refusal that is one; `None` for a refused call or message, which are
    /// answers.
    pub fn from_refusal(refusal: &WireRefusal) -> Option<Self> {
        let text = String::new();
        match refusal {
            WireRefusal::NotAllowed => Some(IntentsError::NotAllowed(text)),
            WireRefusal::NoSuchSession => Some(IntentsError::NoSuchSession(text)),
            WireRefusal::Malformed => Some(IntentsError::Malformed(text)),
            WireRefusal::Call(_) | WireRefusal::Send(_) | WireRefusal::Read(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docket_core::{CallRefusal, SendRefusal};
    use zbus::DBusError;

    #[test]
    fn each_error_maps_one_to_one_to_a_refusal() {
        for refusal in [
            WireRefusal::NotAllowed,
            WireRefusal::NoSuchSession,
            WireRefusal::Malformed,
        ] {
            let error = IntentsError::from_refusal(&refusal).expect("an error");
            assert_eq!(error.refusal(), Some(refusal));
        }
    }

    #[test]
    fn refused_calls_and_messages_are_answers_not_errors() {
        assert!(IntentsError::from_refusal(&WireRefusal::Call(CallRefusal::Timeout)).is_none());
        assert!(IntentsError::from_refusal(&WireRefusal::Send(SendRefusal::NoRecipient)).is_none());
    }

    #[test]
    fn error_names_follow_the_name_rule() {
        let names: Vec<String> = [
            IntentsError::NotAllowed(String::new()),
            IntentsError::NoSuchSession(String::new()),
            IntentsError::Malformed(String::new()),
        ]
        .iter()
        .map(|e| e.name().to_string())
        .collect();
        assert_eq!(
            names,
            [
                "org.quire.Intents1.Error.NotAllowed",
                "org.quire.Intents1.Error.NoSuchSession",
                "org.quire.Intents1.Error.Malformed"
            ]
        );
    }
}
