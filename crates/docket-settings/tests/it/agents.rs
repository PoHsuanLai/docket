//! The agent rows: what was offered against what is chosen, the plain words, and the in-place
//! edit of `agents.toml`. A scratch agents directory; no agent starts and no clock is read.

use docket_agents::AgentsDir;
use docket_agents::offered::{Named, Offered, Start};
use docket_settings::*;

const FILE: &str = r#"# my agents
[[agent]]
program = "antigravity"
registry = "antigravity-acp"
version = "1.3.0"
label = "Antigravity"
model = "pro"   # keep me
sign_in = "google"

[[agent]]
program = "mine"
command = "/opt/mine"
route = "login"
"#;

fn named(id: &str, name: &str) -> Named {
    Named {
        id: id.to_owned(),
        name: name.to_owned(),
    }
}

fn installed(dir: &AgentsDir, id: &str, version: &str) {
    let at = dir.installed(
        &docket_agents::slug::Slug::parse(id).expect("id"),
        &docket_agents::slug::Slug::parse(version).expect("version"),
    );
    std::fs::create_dir_all(&at).expect("dir");
    std::fs::write(
        at.join("installed.toml"),
        format!(
            "id = \"{id}\"\nversion = \"{version}\"\ncommand = \"agy\"\ncheck = \"first_use\"\n"
        ),
    )
    .expect("record");
}

fn record(start: Start, models: Vec<Named>, ways: Vec<Named>) -> Offered {
    Offered {
        seen: 0,
        start,
        in_use: models.first().map(|m| m.id.clone()),
        models,
        ways,
    }
}

#[test]
fn an_agent_never_started_has_nothing_offered_and_says_so() {
    let tmp = tempfile::tempdir().expect("tmp");
    let agents = read_agents(FILE, &AgentsDir::at(tmp.path()));
    assert_eq!(agents.rows.len(), 2);
    let row = agents.get("antigravity").expect("row");
    assert_eq!(row.label, "Antigravity");
    assert_eq!(row.offers, Offers::NotSeenYet);
    assert_eq!(row.sign_in, SignInState::NotSeenYet);
    assert_eq!(row.sign_in_line(), "Not started yet");
    assert_eq!(row.offers_line(), "Not started yet");
    assert_eq!(row.install_line(), "Not installed yet (version 1.3.0)");
    assert_eq!(row.model_line(), "Model: pro (not checked yet)");
    assert_eq!(agents.get("mine").expect("own").label, "mine");
}

#[test]
fn the_record_decides_the_models_and_whether_the_choice_is_still_offered() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = AgentsDir::at(tmp.path());
    installed(&dir, "antigravity-acp", "1.3.0");
    record(
        Start::SignedIn,
        vec![named("pro", "Gemini Pro")],
        Vec::new(),
    )
    .write(&dir, "antigravity-acp")
    .expect("write");
    let row = read_agents(FILE, &dir).rows.remove(0);
    assert_eq!(row.install_line(), "Installed");
    assert_eq!(row.sign_in_line(), "Signed in");
    assert_eq!(row.model_line(), "Model: Gemini Pro");
    assert_eq!(row.offers_line(), "1 model to choose from");

    record(
        Start::SignedIn,
        vec![named("flash", "Gemini Flash")],
        Vec::new(),
    )
    .write(&dir, "antigravity-acp")
    .expect("write");
    let row = read_agents(FILE, &dir).rows.remove(0);
    assert_eq!(row.model_line(), "Model: pro (no longer offered)");
}

#[test]
fn needing_sign_in_lists_the_ways_and_keeps_the_models_seen_before() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = AgentsDir::at(tmp.path());
    record(
        Start::SignedIn,
        vec![named("pro", "Gemini Pro")],
        Vec::new(),
    )
    .write(&dir, "antigravity-acp")
    .expect("write");
    let after = record(
        Start::NeedsSignIn,
        Vec::new(),
        vec![named("google", "Google account")],
    )
    .write(&dir, "antigravity-acp")
    .expect("write");
    assert_eq!(after.seen, 2, "a counter, not a clock");
    assert_eq!(after.models.len(), 1);
    let row = read_agents(FILE, &dir).rows.remove(0);
    assert_eq!(row.sign_in_line(), "Needs signing in");
    assert_eq!(
        row.ways_line().as_deref(),
        Some("Ways to sign in: Google account")
    );
}

#[test]
fn the_choice_is_edited_in_place_and_nothing_else_moves() {
    let choice = AgentChoice {
        model: Pick::Set("flash".to_owned()),
        way: Pick::Clear,
    };
    let out = edit_agent_choice(FILE, "antigravity", &choice).expect("edit");
    assert!(out.contains("# my agents"));
    assert!(out.contains("model = \"flash\""));
    assert!(!out.contains("sign_in"));
    assert!(out.contains("label = \"Antigravity\""));
    assert!(out.contains("command = \"/opt/mine\""));
}

#[test]
fn a_bad_choice_or_an_unlisted_agent_writes_nothing() {
    let bad = |model: &str| AgentChoice {
        model: Pick::Set(model.to_owned()),
        way: Pick::Keep,
    };
    assert_eq!(
        edit_agent_choice(FILE, "antigravity", &bad("two words")),
        Err(ChoiceFault::BadModel)
    );
    assert_eq!(
        edit_agent_choice(FILE, "nobody", &bad("ok")),
        Err(ChoiceFault::NoSuchAgent)
    );
    assert_eq!(
        edit_agent_choice("[[", "a", &bad("ok")),
        Err(ChoiceFault::Unreadable)
    );
    let way = AgentChoice {
        model: Pick::Keep,
        way: Pick::Set("a b".to_owned()),
    };
    assert_eq!(
        edit_agent_choice(FILE, "antigravity", &way),
        Err(ChoiceFault::BadWay)
    );
}

#[test]
fn the_file_is_replaced_whole_and_a_refresh_names_only_the_agent() {
    let tmp = tempfile::tempdir().expect("tmp");
    let path = tmp.path().join("agents.toml");
    std::fs::write(&path, FILE).expect("file");
    let choice = AgentChoice {
        model: Pick::Clear,
        way: Pick::Keep,
    };
    write_agent_choice(&path, "antigravity", &choice).expect("write");
    let rows = read_agents(
        &std::fs::read_to_string(&path).expect("read"),
        &AgentsDir::at(tmp.path()),
    );
    assert_eq!(
        rows.get("antigravity").expect("row").model,
        ModelState::AgentsOwn
    );
    assert_eq!(
        rows.get("antigravity").expect("row").way.as_deref(),
        Some("google")
    );
    let ask = RefreshRequest::of(rows.get("mine").expect("row"));
    assert_eq!(ask.arguments(), ["mine", "--refresh"]);
}

#[test]
fn who_keeps_the_history_is_read_with_the_profile_as_default_and_edited_in_place() {
    use docket_core::Rewind;
    let text = format!(
        "{FILE}\n[[agent]]\nprogram = \"claude-code\"\ncommand = \"/opt/c\"\nroute = \"login\"\nprofile = \"claude-code\"\n"
    );
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = AgentsDir::at(tmp.path());
    let read = |text: &str, program: &str| {
        read_agents(text, &dir)
            .get(program)
            .map(|row| (row.rewind, row.rewind_line()))
    };
    let kept_by_agent = (Rewind::Agent, "Restore points: kept by the agent");
    let kept_by_docket = (Rewind::Docket, "Restore points: kept by docket");
    assert_eq!(read(&text, "antigravity"), Some(kept_by_docket));
    assert_eq!(read(&text, "mine"), Some(kept_by_docket));
    assert_eq!(read(&text, "claude-code"), Some(kept_by_agent));

    // A choice is written out in full and moves nothing else; it beats the profile's default.
    let out = edit_agent_rewind(&text, "mine", Rewind::Agent).expect("edit");
    assert!(out.contains("# my agents") && out.contains("model = \"pro\"   # keep me"));
    assert!(out.contains("checkpoints = \"agent\""));
    assert_eq!(read(&out, "mine"), Some(kept_by_agent));
    let out = edit_agent_rewind(&text, "claude-code", Rewind::Docket).expect("edit");
    assert_eq!(read(&out, "claude-code"), Some(kept_by_docket));

    assert_eq!(
        edit_agent_rewind(&text, "nobody", Rewind::Agent),
        Err(ChoiceFault::NoSuchAgent)
    );
    assert_eq!(
        edit_agent_rewind("[[", "a", Rewind::Agent),
        Err(ChoiceFault::Unreadable)
    );

    let path = tmp.path().join("agents.toml");
    std::fs::write(&path, &text).expect("file");
    write_agent_rewind(&path, "mine", Rewind::Agent).expect("write");
    let written = std::fs::read_to_string(&path).expect("read");
    assert_eq!(read(&written, "mine"), Some(kept_by_agent));
}
