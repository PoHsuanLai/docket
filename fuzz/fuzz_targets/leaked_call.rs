//! A model's words: telling a call written as words from words about calls never panics.
#![no_main]

use agent_loop::leaked_call;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let _ = leaked_call(text);
});
