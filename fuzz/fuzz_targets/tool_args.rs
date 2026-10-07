//! A tool call's arguments, as JSON bytes, read by the types of a real action.
#![no_main]

use docket_core::{Value, args_from_json, plain_text};
use docket_fake::mail_manifest;
use libfuzzer_sys::fuzz_target;
use prov::Label;

fuzz_target!(|bytes: &[u8]| {
    let Ok(serde_json::Value::Object(given)) = serde_json::from_slice::<serde_json::Value>(bytes)
    else {
        return;
    };
    let Ok(manifest) = mail_manifest() else {
        return;
    };
    for decl in &manifest.manifest().actions {
        let Ok((_target, args)) = args_from_json(decl, Some(&given), &Label::trusted_user()) else {
            continue;
        };
        for (name, held) in &args {
            assert!(
                decl.params.iter().any(|p| &p.name == name),
                "undeclared {name}"
            );
            if let Value::Text(text) = &held.value {
                assert!(plain_text(text, &['\n', '\r', '\t']), "{text:?}");
            }
        }
    }
});
