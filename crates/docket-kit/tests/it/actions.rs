//! An action outside the chosen set is not declared to the model.

use crate::support::*;
use docket_kit::Actions;
use prov::Effect;

fn tool_names(infer: &ScriptedInfer) -> Vec<String> {
    infer.asked()[0]
        .tools
        .iter()
        .map(|t| t.name.as_str().to_owned())
        .collect()
}

#[tokio::test]
async fn reads_only_declares_no_write() {
    let world = World::new(vec![words("Fine.")]);
    let link = link(&world);
    let all = catalogue(&link).await;
    let reads = Actions::from(&all).app("org.quire.Mail").reads_only();
    assert!(
        reads
            .catalogue()
            .tools()
            .iter()
            .all(|t| t.decl.effect == Effect::Read),
        "the set holds reads"
    );
    assert!(
        all.tools()
            .iter()
            .any(|t| t.decl.effect > Effect::Read && t.app.as_str() == "org.quire.Mail"),
        "the fixture has writes to leave out"
    );
    let agent = docket_kit::Agent::builder(world.infer.clone(), link)
        .actions(reads)
        .build()
        .expect("agent");
    agent.ask(&asker(), "Look.").await.expect("ask");
    let names = tool_names(&world.infer);
    assert!(!names.iter().any(|n| n == ARCHIVE), "{names:?}");
    assert!(
        names.iter().any(|n| n == "quire_finish"),
        "the meta tools stay"
    );
}

#[tokio::test]
async fn named_declares_exactly_those() {
    let world = World::new(vec![words("Fine.")]);
    let link = link(&world);
    let only = Actions::from(&catalogue(&link).await).named(&["mail.thread.archive"]);
    let agent = docket_kit::Agent::builder(world.infer.clone(), link)
        .actions(only)
        .build()
        .expect("agent");
    agent.ask(&asker(), "Look.").await.expect("ask");
    let actions: Vec<_> = tool_names(&world.infer)
        .into_iter()
        .filter(|n| !n.starts_with("quire_"))
        .collect();
    assert_eq!(actions, vec![ARCHIVE.to_owned()]);
}

#[tokio::test]
async fn an_app_that_is_not_there_is_named_in_the_fault() {
    let world = World::new(vec![]);
    let link = link(&world);
    let none = Actions::from(&catalogue(&link).await).app("org.example.Nope");
    assert_eq!(none.missing().len(), 1);
    assert!(none.catalogue().tools().is_empty());
}
