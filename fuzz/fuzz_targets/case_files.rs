//! Case files a person edits: damaged TOML is an error.
#![no_main]

use docket_eval::{Case, PlannerCase};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let _ = toml::from_str::<Case>(text);
    let _ = toml::from_str::<PlannerCase>(text);
});
