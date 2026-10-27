//! A trusted domain matches the host a URL names as the WHATWG standard reads it, never a host
//! that only looks like it in the raw text (user information, backslashes, lookalike suffixes).

use crate::task_policy::{call, declared, edit, labels, policy};
use docket_core::*;
use prov::{Effect, Integrity};

fn trusts(domain: &str, url: &str) -> bool {
    let policy = edit(|p| p.recipients.push(TrustedPattern::Domain(domain.into())));
    let request = call(
        "mail.message.send",
        &[],
        vec![("to", Value::Url(url.into()))],
    );
    let decl = declared(&request, Effect::UndoableWrite);
    let untrusted = labels(vec![("to", Integrity::Untrusted)]);
    covers(&policy, &decl, &request, &untrusted) == Coverage::Inside
}

#[test]
fn a_url_is_inside_a_domain_only_when_its_real_host_is() {
    let rows = [
        ("https://good.example/", true),
        ("https://GOOD.Example/path", true),
        ("https://www.good.example:8443/x?y#z", true),
        ("https://user:pw@good.example/", true),
        ("https://good.example\\@evil.test/", true),
        ("https://good%2eexample/", true),
        ("https://evil.test\\@good.example/", false),
        ("https://evil.test\\.good.example/", false),
        ("https://good.example@evil.test/", false),
        ("https://good.example:pw@evil.test/", false),
        ("https://good.example.evil.test/", false),
        ("https://evil-good.example/", false),
        ("https://good.example%2eevil.test/", false),
        ("https://evil.test%2f.good.example/", false),
        ("https://good.example%40evil.test/", false),
        ("https://good.example./", false),
        ("https://[::1]/", false),
        ("https://[::ffff:1.2.3.4]:80/", false),
        ("https://1.2.3.4/", false),
        ("good.example/path", false),
        ("//good.example/", false),
        ("good.example:443/x", false),
        ("mailto:bob@good.example", false),
    ];
    for (url, want) in rows {
        assert_eq!(trusts("good.example", url), want, "{url}");
    }
}

#[test]
fn an_untrusting_policy_covers_no_url() {
    let request = call(
        "mail.message.send",
        &[],
        vec![("to", Value::Url("https://good.example/".into()))],
    );
    let decl = declared(&request, Effect::UndoableWrite);
    let untrusted = labels(vec![("to", Integrity::Untrusted)]);
    assert_ne!(
        covers(&policy(), &decl, &request, &untrusted),
        Coverage::Inside
    );
}
