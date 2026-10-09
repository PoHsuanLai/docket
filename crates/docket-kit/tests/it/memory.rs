//! Memory sections are off until asked for.

use crate::support::*;

#[tokio::test]
async fn sections_not_asked_for_are_absent() {
    let world = World::new(vec![words("Fine.")]);
    let agent = mail_agent(&world).await.build().expect("agent");
    agent.ask(&asker(), "Look.").await.expect("ask");
    let (system, user) = (world.infer.system_text(0), world.infer.user_text(0));
    for heading in [
        "What the person has told you about themselves",
        "Primer:",
        "Lately:",
        "What recently ended",
        "What you remember that bears on this",
        "Who else is working",
    ] {
        assert!(
            !system.contains(heading) && !user.contains(heading),
            "{heading}"
        );
    }
    assert!(user.contains("You said: Look."));
}
