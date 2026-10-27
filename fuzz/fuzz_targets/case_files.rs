//! Case files a person edits: damaged TOML is an error.
#![no_main]

use docket_eval::PlannerCase;
use eval_core::Case;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let _ = toml::from_str::<Case>(text);
    let _ = toml::from_str::<PlannerCase>(text);
});
