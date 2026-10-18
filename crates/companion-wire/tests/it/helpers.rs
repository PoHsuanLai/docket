//! The helpers on the wire: the list the shell shows, a card, and the words in them.

use companion_wire::*;
use docket_core::helper::*;
use docket_core::{ActionMatch, LabelText, TaskPolicy, TaskPolicyState};
use porter_core::Count;
use prov::{AgentRef, Effect, HelperId, SpaceId, TaskId};
use std::collections::BTreeSet;

fn hid(n: u64) -> HelperId {
    serde_json::from_str(&format!("\"h-{n}\"")).expect("helper id")
}

fn reach() -> TaskPolicy {
    TaskPolicy {
        task: TaskId::parse("t-0").expect("task"),
        space: SpaceId::parse("work").expect("space"),
        from: vec![],
        actions: BTreeSet::from([ActionMatch::AppUpTo(
            porter_core::AppName::parse("org.quire.Mail").expect("app"),
            Effect::Read,
        )]),
        kinds: BTreeSet::new(),
        ceiling: Effect::Read,
        max_count: Count(5),
        recipients: vec![],
        destinations: vec![],
        paths: vec![],
        expires: prov::UnixSeconds(i64::MAX),
        rationale: LabelText::parse("what the helper may use").expect("words"),
        state: TaskPolicyState::Active,
    }
}

fn draft(name: &str, model: HelperModel) -> CardDraft {
    CardDraft {
        name: HelperName::parse(name).expect("name"),
        role: Role::parse("Reads the mail.").expect("role"),
        kind: HelperKind::Native,
        space: SpaceId::parse("work").expect("space"),
        model,
        reach: reach(),
        budget: HelperBudget { steps: Count(8) },
    }
}

fn board_with(model: HelperModel) -> (Board, HelperId) {
    let (board, effects) = step(
        Board::new(),
        BoardEvent::Create(Box::new(draft("Research", model))),
    );
    let id = effects
        .iter()
        .find_map(|e| match e {
            BoardEffect::Created(id) => Some(id.clone()),
            _ => None,
        })
        .expect("created");
    (board, id)
}

fn job(to: &HelperId, text: &str) -> BoardEvent {
    BoardEvent::Post(Box::new(NewJob {
        to: to.clone(),
        goal: GoalText::parse(text).expect("goal"),
        origin: GoalOrigin::Person,
        giver: None,
        owns: vec![],
        with: vec![],
    }))
}

#[test]
fn the_list_says_in_plain_words_what_each_helper_is_doing_and_what_it_needs() {
    // No model and a job: it waits, and says what it needs.
    let (board, id) = board_with(HelperModel::Unset);
    let (board, _) = step(board, job(&id, "compare the three offers"));
    let card = board.card(&id).expect("card");
    let line = line_of(card, &board, None);
    assert_eq!(line.state, HelperPhase::Waiting);
    assert_eq!(line.needs_you, Some(HelperNeeds::ChooseModel));
    assert_eq!(line.doing, "Needs a model before it can start");
    assert!(line.agent.is_none());

    // Idle with nothing to do.
    let (board, id) = board_with(HelperModel::Unset);
    let idle = line_of(board.card(&id).expect("card"), &board, None);
    assert_eq!(
        (idle.state, idle.doing.as_str()),
        (HelperPhase::Idle, "Nothing to do")
    );

    // A job taken whose session is still opening: already working on it, nobody to speak to yet.
    let model: HelperModel =
        serde_json::from_str(r#"{"kind":"named","v":{"account":"local","model":"qwen3-4b"}}"#)
            .expect("model");
    let (board, id) = board_with(model);
    let (board, _) = step(board, job(&id, "compare the three offers"));
    let starting = line_of(board.card(&id).expect("card"), &board, None);
    assert_eq!(
        (starting.state, starting.doing.as_str(), starting.agent),
        (HelperPhase::Working, "compare the three offers", None)
    );

    // Paused, each way, with the reason.
    let (board, id) = board_with(HelperModel::Unset);
    for (why, text) in [
        (PauseWhy::ByPerson, "Paused"),
        (
            PauseWhy::Denied3,
            "Paused: three of its requests in a row were refused",
        ),
        (
            PauseWhy::Failed3,
            "Paused: three of its jobs in a row did not finish",
        ),
    ] {
        let mut card = board.card(&id).expect("card").clone();
        card.state = HelperState::Paused(why);
        let line = line_of(&card, &board, None);
        assert_eq!(
            (line.state, line.doing.as_str()),
            (HelperPhase::Paused, text)
        );
    }

    // Working: the goal is the line, the agent to speak to is named, and a question open on its
    // answer is what it needs.
    let (board, id) = board_with(HelperModel::Unset);
    let (board, effects) = step(board, job(&id, "compare the three offers"));
    let posted = effects
        .iter()
        .find_map(|e| match e {
            BoardEffect::Posted(job) => Some(*job),
            _ => None,
        })
        .expect("posted");
    let mut card = board.card(&id).expect("card").clone();
    let task = TaskId::parse("t-9").expect("task");
    card.state = HelperState::Working(TaskRef {
        job: posted,
        task: task.clone(),
    });
    let line = line_of(&card, &board, Some(HelperNeeds::Answer));
    assert_eq!(line.state, HelperPhase::Working);
    assert_eq!(line.doing, "compare the three offers");
    assert_eq!(line.needs_you, Some(HelperNeeds::Answer));
    assert_eq!(line.agent, Some(AgentRef::Worker { task }));
}

#[test]
fn a_card_shows_its_reach_in_words_and_its_jobs_with_why_one_did_not_finish() {
    let (board, id) = board_with(HelperModel::Unset);
    let (board, _) = step(board, job(&id, "compare the offers"));
    let view = card_of(board.card(&id).expect("card"), &board, None, |_| {
        JobWords::default()
    });
    assert_eq!(view.role, "Reads the mail.");
    assert_eq!(view.reach, ["org.quire.Mail: look at things"]);
    assert_eq!(view.steps, Count(8));
    assert_eq!(view.jobs.len(), 1);
    assert_eq!(view.jobs[0].state, JobShown::NeedsModel);
    assert_eq!(view.jobs[0].note, None);
    assert_eq!(view.jobs[0].answer, None);
    let posted = view.jobs[0].id;
    let kept = card_of(board.card(&id).expect("card"), &board, None, |job| {
        JobWords {
            answer: (job == posted).then(|| "The second offer is cheapest.".to_owned()),
            why: None,
        }
    });
    assert_eq!(
        kept.jobs[0].answer.as_deref(),
        Some("The second offer is cheapest.")
    );
    // A job that stopped says why in the sentence that was told, which names the model; with
    // none kept, in the fault's own words.
    let (board, _) = step(
        board,
        BoardEvent::Failed {
            job: posted,
            fault: JobFault::NoAnswer,
        },
    );
    let card = board.card(&id).expect("card");
    let told = card_of(card, &board, None, |_| JobWords {
        answer: None,
        why: Some("Qwen3 4B cannot answer: it cannot run right now.".to_owned()),
    });
    assert_eq!(
        told.jobs[0].note.as_deref(),
        Some("Qwen3 4B cannot answer: it cannot run right now.")
    );
    let bare = card_of(card, &board, None, |_| JobWords::default());
    assert_eq!(
        bare.jobs[0].note.as_deref(),
        Some("It stopped: the model gave no usable answer")
    );
}

fn line(program: &str, args: &[&str], env: &[(&str, &str)]) -> TerminalLine {
    TerminalLine {
        program: program.to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        env: env
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    }
}

#[test]
fn a_sign_in_terminal_carries_the_resolved_command_and_an_older_request_still_reads() {
    let request = TerminalRequest {
        helper: hid(2),
        program: prov::AgentProgram::parse("claude-code").expect("program"),
        login: TerminalLogin {
            args: vec!["/login".to_owned()],
            ..TerminalLogin::default()
        },
        run: Some(Box::new(TerminalRun {
            plain: line("/usr/bin/claude", &["/login"], &[("TERM", "xterm")]),
            folder: Some("/home/p/.local/share/docket/helpers/h-2".to_owned()),
            confined: Some(line("/usr/bin/systemd-run", &["--scope", "bwrap"], &[])),
        })),
        handoff: None,
    };
    let reply = HelperReply::OpenTerminal(request);
    let json = serde_json::to_string(&reply).expect("json");
    assert_eq!(
        json,
        concat!(
            r#"{"kind":"open_terminal","v":{"helper":"h-2","program":"claude-code","#,
            r#""login":{"args":["/login"]},"run":{"plain":{"program":"/usr/bin/claude","#,
            r#""args":["/login"],"env":{"TERM":"xterm"}},"#,
            r#""folder":"/home/p/.local/share/docket/helpers/h-2","#,
            r#""confined":{"program":"/usr/bin/systemd-run","args":["--scope","bwrap"]}}}}"#
        )
    );
    assert_eq!(
        serde_json::from_str::<HelperReply>(&json).expect("reply"),
        reply
    );
    // A request from before the command was resolved has no `run`, and reads as one.
    let older = r#"{"kind":"open_terminal","v":{"helper":"h-2","program":"claude-code","login":{"args":["/login"]}}}"#;
    let HelperReply::OpenTerminal(old) = serde_json::from_str(older).expect("older") else {
        panic!("a terminal request")
    };
    assert_eq!(old.run, None);
    assert_eq!(old.login.args, ["/login"]);
}

#[test]
fn a_conversation_in_a_terminal_pins_its_json_and_older_cards_read_as_in_docket() {
    let session = prov::SessionId::parse("s-3").expect("session");
    // The request to open a conversation in a terminal names the session that keeps its points.
    let request = TerminalRequest {
        helper: hid(2),
        program: prov::AgentProgram::parse("claude-code").expect("program"),
        login: TerminalLogin::default(),
        run: None,
        handoff: Some(session.clone()),
    };
    let json = serde_json::to_string(&HelperReply::OpenTerminal(request.clone())).expect("json");
    assert_eq!(
        json,
        r#"{"kind":"open_terminal","v":{"helper":"h-2","program":"claude-code","login":{},"handoff":"s-3"}}"#
    );
    assert_eq!(
        serde_json::from_str::<HelperReply>(&json).expect("reply"),
        HelperReply::OpenTerminal(request)
    );
    // A card from before the move reads as one that runs in docket with nothing to do.
    let (board, id) = board_with(HelperModel::Unset);
    let kept = board.card(&id).expect("card");
    let card = card_of(kept, &board, None, |_| JobWords::default());
    let mut value = serde_json::to_value(&card).expect("json");
    let object = value.as_object_mut().expect("object");
    object.remove("runs_in");
    object.remove("handover");
    let older: HelperCardView = serde_json::from_value(value).expect("older");
    assert_eq!(
        (older.runs_in, older.handover),
        (RunsIn::Desktop, Handover::Unavailable)
    );
    // The shell's other words for it.
    let listed = TerminalSession {
        session: session.clone(),
        program: prov::AgentProgram::parse("claude-code").expect("program"),
        app_name: Some("Claude Code".into()),
        folder: Some("/work/project".into()),
        handover: Handover::BringBack,
    };
    assert_eq!(
        serde_json::to_string(&listed).expect("json"),
        r#"{"session":"s-3","program":"claude-code","app_name":"Claude Code","folder":"/work/project","handover":"bring_back"}"#
    );
    assert_eq!(
        serde_json::to_string(&BroughtBack::Session(session)).expect("json"),
        r#"{"kind":"session","v":"s-3"}"#
    );
    assert_eq!(
        serde_json::to_string(&BroughtBack::Refused("No.".into())).expect("json"),
        r#"{"kind":"refused","v":"No."}"#
    );
}

#[test]
fn what_a_helper_says_in_its_conversation_pins_its_json() {
    for (value, json) in [
        (
            SayReply::Said("Offer B.".into()),
            r#"{"kind":"said","v":"Offer B."}"#,
        ),
        (
            SayReply::Refused("It needs a model.".into()),
            r#"{"kind":"refused","v":"It needs a model."}"#,
        ),
    ] {
        assert_eq!(serde_json::to_string(&value).expect("json"), json);
        assert_eq!(
            serde_json::from_str::<SayReply>(json).expect("reply"),
            value
        );
    }
}

#[test]
fn what_the_shell_reads_and_sends_pins_its_json() {
    let reply = [
        (HelperReply::Done, r#"{"kind":"done"}"#.to_owned()),
        (
            HelperReply::Refused("There is no helper by that name.".into()),
            r#"{"kind":"refused","v":"There is no helper by that name."}"#.to_owned(),
        ),
        (
            HelperReply::Posted(JobId(4)),
            r#"{"kind":"posted","v":4}"#.to_owned(),
        ),
        (
            HelperReply::Made(hid(2)),
            r#"{"kind":"made","v":"h-2"}"#.to_owned(),
        ),
    ];
    for (value, json) in reply {
        assert_eq!(serde_json::to_string(&value).expect("json"), json);
        let back: HelperReply = serde_json::from_str(&json).expect("reply");
        assert_eq!(back, value);
    }
    for (shown, slug) in [
        (HelperPhase::Idle, "idle"),
        (HelperPhase::Working, "working"),
        (HelperPhase::Waiting, "waiting"),
        (HelperPhase::Paused, "paused"),
    ] {
        assert_eq!(
            serde_json::to_string(&shown).expect("json"),
            format!("\"{slug}\"")
        );
    }
    assert_eq!(
        serde_json::to_string(&HelperNeeds::ChooseModel).expect("json"),
        "\"choose_model\""
    );
    let form = HelperForm {
        name: HelperName::parse("Research").expect("name"),
        role: Role::parse("Reads.").expect("role"),
        space: SpaceId::parse("work").expect("space"),
        model: HelperModel::Unset,
        apps: vec![porter_core::AppName::parse("org.quire.Mail").expect("app")],
        ceiling: Ceiling::Change,
        steps: Count(8),
        kind: Default::default(),
    };
    let text = serde_json::to_string(&form).expect("json");
    assert!(text.contains(r#""ceiling":"change""#), "{text}");
    let back: HelperForm = serde_json::from_str(&text).expect("form");
    assert_eq!(back, form);
}

#[test]
fn a_line_without_a_conversation_reads_from_an_older_list_and_is_not_written_with_one() {
    let (board, id) = board_with(HelperModel::Unset);
    let mut line = line_of(board.card(&id).expect("card"), &board, None);
    assert_eq!(line.conversation, None);
    let text = serde_json::to_string(&line).expect("json");
    assert!(!text.contains("conversation"), "{text}");
    let back: HelperLine = serde_json::from_str(&text).expect("an older list reads");
    assert_eq!(back, line);

    line.conversation = Some(prov::SessionId::parse("s-61").expect("session"));
    let text = serde_json::to_string(&line).expect("json");
    assert!(text.contains("\"conversation\":\"s-61\""), "{text}");
}

#[test]
fn a_helper_that_runs_a_known_app_carries_the_name_people_call_it() {
    let app = |program: &str| {
        let mut draft = draft("Coder", HelperModel::Unset);
        draft.kind = HelperKind::AgentApp(prov::AgentProgram::parse(program).expect("program"));
        let (board, effects) = step(Board::new(), BoardEvent::Create(Box::new(draft)));
        let id = effects
            .iter()
            .find_map(|e| match e {
                BoardEffect::Created(id) => Some(id.clone()),
                _ => None,
            })
            .expect("created");
        card_of(board.card(&id).expect("card"), &board, None, |_| {
            JobWords::default()
        })
    };
    assert_eq!(app("claude-code").app_name.as_deref(), Some("Claude Code"));
    assert_eq!(app("gemini-cli").app_name.as_deref(), Some("Gemini CLI"));
    // An app we do not know the name of has none, and the field is left out of the JSON.
    let unknown = app("mine");
    assert_eq!(unknown.app_name, None);
    assert!(
        !serde_json::to_string(&unknown)
            .expect("json")
            .contains("app_name")
    );
    let (board, id) = board_with(HelperModel::Unset);
    let native = card_of(board.card(&id).expect("card"), &board, None, |_| {
        JobWords::default()
    });
    assert_eq!(native.app_name, None);
}
