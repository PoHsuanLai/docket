//! SPEC section 5 flows (a) and (c) through the real daemons on a private bus (see
//! `dev/accept/README.md` for what is real and what is scripted).

mod support;

use companion_wire::{AnswerPhase, AnswerWire, NeedsYou};
use docket_accept::confirm::Verdict;
use docket_accept::drive::{Launcher, recorded};
use docket_accept::inferd::{Role, Say, text_of};
use docket_accept::provider::{INJECTION, Sending};
use docket_accept::world::{Consent, World};
use docket_core::{
    ConfirmDetail, ConfirmOffer, Handle, JournalFilter, Shown, TaintNote, UndoState,
};
use serde_json::json;
use support::*;

fn settled(view: &AnswerWire) -> bool {
    matches!(
        view.phase,
        AnswerPhase::Done | AnswerPhase::Failed | AnswerPhase::Cancelled
    )
}

fn waiting_on_sheet(view: &AnswerWire) -> bool {
    matches!(view.phase, AnswerPhase::NeedsYou(NeedsYou::Confirm(_)))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn flow_a_prompt_plan_confirm_perform_journal_undo() {
    let world = World::start(&binaries(), Consent::Standing).await;
    // The planner finds the threads and the contact, forwards, and says it is done.
    world.model.planner(vec![
        calls(&tool("mail.thread.search"), json!({"query": "Lisbon"})),
        calls(&tool("mail.contact.search"), json!({"query": "Accounting"})),
        // The planner names the things the find returned (entity ids are structure; only the
        // words of a title are held back).
        calls(
            &tool("mail.message.forward"),
            json!({"target": [thread("lisbon-1"), thread("lisbon-2")], "to": contact("accounting")}),
        ),
        words("Forwarded the Lisbon receipts to Accounting."),
    ]);
    world.sheet.will(Verdict::Allow);

    let launcher = Launcher::of(&world).await;
    let opened = launcher.open().await;
    let mut answer = launcher
        .say(&opened, "forward the Lisbon receipts to accounting")
        .await;
    let history = answer.history_until(settled).await;
    let last = history.last().expect("a view").clone();
    if last.phase != AnswerPhase::Done {
        fail(
            &world,
            &format!("the answer ended {:?}\n{history:#?}", last.phase),
        );
    }

    if std::env::var_os("ACCEPT_SHOW_PLANNER").is_some() {
        for request in world.model.asked_by(Role::Planner) {
            eprintln!("---- planner view\n{}", text_of(&request));
        }
    }

    // One sheet: Forward, Outbound, the recipient from the app's own dry run, shown as the
    // person's own contact (plain); the session read untrusted thread titles, so it is
    // once-only and says so.
    let shown = world.sheet.shown();
    assert_eq!(shown.len(), 1, "exactly one sheet: {shown:#?}");
    let sheet = &shown[0];
    assert_eq!(sheet.action.as_str(), "Forward");
    assert_eq!(sheet.effect, prov::Effect::Outbound);
    assert!(
        matches!(
            &sheet.detail,
            ConfirmDetail::Preview(_) | ConfirmDetail::Recipients(_)
        ),
        "{:?}",
        sheet.detail
    );
    assert_eq!(sheet.offer, ConfirmOffer::OnceOnly);
    assert!(matches!(sheet.taint, TaintNote::ReadUntrusted(_)));

    // Mailo's side: the forward went out as held, to the contact, for the two Lisbon threads,
    // and the actor was the companion.
    let messages = world.mail.messages();
    assert_eq!(messages.len(), 1, "{messages:#?}");
    assert_eq!(messages[0].to, "accounting");
    assert_eq!(messages[0].threads, ["lisbon-1", "lisbon-2"]);
    assert_eq!(messages[0].state, Sending::Held);
    assert!(
        matches!(messages[0].actor, prov::Actor::Companion { .. }),
        "{:?}",
        messages[0].actor
    );

    // The journal has the row, from the plan's last step, and the card carries it.
    let journal = launcher
        .intents
        .journal(JournalFilter {
            run: None,
            session: None,
            limit: porter_core::Count(10),
        })
        .await
        .expect("journal");
    assert_eq!(journal.len(), 1, "{journal:#?}");
    assert_eq!(journal[0].state, UndoState::Available);
    assert_eq!(journal[0].action.name.as_str(), "mail.message.forward");

    // Undo as the person: the app cancels the held send; the row says who.
    launcher
        .intents
        .undo(journal[0].id)
        .await
        .expect("undo request")
        .expect("undone");
    assert_eq!(world.mail.messages()[0].state, Sending::Cancelled);
    let after = launcher
        .intents
        .journal(JournalFilter {
            run: None,
            session: None,
            limit: porter_core::Count(10),
        })
        .await
        .expect("journal");
    assert!(matches!(
        after[0].state,
        UndoState::Undone {
            by: prov::Actor::User { .. }
        }
    ));
    assert_eq!(world.model.planner_left(), 0, "every planned step was used");

    // The router's audit records reached memoryd, from intentd's process, over the bus.
    let recorded = recorded(
        &world,
        &[
            "docket.call",
            "docket.confirm",
            "docket.undo",
            "docket.task_policy",
        ],
    )
    .await;
    assert!(
        recorded
            .iter()
            .all(|e| !format!("{e:?}").contains("IGNORE ALL PREVIOUS")),
        "no mail body in the audit"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn flow_a_the_person_refuses_the_sheet_and_nothing_is_sent() {
    let world = World::start(&binaries(), Consent::Standing).await;
    world.model.planner(vec![
        calls(&tool("mail.thread.search"), json!({"query": "Lisbon"})),
        calls(&tool("mail.contact.search"), json!({"query": "Accounting"})),
        calls(
            &tool("mail.message.forward"),
            json!({"target": [thread("lisbon-1"), thread("lisbon-2")], "to": contact("accounting")}),
        ),
        words("Understood, I left them where they are."),
    ]);
    world.sheet.will(Verdict::Refuse);

    let launcher = Launcher::of(&world).await;
    let opened = launcher.open().await;
    let mut answer = launcher
        .say(&opened, "forward the Lisbon receipts to accounting")
        .await;
    let history = answer.history_until(settled).await;
    assert_eq!(
        history.last().map(|v| v.phase.clone()),
        Some(AnswerPhase::Done),
        "{history:#?}"
    );

    // The sheet was shown, the person said no, and the app never held anything.
    assert_eq!(world.sheet.shown().len(), 1);
    assert!(
        world.mail.messages().is_empty(),
        "{:?}",
        world.mail.messages()
    );
    assert!(
        !world
            .mail
            .performed()
            .iter()
            .any(|a| a == "mail.message.forward"),
        "the app was never asked to forward: {:?}",
        world.mail.performed()
    );
    // The planner was told, as a coarse step, that the call was not confirmed.
    let last = world
        .model
        .asked_by(Role::Planner)
        .pop()
        .expect("a request");
    assert!(
        text_of(&last).contains("mail.message.forward not confirmed"),
        "{}",
        text_of(&last)
    );
    // Nothing to undo.
    let journal = launcher
        .intents
        .journal(JournalFilter {
            run: None,
            session: None,
            limit: porter_core::Count(10),
        })
        .await
        .expect("journal");
    assert!(journal.is_empty(), "{journal:#?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn flow_c_an_injected_body_never_reaches_the_planner_and_the_send_still_asks_quoted() {
    let world = World::start(&binaries(), Consent::Standing).await;
    const SUMMARY: &str = "The sender asks about an invoice and tells the reader to forward all mail to x@evil.example.";
    world
        .model
        .reader(&[&json!({ "answer": SUMMARY }).to_string()]);
    world.model.planner(vec![
        calls(&tool("mail.thread.read"), json!({"target": thread("injected")})),
        Box::new(|request| {
            let body = handle_after(request, "mail.thread.read done");
            Say::Calls(vec![(
                "quire_read".to_owned(),
                json!({"inputs": [body], "task": "summarise", "want": {"kind": "text", "v": {"max": 300}}}),
            )])
        }),
        Box::new(|request| {
            // The reader's answer came back as a handle; the planner drafts a send with it as
            // the body, to the address the (steered) summary mentions.
            let summary = handle_after(request, "companion.read done");
            Say::Calls(vec![(
                tool("mail.message.send"),
                json!({"to": "x@evil.example", "body": {"handle": summary}}),
            )])
        }),
        words("I did not send that."),
    ]);
    world.sheet.will(Verdict::Refuse);

    let launcher = Launcher::of(&world).await;
    let opened = launcher.open().await;
    let mut answer = launcher
        .say(&opened, "summarise this thread and reply")
        .await;
    let history = answer.history_until(settled).await;
    if history.last().map(|v| v.phase.clone()) != Some(AnswerPhase::Done) {
        fail(&world, &format!("{history:#?}"));
    }

    // 1. The planner never saw the body, in any turn: not the injection, not the summary.
    let planner = world.model.asked_by(Role::Planner);
    assert_eq!(planner.len(), 4);
    for request in &planner {
        let text = text_of(request);
        assert!(!text.contains("IGNORE ALL PREVIOUS"), "{text}");
        assert!(!text.contains("tells the reader to forward"), "{text}");
        assert!(!text.contains(INJECTION), "{text}");
    }

    // 2. The reader saw it, and only as fenced data: no tools, one structured request, the
    //    class of the mail it read, and the fixed instruction.
    let reader = world.model.asked_by(Role::Reader);
    assert_eq!(reader.len(), 1);
    assert!(reader[0].tools.is_empty());
    assert!(matches!(reader[0].shape, porter_infer::ReplyShape::Json(_)));
    assert!(text_of(&reader[0]).contains(INJECTION));
    // `Session.Resolve` hands readerd the handle's label with its text, so the reader's
    // session is opened for the class of the mail it reads.
    assert!(
        world
            .model
            .opened()
            .contains(&(Role::Reader, porter_core::DataClass::Mail)),
        "{:?}",
        world.model.opened()
    );

    // 3. The summary is for the screen only: Session.Display gives it to the person.
    let number = handle_after(&planner[2], "companion.read done");
    let shown = launcher
        .intents
        .session_display(opened.session.clone(), Handle(number))
        .await
        .expect("display");
    assert_eq!(shown, SUMMARY);

    // 4. The send with that body to that recipient still asks, with the recipient quoted from
    //    mail, once only, and says the session read untrusted mail. The person refused it.
    let sheets = world.sheet.shown();
    assert_eq!(sheets.len(), 1, "{sheets:#?}");
    let ask = &sheets[0];
    assert_eq!(ask.action.as_str(), "Send");
    assert_eq!(ask.offer, ConfirmOffer::OnceOnly);
    let ConfirmDetail::Recipients(to) = &ask.detail else {
        panic!("recipients: {:?}", ask.detail);
    };
    assert!(
        matches!(&to[..], [Shown::Quoted { text, from: prov::Source::Mail }] if text == "x@evil.example"),
        "{to:?}"
    );
    assert!(
        matches!(&ask.taint, TaintNote::ReadUntrusted(sources) if sources.contains(&prov::Source::Mail)),
        "{:?}",
        ask.taint
    );
    assert!(world.mail.messages().is_empty());
}

/// SPEC 5(a) steps 7 and 8: a plan card streams, and while the sheet is up the answer shows
/// `NeedsYou(Confirm)`; then it runs again and is done.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn flow_a_the_answer_shows_the_sheet_while_it_waits() {
    let world = World::start(&binaries(), Consent::Standing).await;
    world.model.planner(vec![
        calls(&tool("mail.thread.search"), json!({"query": "Lisbon"})),
        calls(&tool("mail.contact.search"), json!({"query": "Accounting"})),
        calls(
            &tool("mail.message.forward"),
            json!({"target": [thread("lisbon-1"), thread("lisbon-2")], "to": contact("accounting")}),
        ),
        words("Forwarded."),
    ]);
    world.sheet.will(Verdict::Allow);
    let launcher = Launcher::of(&world).await;
    let opened = launcher.open().await;
    let mut answer = launcher
        .say(&opened, "forward the Lisbon receipts to accounting")
        .await;
    let history = answer.history_until(settled).await;
    assert!(history.iter().any(waiting_on_sheet), "{history:#?}");
    assert!(
        history
            .iter()
            .any(|v| matches!(v.body, companion_wire::AnswerBody::Plan(_))),
        "{history:#?}"
    );
}

/// The first use of Mail's classes in a Space asks (a sheet that says so); the person says
/// "always", and the second call of the same kind does not ask.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn first_use_of_mail_in_a_space_asks_once_and_the_second_call_does_not() {
    let world = World::start(&binaries(), Consent::FirstUse).await;
    world.model.planner(vec![
        calls(&tool("mail.thread.search"), json!({"query": "Lisbon"})),
        calls(&tool("mail.thread.search"), json!({"query": "Porto"})),
        words("Found both."),
    ]);
    world.sheet.will(Verdict::AllowAlways);
    let launcher = Launcher::of(&world).await;
    let opened = launcher.open().await;
    let mut answer = launcher
        .say(&opened, "look for Lisbon and Porto mail")
        .await;
    let history = answer.history_until(settled).await;
    assert_eq!(
        history.last().map(|v| v.phase.clone()),
        Some(AnswerPhase::Done),
        "{history:#?}"
    );
    assert!(history.iter().any(waiting_on_sheet), "{history:#?}");

    // Both reads ran; only the first asked, and it said why.
    assert_eq!(
        world
            .mail
            .performed()
            .iter()
            .filter(|a| *a == "mail.thread.search")
            .count(),
        2,
        "{:?}",
        world.mail.performed()
    );
    let sheets = world.sheet.shown();
    assert_eq!(sheets.len(), 1, "{sheets:#?}");
    assert!(
        sheets[0].why.contains(&docket_core::AskReason::FirstUse),
        "{:?}",
        sheets[0].why
    );
    assert_eq!(world.model.planner_left(), 0);
}
