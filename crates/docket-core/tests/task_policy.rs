//! Comparing and covering task policies: the table of what widens, narrows or stays, and the
//! rule that an untrusted argument is never inside.

mod support;

use docket_core::*;
use porter_core::Count;
use prov::{Effect, Integrity, TaskId};
use std::collections::{BTreeMap, BTreeSet};
use support::*;

fn mail_action(name: &str) -> ActionRef {
    ActionRef {
        app: app("org.quire.Mail"),
        name: action(name),
    }
}

fn policy() -> TaskPolicy {
    TaskPolicy {
        task: TaskId::parse("t-1").expect("task"),
        space: space("work"),
        from: vec![TurnId(1)],
        actions: BTreeSet::from([ActionMatch::AppUpTo(
            app("org.quire.Mail"),
            Effect::UndoableWrite,
        )]),
        kinds: BTreeSet::from([kind("mail.thread")]),
        ceiling: Effect::UndoableWrite,
        max_count: Count(10),
        recipients: vec![],
        destinations: vec![],
        paths: vec![],
        expires: at(3600),
        rationale: words("archive the newsletters"),
        state: TaskPolicyState::Active,
    }
}

fn edit(f: impl FnOnce(&mut TaskPolicy)) -> TaskPolicy {
    let mut p = policy();
    f(&mut p);
    p
}

#[test]
fn task_policy_compare_table() {
    let outbound = ActionMatch::AppUpTo(app("org.quire.Mail"), Effect::Outbound);
    let other_app = ActionMatch::AppUpTo(app("org.quire.Files"), Effect::UndoableWrite);
    let cases: Vec<(&str, TaskPolicy, TaskPolicy, PolicyChange)> = vec![
        ("identical", policy(), policy(), PolicyChange::Same),
        (
            "only the rationale and turns differ",
            edit(|p| {
                p.rationale = words("something else");
                p.from = vec![TurnId(2)];
            }),
            policy(),
            PolicyChange::Same,
        ),
        (
            "a new app widens",
            edit(|p| {
                p.actions.insert(other_app.clone());
            }),
            policy(),
            PolicyChange::Widens(vec![Widening::Action(other_app.clone())]),
        ),
        (
            "a higher cap on the same app widens, and so does the ceiling",
            edit(|p| {
                p.actions = BTreeSet::from([outbound.clone()]);
                p.ceiling = Effect::Outbound;
            }),
            policy(),
            PolicyChange::Widens(vec![
                Widening::Action(outbound.clone()),
                Widening::Ceiling(Effect::Outbound),
            ]),
        ),
        (
            "a higher cap bounded by an unchanged ceiling is the same",
            edit(|p| p.actions = BTreeSet::from([outbound.clone()])),
            policy(),
            PolicyChange::Same,
        ),
        (
            "one named action inside an app already covered narrows",
            edit(|p| {
                p.actions = BTreeSet::from([ActionMatch::One(mail_action("mail.thread.archive"))]);
            }),
            policy(),
            PolicyChange::Narrows,
        ),
        (
            "a new kind widens",
            edit(|p| {
                p.kinds.insert(kind("mail.contact"));
            }),
            policy(),
            PolicyChange::Widens(vec![Widening::Kind(kind("mail.contact"))]),
        ),
        (
            "fewer kinds narrows",
            edit(|p| p.kinds.clear()),
            policy(),
            PolicyChange::Narrows,
        ),
        (
            "a larger count widens",
            edit(|p| p.max_count = Count(11)),
            policy(),
            PolicyChange::Widens(vec![Widening::Count(Count(11))]),
        ),
        (
            "a smaller count narrows",
            edit(|p| p.max_count = Count(1)),
            policy(),
            PolicyChange::Narrows,
        ),
        (
            "a lower ceiling narrows",
            edit(|p| p.ceiling = Effect::Read),
            policy(),
            PolicyChange::Narrows,
        ),
        (
            "a later expiry widens",
            edit(|p| p.expires = at(7200)),
            policy(),
            PolicyChange::Widens(vec![Widening::Expiry]),
        ),
        (
            "an earlier expiry narrows",
            edit(|p| p.expires = at(60)),
            policy(),
            PolicyChange::Narrows,
        ),
        (
            "a new recipient widens, as a recipient",
            edit(|p| {
                p.recipients
                    .push(TrustedPattern::Domain("example.com".into()))
            }),
            policy(),
            PolicyChange::Widens(vec![Widening::Pattern(
                ArgSink::Recipient,
                TrustedPattern::Domain("example.com".into()),
            )]),
        ),
        (
            "a subdomain of a trusted domain narrows nothing and widens nothing",
            edit(|p| {
                p.recipients
                    .push(TrustedPattern::Domain("mail.example.com".into()))
            }),
            edit(|p| {
                p.recipients
                    .push(TrustedPattern::Domain("example.com".into()))
            }),
            PolicyChange::Narrows,
        ),
        (
            "a parent of a trusted domain widens",
            edit(|p| {
                p.recipients
                    .push(TrustedPattern::Domain("example.com".into()))
            }),
            edit(|p| {
                p.recipients
                    .push(TrustedPattern::Domain("mail.example.com".into()))
            }),
            PolicyChange::Widens(vec![Widening::Pattern(
                ArgSink::Recipient,
                TrustedPattern::Domain("example.com".into()),
            )]),
        ),
        (
            "a path under a trusted folder narrows",
            edit(|p| {
                p.paths
                    .push(TrustedPattern::Under(file("/home/me/Taxes/2025")));
            }),
            edit(|p| p.paths.push(TrustedPattern::Under(file("/home/me/Taxes")))),
            PolicyChange::Narrows,
        ),
        (
            "a sibling folder with the same prefix widens",
            edit(|p| {
                p.paths
                    .push(TrustedPattern::Under(file("/home/me/Taxes-old")));
            }),
            edit(|p| p.paths.push(TrustedPattern::Under(file("/home/me/Taxes")))),
            PolicyChange::Widens(vec![Widening::Pattern(
                ArgSink::Path,
                TrustedPattern::Under(file("/home/me/Taxes-old")),
            )]),
        ),
    ];
    for (name, new, old, want) in cases {
        assert_eq!(compare(&new, &old), want, "case: {name}");
    }
}

fn file(path: &str) -> FileRef {
    FileRef::parse(path).expect("file")
}

fn call(name: &str, targets: &[&str], args: Vec<(&str, Value)>) -> CallRequest {
    CallRequest {
        action: mail_action(name),
        target: TargetValue::Entities(targets.iter().map(|k| entity("mail.thread", k)).collect()),
        args: args
            .into_iter()
            .map(|(k, v)| (param(k), lab(v, trusted())))
            .collect(),
        origin: Origin::Companion,
    }
}

fn labels(per_arg: Vec<(&str, Integrity)>) -> ArgLabels {
    ArgLabels {
        per_arg: per_arg
            .into_iter()
            .map(|(k, i)| {
                let label = match i {
                    Integrity::Trusted => trusted(),
                    Integrity::Untrusted => untrusted_mail("work"),
                };
                (param(k), label)
            })
            .collect::<BTreeMap<_, _>>(),
        planner: Integrity::Trusted,
        saw: SessionSaw {
            private: Saw::NotSeen,
            untrusted: Saw::NotSeen,
        },
    }
}

#[test]
fn covers_table() {
    let eve = Value::Entity(entity("mail.contact", "eve@evil.test"));
    let bob = Value::Text("bob@example.com".into());
    let with_recipients = |p: TrustedPattern| edit(|x| x.recipients.push(p));
    let cases: Vec<(&str, TaskPolicy, CallRequest, ArgLabels, Coverage)> = vec![
        (
            "a named app and kind with trusted arguments",
            policy(),
            call("mail.thread.archive", &["t1", "t2"], vec![]),
            labels(vec![]),
            Coverage::Inside,
        ),
        (
            "an app the policy does not name",
            policy(),
            CallRequest {
                action: ActionRef {
                    app: app("org.quire.Files"),
                    name: action("files.file.move"),
                },
                ..call("mail.thread.archive", &[], vec![])
            },
            labels(vec![]),
            Coverage::Outside(Widening::Action(ActionMatch::One(ActionRef {
                app: app("org.quire.Files"),
                name: action("files.file.move"),
            }))),
        ),
        (
            "a kind the policy does not name",
            edit(|p| p.kinds.clear()),
            call("mail.thread.archive", &["t1"], vec![]),
            labels(vec![]),
            Coverage::Outside(Widening::Kind(kind("mail.thread"))),
        ),
        (
            "more things than the count allows",
            edit(|p| p.max_count = Count(1)),
            call("mail.thread.archive", &["t1", "t2"], vec![]),
            labels(vec![]),
            Coverage::Outside(Widening::Count(Count(2))),
        ),
        (
            "a policy that lapsed",
            edit(|p| p.state = TaskPolicyState::Expired),
            call("mail.thread.archive", &["t1"], vec![]),
            labels(vec![]),
            Coverage::Outside(Widening::Expiry),
        ),
        (
            "an untrusted recipient is outside",
            policy(),
            call("mail.message.send", &[], vec![("to", eve.clone())]),
            labels(vec![("to", Integrity::Untrusted)]),
            Coverage::Outside(Widening::Pattern(
                ArgSink::Recipient,
                TrustedPattern::Exact(eve.clone()),
            )),
        ),
        (
            "an untrusted recipient is outside even when it equals a trusted one as a string",
            with_recipients(TrustedPattern::Exact(Value::Text("eve@evil.test".into()))),
            call("mail.message.send", &[], vec![("to", eve.clone())]),
            labels(vec![("to", Integrity::Untrusted)]),
            Coverage::Outside(Widening::Pattern(
                ArgSink::Recipient,
                TrustedPattern::Exact(eve.clone()),
            )),
        ),
        (
            "an untrusted address inside a trusted domain is covered",
            with_recipients(TrustedPattern::Domain("example.com".into())),
            call("mail.message.send", &[], vec![("to", bob.clone())]),
            labels(vec![("to", Integrity::Untrusted)]),
            Coverage::Inside,
        ),
        (
            "a lookalike domain is not inside",
            with_recipients(TrustedPattern::Domain("example.com".into())),
            call(
                "mail.message.send",
                &[],
                vec![("to", Value::Text("bob@evil-example.com".into()))],
            ),
            labels(vec![("to", Integrity::Untrusted)]),
            Coverage::Outside(Widening::Pattern(
                ArgSink::Body,
                TrustedPattern::Exact(Value::Text("bob@evil-example.com".into())),
            )),
        ),
        (
            "an argument the router did not label is untrusted",
            policy(),
            call("mail.message.send", &[], vec![("to", eve.clone())]),
            labels(vec![]),
            Coverage::Outside(Widening::Pattern(
                ArgSink::Recipient,
                TrustedPattern::Exact(eve.clone()),
            )),
        ),
        (
            "a trusted argument needs no pattern",
            policy(),
            call("mail.message.send", &[], vec![("to", eve.clone())]),
            labels(vec![("to", Integrity::Trusted)]),
            Coverage::Inside,
        ),
        (
            "a trusted entity pattern covers an untrusted-labelled copy of the same entity",
            with_recipients(TrustedPattern::Entity(entity("mail.contact", "c1"))),
            call(
                "mail.message.send",
                &[],
                vec![("to", Value::Entity(entity("mail.contact", "c1")))],
            ),
            labels(vec![("to", Integrity::Untrusted)]),
            Coverage::Inside,
        ),
        (
            "a path that climbs out of a trusted folder is not under it",
            edit(|p| {
                p.paths.push(TrustedPattern::Under(file("/home/me/Taxes")));
            }),
            call(
                "mail.thread.archive",
                &[],
                vec![("into", Value::File(file("/home/me/Taxes/../.ssh")))],
            ),
            labels(vec![("into", Integrity::Untrusted)]),
            Coverage::Outside(Widening::Pattern(
                ArgSink::Path,
                TrustedPattern::Exact(Value::File(file("/home/me/Taxes/../.ssh"))),
            )),
        ),
    ];
    for (name, policy, call, labels, want) in cases {
        assert_eq!(covers(&policy, &call, &labels), want, "case: {name}");
    }
}
