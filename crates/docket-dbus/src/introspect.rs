//! The introspection XML of each bus, from the skeletons: the checked-in `dbus/*.xml` must
//! equal it.

use crate::{
    CompanionAnswerSkeleton, CompanionSkeleton, ConfirmSkeleton, ContextSkeleton, ControlSkeleton,
    GateSkeleton, IndexSkeleton, IntentProviderSkeleton, MessageSkeleton, ReaderSkeleton,
    RegistrySkeleton, RequestSkeleton, RunSkeleton, SearchSkeleton, SessionSkeleton,
};
use zbus::fdo;
use zbus::object_server::Interface;

/// One of the buses declared here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bus {
    /// `org.quire.Intents1` (intentd).
    Intents,
    /// `org.quire.IntentProvider1` (every provider).
    IntentProvider,
    /// `org.quire.Confirm1` (sill).
    Confirm,
    /// `org.quire.Companion1` (companiond).
    Companion,
    /// `org.quire.Reader1` (readerd).
    Reader,
}

impl Bus {
    /// Every bus.
    pub const ALL: [Bus; 5] = [
        Bus::Intents,
        Bus::IntentProvider,
        Bus::Confirm,
        Bus::Companion,
        Bus::Reader,
    ];

    /// The file under `dbus/` holding its introspection.
    pub fn file_name(self) -> &'static str {
        match self {
            Bus::Intents => "org.quire.Intents1.xml",
            Bus::IntentProvider => "org.quire.IntentProvider1.xml",
            Bus::Confirm => "org.quire.Confirm1.xml",
            Bus::Companion => "org.quire.Companion1.xml",
            Bus::Reader => "org.quire.Reader1.xml",
        }
    }
}

/// The introspection document of `bus`: every interface it serves, in one `<node>`.
pub fn introspection(bus: Bus) -> String {
    let interfaces: Vec<&dyn Interface> = match bus {
        Bus::Intents => vec![
            &RegistrySkeleton,
            &IndexSkeleton,
            &SearchSkeleton,
            &RunSkeleton,
            &ContextSkeleton,
            &SessionSkeleton,
            &MessageSkeleton,
            &GateSkeleton,
            &ControlSkeleton,
            &RequestSkeleton,
        ],
        Bus::IntentProvider => vec![&IntentProviderSkeleton],
        Bus::Confirm => vec![&ConfirmSkeleton],
        Bus::Companion => vec![&CompanionSkeleton, &CompanionAnswerSkeleton],
        Bus::Reader => vec![&ReaderSkeleton],
    };
    let mut xml = String::from(
        "<!DOCTYPE node PUBLIC \"-//freedesktop//DTD D-BUS Object Introspection 1.0//EN\"\n \"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd\">\n<node>\n",
    );
    for interface in interfaces {
        interface.introspect_to_writer(&mut xml, 1);
    }
    xml.push_str("</node>\n");
    xml
}

/// The answer of every skeleton method: the interface is frozen, its behaviour not built.
pub(crate) fn frozen() -> fdo::Error {
    fdo::Error::NotSupported("docket: frozen interface, not implemented".into())
}
