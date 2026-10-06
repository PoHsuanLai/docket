//! `org.quire.Voice1.Error.<Variant>`: the bus form of a refused call. The six refusals are
//! `VoiceRefusal`'s, one to one; `Malformed` is a body that does not parse and `Gone` is a daemon
//! that is shutting down.

use voice_wire::{ERROR_PREFIX, VoiceRefusal};
use zbus::fdo;

/// The errors the members return.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.quire.Voice1.Error")]
pub enum VoiceError {
    /// zbus's own failures.
    #[zbus(error)]
    ZBus(zbus::Error),
    /// Voice is off.
    Disabled(String),
    /// First-use consent is pending.
    NeedsConsent(String),
    /// Busy.
    Busy(String),
    /// This caller may not.
    NotAllowed(String),
    /// No speech model.
    NoModel(String),
    /// No microphone.
    MicUnavailable(String),
    /// The body does not parse.
    Malformed(String),
    /// The daemon is going away.
    Gone(String),
}

impl From<VoiceRefusal> for VoiceError {
    fn from(refusal: VoiceRefusal) -> Self {
        let why = refusal.to_string();
        match refusal {
            VoiceRefusal::Disabled => VoiceError::Disabled(why),
            VoiceRefusal::NeedsConsent => VoiceError::NeedsConsent(why),
            VoiceRefusal::Busy => VoiceError::Busy(why),
            VoiceRefusal::NotAllowed => VoiceError::NotAllowed(why),
            VoiceRefusal::NoModel => VoiceError::NoModel(why),
            VoiceRefusal::MicUnavailable => VoiceError::MicUnavailable(why),
        }
    }
}

impl From<fdo::Error> for VoiceError {
    fn from(error: fdo::Error) -> Self {
        VoiceError::ZBus(error.into())
    }
}

impl VoiceError {
    /// The refusal this error carries, or `None` for anything else.
    pub fn refusal(&self) -> Option<VoiceRefusal> {
        match self {
            VoiceError::Disabled(_) => Some(VoiceRefusal::Disabled),
            VoiceError::NeedsConsent(_) => Some(VoiceRefusal::NeedsConsent),
            VoiceError::Busy(_) => Some(VoiceRefusal::Busy),
            VoiceError::NotAllowed(_) => Some(VoiceRefusal::NotAllowed),
            VoiceError::NoModel(_) => Some(VoiceRefusal::NoModel),
            VoiceError::MicUnavailable(_) => Some(VoiceRefusal::MicUnavailable),
            VoiceError::ZBus(_) | VoiceError::Malformed(_) | VoiceError::Gone(_) => None,
        }
    }
}

/// The refusal a bus error name stands for (a client's side of the table).
pub fn refusal_of_name(name: &str) -> Option<VoiceRefusal> {
    name.strip_prefix(ERROR_PREFIX)
        .and_then(|_| VoiceRefusal::from_error_name(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::DBusError;

    #[test]
    fn each_refusal_is_one_error_with_the_names_of_the_wire() {
        for refusal in VoiceRefusal::ALL {
            let error = VoiceError::from(refusal);
            assert_eq!(error.refusal(), Some(refusal));
            let name = error.name();
            assert_eq!(name.as_str(), refusal.error_name());
            assert_eq!(refusal_of_name(name.as_str()), Some(refusal));
        }
        assert_eq!(refusal_of_name("org.freedesktop.DBus.Error.Failed"), None);
        assert_eq!(VoiceError::Malformed(String::new()).refusal(), None);
    }
}
