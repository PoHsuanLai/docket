//! One look at the registry: the audio nodes now present and the default source.

use crate::default_source::DefaultSources;
use pipewire as pw;
use std::time::Duration;
use voice_audio::{AudioNode, MediaClass, NodeId, NodeKind};

/// The node kind PipeWire's `media.class` and name say.
fn kind_of(class: &str, name: &str) -> Option<NodeKind> {
    match class {
        "Audio/Source" if name.ends_with(".monitor") => Some(NodeKind::Monitor),
        "Audio/Source" => Some(NodeKind::Source),
        "Audio/Source/Virtual" => Some(NodeKind::Monitor),
        "Audio/Sink" => Some(NodeKind::Sink),
        _ => None,
    }
}

/// The audio nodes now present and what the `default` metadata object says is the default source.
/// Two round trips: the first lists the globals (binding the metadata object as it appears), the
/// second lets the bound metadata deliver its properties.
pub(crate) fn list_nodes(budget: Duration) -> Result<(Vec<AudioNode>, DefaultSources), pw::Error> {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;
    let registry = core.get_registry_rc()?;
    let found = Rc::new(RefCell::new(Vec::new()));
    let defaults = Rc::new(RefCell::new(DefaultSources::default()));
    let bound = Rc::new(RefCell::new(Vec::new()));
    let round = Rc::new(Cell::new(0_u8));
    let first = core.sync(0)?;
    let second = Rc::new(Cell::new(None));
    let (round_in, second_in, quit, core_in) = (
        round.clone(),
        second.clone(),
        mainloop.clone(),
        core.clone(),
    );
    let _core_listener = core
        .add_listener_local()
        .done(move |id, seq| {
            if id != pw::core::PW_ID_CORE {
                return;
            }
            if seq == first {
                second_in.set(core_in.sync(0).ok());
            } else if Some(seq) == second_in.get() {
                round_in.set(2);
                quit.quit();
            }
        })
        .register();
    let (sink, sink_defaults, sink_bound, bind_from) = (
        found.clone(),
        defaults.clone(),
        bound.clone(),
        registry.clone(),
    );
    let _registry_listener = registry
        .add_listener_local()
        .global(move |global| {
            let Some(props) = global.props.as_ref() else {
                return;
            };
            match global.type_ {
                pw::types::ObjectType::Node => {
                    let class = props.get("media.class").unwrap_or_default();
                    let name = props.get("node.name").unwrap_or_default();
                    if let Some(kind) = kind_of(class, name) {
                        sink.borrow_mut().push(AudioNode {
                            id: NodeId(global.id),
                            name: name.to_owned(),
                            kind,
                            media_class: MediaClass(class.to_owned()),
                        });
                    }
                }
                pw::types::ObjectType::Metadata
                    if props.get("metadata.name") == Some("default") =>
                {
                    let Ok(metadata) = bind_from.bind::<pw::metadata::Metadata, _>(global) else {
                        return;
                    };
                    let seen = sink_defaults.clone();
                    let listener = metadata
                        .add_listener_local()
                        .property(move |_subject, key, _type, value| {
                            if let Some(key) = key {
                                seen.borrow_mut().note(key, value);
                            }
                            0
                        })
                        .register();
                    sink_bound.borrow_mut().push((metadata, listener));
                }
                _ => {}
            }
        })
        .register();
    // The budget ends the wait on the loop itself, so a registry that never answers cannot hold
    // the blocking thread.
    let expiry = mainloop.clone();
    let timer = mainloop.loop_().add_timer(move |_| expiry.quit());
    timer
        .update_timer(Some(budget), None)
        .into_result()
        .map_err(|_| pw::Error::CreationFailed)?;
    mainloop.run();
    if round.get() < 2 {
        return Err(pw::Error::CreationFailed);
    }
    let nodes = found.borrow().clone();
    let defaults = defaults.borrow().clone();
    Ok((nodes, defaults))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_plain_source_is_a_microphone() {
        let table = [
            ("Audio/Source", "alsa_input.usb-mic", Some(NodeKind::Source)),
            (
                "Audio/Source",
                "alsa_output.pci.monitor",
                Some(NodeKind::Monitor),
            ),
            (
                "Audio/Source/Virtual",
                "echo-cancel-source",
                Some(NodeKind::Monitor),
            ),
            ("Audio/Sink", "alsa_output.pci", Some(NodeKind::Sink)),
            ("Stream/Output/Audio", "firefox", None),
            ("Video/Source", "camera", None),
        ];
        for (class, name, want) in table {
            assert_eq!(kind_of(class, name), want, "{class} {name}");
        }
    }
}
