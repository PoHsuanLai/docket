//! Skills in the planner's view: the catalogue, the bodies the context preselects, a load by the
//! planner, the order of the action list, the cap of three, and the audit. A skill teaches and
//! grants nothing: every call a skill mentions still goes through the router.

mod support;

use docket_core::{AuditRecord, CallEnd};
use docket_skills::{Origin, Skill, parse};
use serde_json::json;
use support::infer::{call, words};
use support::world::*;

const LOAD: &str = "org.quire.Companion-companion.skill.load";
const ARCHIVE: &str = "org.quire.Mail-mail.thread.archive";

fn skill(id: &str, uses: &str, when: &str, body: &str) -> Skill {
    let toml = format!(
        "vocab = 1\nid = \"{id}\"\nowner = \"org.quire.Companion\"\nversion = \"1.{}\"\n\
         uses = [\"{uses}\"]\n{when}",
        id.len()
    );
    let md = format!("---\nname: {id}\ndescription: about {id}\n---\n{body}\n");
    parse(id, &toml, &md, Origin::Shipped).expect("a valid skill")
}

const LOAD_USE: &str = "org.quire.Companion:companion.skill.load";
const ARCHIVE_USE: &str = "org.quire.Mail:mail.thread.archive";

fn installed() -> Vec<Skill> {
    vec![
        skill("basics", LOAD_USE, "[when]\nalways = \"yes\"\n", "BASICS-BODY"),
        skill(
            "shell-tips",
            LOAD_USE,
            "[when]\nfocused_app = [\"org.quire.Shell\"]\n",
            "SHELL-BODY",
        ),
        skill(
            "mail-tips",
            ARCHIVE_USE,
            "[when]\nfocused_app = [\"org.quire.Mail\"]\n",
            "MAIL-BODY",
        ),
        skill("extra-one", LOAD_USE, "", "ONE-BODY"),
        skill("extra-two", LOAD_USE, "", "TWO-BODY"),
        // Its action is not registered anywhere: hidden, in no catalogue, not loadable.
        skill("ghost", "org.quire.Mail:mail.no.such", "[when]\nalways = \"yes\"\n", "GHOST-BODY"),
    ]
}

fn tool_names(request: &porter_infer::ChatRequest) -> Vec<String> {
    request
        .tools
        .iter()
        .map(|t| t.name.as_str().to_owned())
        .collect()
}

#[tokio::test]
async fn the_catalogue_lists_offered_skills_and_the_contexts_match_is_expanded() {
    let mut w = world_with_skills(vec![words("Fine.")], installed());
    let front = w.open("work").await;
    w.say(&front.session, "hello").await;

    let system = w.infer.system_text(0);
    for id in ["basics", "shell-tips", "mail-tips", "extra-one", "extra-two"] {
        assert!(system.contains(&format!("- {id}: about {id}")), "{id}: {system}");
    }
    assert!(!system.contains("ghost"), "a skill with a missing action is hidden");

    // Summoned from the launcher the focused app is the shell: `always` and the shell skill are
    // expanded (at most two); the mail skill is only in the catalogue.
    let user = w.infer.user_text(0);
    assert!(user.contains("BASICS-BODY") && user.contains("SHELL-BODY"), "{user}");
    for absent in ["MAIL-BODY", "ONE-BODY", "GHOST-BODY"] {
        assert!(!user.contains(absent), "{absent} was not preselected");
    }
    assert!(user.contains("installed text from org.quire.Companion"));
}

#[tokio::test]
async fn a_load_puts_the_skill_first_keeps_its_body_and_is_audited() {
    let mut w = world_with_skills(
        vec![
            call(LOAD, json!({ "id": "mail-tips" })),
            words("Loaded it."),
        ],
        installed(),
    );
    let front = w.open("work").await;
    w.say(&front.session, "tidy my inbox").await;

    let asked = w.infer.asked();
    assert_eq!(asked.len(), 2);
    let (before, after) = (tool_names(&asked[0]), tool_names(&asked[1]));
    assert_ne!(before.first().map(String::as_str), Some(ARCHIVE), "{before:?}");
    assert_eq!(after.first().map(String::as_str), Some(ARCHIVE), "{after:?}");
    // Nothing was added or removed: the same tools in another order.
    let (mut a, mut b) = (before.clone(), after.clone());
    a.sort();
    b.sort();
    assert_eq!(a, b);

    // The body is in the view for the rest of the task, as installed text.
    assert!(!w.infer.user_text(0).contains("MAIL-BODY"));
    assert!(w.infer.user_text(1).contains("MAIL-BODY"));

    // The session record says which skill and which version, never the body.
    let notes: Vec<String> = w
        .records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Session { slug, json, .. } if slug.as_str() == "skill_loaded" => {
                Some(json.as_str().to_owned())
            }
            _ => None,
        })
        .collect();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("mail-tips") && notes[0].contains("1.9"), "{}", notes[0]);
    assert!(!notes[0].contains("MAIL-BODY"));
}

#[tokio::test]
async fn at_most_three_skills_load_in_a_task_and_a_hidden_one_never() {
    let mut w = world_with_skills(
        vec![
            call(LOAD, json!({ "id": "mail-tips" })),
            call(LOAD, json!({ "id": "extra-one" })),
            call(LOAD, json!({ "id": "extra-two" })),
            call(LOAD, json!({ "id": "shell-tips" })),
            call(LOAD, json!({ "id": "ghost" })),
            words("Done."),
        ],
        installed(),
    );
    let front = w.open("work").await;
    w.say(&front.session, "read everything").await;

    let loads: Vec<(bool, String)> = w
        .records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Call { action, end, .. } if action.name.as_str() == "companion.skill.load" => {
                Some((matches!(end, CallEnd::Done), format!("{end:?}")))
            }
            _ => None,
        })
        .collect();
    let done: Vec<bool> = loads.iter().map(|l| l.0).collect();
    assert_eq!(done, [true, true, true, false, false], "{loads:?}");
    assert!(loads[3].1.starts_with("Refused(App(Failed("), "the fourth is the app's refusal: {loads:?}");
    // Only the three loaded skills are recorded as loaded.
    let recorded = w
        .records()
        .into_iter()
        .filter(|r| matches!(r, AuditRecord::Session { slug, .. } if slug.as_str() == "skill_loaded"))
        .count();
    assert_eq!(recorded, 3);
}
