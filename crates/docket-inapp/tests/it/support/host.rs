//! What the multi-task, skills and audit-queue tests share: the Mail provider with two threads, a
//! scripted model and a virtual clock, built into the parts of an agent.

use super::infer::ScriptedInfer;
use super::{Nowhere, TestSheet};
use docket_core::AgentConfig;
use docket_core::Millis;
use docket_fake::{FakeMail, MailThread, ScriptedReviewer, mail_manifest};
use docket_inapp::InAppParts;
use docket_router::Clock;
use prov::{SpaceId, UnixSeconds};
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

pub const ARCHIVE: &str = "org.quire.Mail-mail.thread.archive";

pub fn work() -> SpaceId {
    SpaceId::parse("work").expect("space")
}

pub fn thread(key: &str) -> serde_json::Value {
    json!({ "app": "org.quire.Mail", "kind": "mail.thread", "key": key })
}

pub fn mail() -> FakeMail {
    let mail = FakeMail::new(mail_manifest().expect("manifest"), work());
    for (key, subject) in [("t1", "Invoice"), ("t2", "Digest")] {
        mail.add_thread(MailThread {
            key: key.into(),
            subject: subject.into(),
            from: "news@example.test".into(),
            body: "This week".into(),
        });
    }
    mail
}

/// A clock a test moves by hand: it says the second it was last set to, and a wait of any length
/// never completes (nothing here sleeps).
#[derive(Debug, Clone)]
pub struct MovingClock(Arc<AtomicI64>);

impl MovingClock {
    pub fn at(start: i64) -> Self {
        Self(Arc::new(AtomicI64::new(start)))
    }

    pub fn advance(&self, seconds: i64) {
        self.0.fetch_add(seconds, Ordering::SeqCst);
    }
}

impl Clock for MovingClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(self.0.load(Ordering::SeqCst))
    }

    async fn after(&self, wait: Millis) {
        if wait.0 != 0 {
            std::future::pending::<()>().await;
        }
    }
}

pub type Parts =
    InAppParts<FakeMail, Nowhere, TestSheet, ScriptedReviewer, ScriptedInfer, MovingClock>;

/// The parts of an agent over `model`, asking on `sheet`, on `clock`.
pub fn parts(model: &ScriptedInfer, sheet: &TestSheet, clock: &MovingClock) -> Parts {
    InAppParts {
        provider: mail(),
        context: Nowhere,
        sheet: sheet.clone(),
        reviewer: ScriptedReviewer::always_allow(),
        model: model.clone(),
        clock: clock.clone(),
        space: work(),
        config: AgentConfig::default(),
    }
}

pub fn clock() -> MovingClock {
    MovingClock::at(1_000)
}
