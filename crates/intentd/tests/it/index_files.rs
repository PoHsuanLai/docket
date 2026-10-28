//! What the index keeps on disk: the entries an app pushed come back at the next start, a file of
//! an app that is no longer installed goes, and the entries apps ship are read from the data
//! directories. Every path is a tempdir.

use crate::support::world::{app, caller};
use docket_core::*;
use docket_fake::{FakeSeams, fake_router};
use docket_router::Router;
use docket_vocab::Generation;
use docket_wire::{CallerRole, IntentsReply, IntentsRequest};
use intentd::{IndexStore, ship_search_files};
use prov::{EntityKey, EntityKind, SpaceScope, UnixSeconds};
use std::os::unix::fs::PermissionsExt;

fn router() -> Router<FakeSeams> {
    fake_router(AgentConfig::default()).expect("router")
}

fn entry(key: &str, title: &str) -> IndexEntry {
    IndexEntry {
        key: EntityKey::parse(key).expect("key"),
        kind: EntityKind::parse("mail.thread").expect("kind"),
        title: title.into(),
        subtitle: String::new(),
        keywords: vec![],
        updated: UnixSeconds(1),
        space: SpaceScope::Any,
    }
}

fn found(router: &Router<FakeSeams>, text: &str) -> Vec<String> {
    let ask = SearchAsk {
        text: text.into(),
        scope: SearchScope::Everything,
        generation: Generation(1),
    };
    router
        .search_indexed(CallerRole::Launcher, &ask)
        .iter()
        .map(|h| h.entity.id.key.as_str().to_owned())
        .collect()
}

#[tokio::test]
async fn what_an_app_pushed_is_found_again_by_a_fresh_router() {
    let data = tempfile::tempdir().expect("scratch");
    let store = IndexStore::under(data.path());
    let mail = app("org.quire.Mail");
    let router = router();
    let owner = caller("org.quire.Mail", &[CallerRole::App]);
    for request in [
        IntentsRequest::IndexReset { epoch: 1 },
        IntentsRequest::IndexPush(IndexBatch {
            epoch: 1,
            upserts: vec![entry("t1", "Invoice")],
            removals: vec![],
        }),
    ] {
        assert_eq!(router.handle(&owner, request).await, IntentsReply::Done);
    }
    store.save(&router, &mail);

    let file = data.path().join("quire/intents/index/org.quire.Mail.json");
    let mode = std::fs::metadata(&file)
        .expect("the file")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "private to the person");

    let fresh = self::router();
    assert!(found(&fresh, "invoice").is_empty());
    store.restore(&fresh);
    assert_eq!(found(&fresh, "invoice"), ["t1"]);
}

#[test]
fn a_file_of_an_app_that_is_not_installed_is_removed_and_a_broken_one_is_left() {
    let data = tempfile::tempdir().expect("scratch");
    let store = IndexStore::under(data.path());
    store
        .write(&app("com.gone.App"), vec![entry("g1", "Gone")])
        .expect("written");
    let dir = data.path().join("quire/intents/index");
    std::fs::write(dir.join("org.quire.Mail.json"), "not json").expect("write");
    store.restore(&router());
    assert!(!dir.join("com.gone.App.json").exists());
    assert!(dir.join("org.quire.Mail.json").exists());
}

fn ship(data: &std::path::Path, name: &str, text: &str) {
    let dir = data.join("quire/search");
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join(name), text).expect("write");
}

#[test]
fn shipped_files_are_read_for_installed_apps_and_an_earlier_directory_wins() {
    let mine = tempfile::tempdir().expect("scratch");
    let system = tempfile::tempdir().expect("scratch");
    let page = |title: &str| {
        format!(
            "[[entries]]\nkind = \"mail.thread\"\nkey = \"p1\"\ntitle = \"{title}\"\nkeywords = [\"wireless\"]\n"
        )
    };
    ship(mine.path(), "org.quire.Mail.toml", &page("Mine"));
    ship(system.path(), "org.quire.Mail.toml", &page("System"));
    ship(system.path(), "com.gone.App.toml", &page("Gone"));
    ship(system.path(), "org.quire.Files.toml", "this is not toml");
    let router = router();
    ship_search_files(
        &router,
        &[mine.path().to_path_buf(), system.path().to_path_buf()],
    );
    assert_eq!(found(&router, "mine"), ["p1"]);
    assert_eq!(found(&router, "wireless"), ["p1"]);
    assert!(found(&router, "system").is_empty());
    assert!(found(&router, "gone").is_empty());
}
