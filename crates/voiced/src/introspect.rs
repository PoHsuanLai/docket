//! The introspection XML of `org.quire.Voice1`, from the skeletons: the checked-in
//! `dbus/org.quire.Voice1.xml` must equal it.

use crate::bus::{SpeechSkeleton, UtteranceSkeleton, VoiceSkeleton};
use zbus::fdo;
use zbus::object_server::Interface;

/// The file under `dbus/`.
pub const VOICE1_FILE: &str = "org.quire.Voice1.xml";

/// The introspection document: every interface in one `<node>`.
pub fn introspection() -> String {
    let interfaces: Vec<&dyn Interface> = vec![&VoiceSkeleton, &UtteranceSkeleton, &SpeechSkeleton];
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
    fdo::Error::NotSupported("voiced: frozen interface, not implemented".into())
}
