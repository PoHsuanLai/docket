//! Limits: precedence, the shapes that cannot say "allow", old files still reading, and the
//! administrator's floor.

use crate::support::*;
use docket_core::*;
use prov::{ActionName, Effect, SpaceScope};

fn name(text: &str) -> ActionName {
    ActionName::parse(text).expect("action")
}

fn key(level: LimitLevel, scope: LimitScope, space_scope: SpaceScope, from: Effect) -> LimitKey {
    LimitKey {
        caller: LimitCaller::AnyAgent,
        scope,
        space: space_scope,
        from,
        level,
    }
}

fn limit(key: LimitKey, by: LimitMaker) -> Limit {
    Limit::new(key, by, None, at(1))
}

fn call_in<'a>(
    caller: &'a GrantCaller,
    app: &'a porter_core::AppName,
    action: &'a ActionName,
    in_space: &'a prov::SpaceId,
    effect: Effect,
) -> LimitCall<'a> {
    LimitCall {
        caller,
        app,
        action: Some(action),
        space: in_space,
        effect,
    }
}

#[test]
fn the_strongest_covering_limit_wins_and_the_floor_wins_a_tie() {
    let files = app("org.quire.Files");
    let delete = name("files.file.delete");
    let work = space("work");
    let home = space("home");
    let caller = GrantCaller::Companion;
    let here = SpaceScope::Only(work.clone());
    let ask = limit(
        key(
            LimitLevel::AlwaysAsk,
            LimitScope::App(files.clone()),
            here.clone(),
            Effect::UndoableWrite,
        ),
        LimitMaker::Person,
    );
    let never = limit(
        key(
            LimitLevel::Never,
            LimitScope::Action(ActionRef {
                app: files.clone(),
                name: delete.clone(),
            }),
            here.clone(),
            Effect::Destructive,
        ),
        LimitMaker::Assistant,
    );
    let locked_ask = limit(
        key(
            LimitLevel::AlwaysAsk,
            LimitScope::Everything,
            SpaceScope::Any,
            Effect::Read,
        ),
        LimitMaker::Floor,
    );
    let level = |held: &[Limit], locked: &[Limit], c: &LimitCall<'_>| match limited(held, locked, c)
    {
        Limited::Free => Level::AlwaysAllow,
        Limited::Ask(_) => Level::AlwaysAsk,
        Limited::Never(_) => Level::Never,
    };
    let none: Vec<Limit> = Vec::new();
    let only_ask = vec![ask.clone()];
    let both = vec![ask, never];
    let locked = vec![locked_ask.clone()];
    let rows = [
        // (what is held, what is locked, the call, the level it ends at)
        (
            "nothing held: free",
            &none,
            &none,
            call_in(&caller, &files, &delete, &work, Effect::Destructive),
            Level::AlwaysAllow,
        ),
        (
            "ask alone asks",
            &only_ask,
            &none,
            call_in(&caller, &files, &delete, &work, Effect::UndoableWrite),
            Level::AlwaysAsk,
        ),
        (
            "never beats always ask",
            &both,
            &none,
            call_in(&caller, &files, &delete, &work, Effect::Destructive),
            Level::Never,
        ),
        (
            "a read is below the effect the limit starts at",
            &both,
            &none,
            call_in(&caller, &files, &delete, &work, Effect::Read),
            Level::AlwaysAllow,
        ),
        (
            "another Space is not covered",
            &both,
            &none,
            call_in(&caller, &files, &delete, &home, Effect::Destructive),
            Level::AlwaysAllow,
        ),
        (
            "a locked entry binds where the person set nothing",
            &none,
            &locked,
            call_in(&caller, &files, &delete, &home, Effect::Read),
            Level::AlwaysAsk,
        ),
        (
            "a stronger entry of the person's still wins over a locked ask",
            &both,
            &locked,
            call_in(&caller, &files, &delete, &work, Effect::Destructive),
            Level::Never,
        ),
    ];
    for (row, held, locked, c, want) in rows {
        assert_eq!(level(held, locked, &c), want, "{row}");
    }
    // The reason shown for a tie is the administrator's.
    let tie = limited(
        &[limit(
            key(
                LimitLevel::AlwaysAsk,
                LimitScope::Everything,
                SpaceScope::Any,
                Effect::Read,
            ),
            LimitMaker::Person,
        )],
        &[locked_ask],
        &call_in(&caller, &files, &delete, &work, Effect::Read),
    );
    match tie {
        Limited::Ask(note) => {
            assert_eq!(note.undo, UndoOffer::Withheld);
            assert!(note.why.contains("administrator"), "{}", note.why);
        }
        other => panic!("expected an ask, got {other:?}"),
    }
}

#[test]
fn a_level_that_allows_is_not_a_limit() {
    for allowing in ["\"always\"", "\"always_allow\"", "\"allow\""] {
        assert!(
            serde_json::from_str::<LimitLevel>(allowing).is_err(),
            "{allowing}"
        );
    }
    assert_eq!(
        serde_json::from_str::<LimitLevel>("\"never\"").expect("never"),
        LimitLevel::Never
    );
    assert_eq!(
        LimitLevel::try_from(Level::AlwaysAllow),
        Err(NotATightening)
    );
    // Strength is the order of the variants.
    assert_eq!(Level::AlwaysAllow.over(Level::AlwaysAsk), Level::AlwaysAsk);
    assert_eq!(Level::Never.over(Level::AlwaysAsk), Level::Never);
    assert_eq!(Level::default(), Level::AlwaysAllow);
}

#[test]
fn a_file_with_no_level_still_reads_as_always_allow() {
    // A grant request written before limits has no level: it is an always-allow.
    let now = GrantAsk {
        app: app("org.quire.Mail"),
        space: space("work"),
        level: Level::Never,
        run: None,
    };
    let mut old = serde_json::to_value(&now).expect("json");
    old.as_object_mut().expect("object").remove("level");
    let ask: GrantAsk = serde_json::from_value(old).expect("an old request still reads");
    assert_eq!(ask.level, Level::AlwaysAllow);
    // A sheet written before limits offers no "never" and names no limit.
    assert_eq!(SheetLimits::default().never, NeverOffer::Withheld);
    // An empty limits file holds none, and a list round-trips.
    assert_eq!(decode_limits("  ").expect("empty"), vec![]);
    let l = limit(
        key(
            LimitLevel::Never,
            LimitScope::Everything,
            SpaceScope::Any,
            Effect::Outbound,
        ),
        LimitMaker::Person,
    );
    let text = encode_limits(std::slice::from_ref(&l)).expect("text");
    assert_eq!(decode_limits(&text).expect("back"), vec![l]);
    assert!(decode_limits("[{\"level\":\"always_allow\"}]").is_err());
}

#[test]
fn what_the_assistant_adds_is_theirs_to_remove_by_id_only() {
    let made = Tighten {
        level: LimitLevel::Never,
        scope: LimitScope::App(app("org.quire.Files")),
        space: SpaceScope::Only(space("work")),
        from: Effect::UndoableWrite,
        said: Some(Said::parse("don't touch my Work folder").expect("words")),
    }
    .limit(at(5));
    assert_eq!(made.by, LimitMaker::Assistant);
    let note = made.note();
    assert_eq!(note.id, made.id);
    assert_eq!(note.undo, UndoOffer::Offered);
    assert!(
        note.why.contains("don't touch my Work folder"),
        "{}",
        note.why
    );
    let (rest, done) = limits_without(vec![made.clone()], &made.id);
    assert!(rest.is_empty());
    assert_eq!(done, Revocation::Revoked);
    let (_, none) = limits_without(
        vec![made],
        &StandingGrantId::parse("sg-0000000000000000").expect("id"),
    );
    assert_eq!(none, Revocation::NotHeld);
}

#[test]
fn the_floor_clamps_strictness_and_locks_entries() {
    let loose = Floor::none();
    assert_eq!(loose.clamp(Strictness::TrustMore), Strictness::TrustMore);
    let file = FloorFile {
        strictness: Some(Strictness::Default),
        locked: vec![FloorEntry {
            level: LimitLevel::Never,
            app: Some(app("org.quire.Files")),
            action: Some(name("files.file.delete")),
            from: Effect::Destructive,
        }],
    };
    let floor = Floor::from_file(file).expect("floor");
    for (wanted, want) in [
        (Strictness::TrustMore, Strictness::Default),
        (Strictness::Default, Strictness::Default),
        (Strictness::AskMore, Strictness::AskMore),
    ] {
        assert_eq!(floor.clamp(wanted), want, "{wanted:?}");
    }
    assert_eq!(floor.locked().len(), 1);
    assert_eq!(floor.locked()[0].by, LimitMaker::Floor);
    let bad = FloorFile {
        strictness: None,
        locked: vec![FloorEntry {
            level: LimitLevel::AlwaysAsk,
            app: None,
            action: Some(name("files.file.delete")),
            from: Effect::Read,
        }],
    };
    assert_eq!(Floor::from_file(bad), Err(FloorFault::ActionWithoutApp));
}

#[test]
fn a_limit_is_a_rule_in_a_file_that_is_added_replaced_and_removed_by_id() {
    let made = Tighten {
        level: LimitLevel::AlwaysAsk,
        scope: LimitScope::Action(ActionRef {
            app: app("org.quire.Mail"),
            name: name("mail.thread.archive"),
        }),
        space: SpaceScope::Only(space("work")),
        from: Effect::UndoableWrite,
        said: Some(Said::parse("say \"ask\" first").expect("words")),
    }
    .limit(at(5));
    let rule = rule_text(&made);
    assert!(rule.starts_with(&format!("@id(\"{}\")", made.id.as_str())), "{rule}");
    assert!(rule.contains("perform_unasked") && rule.contains("perform_unjudged"), "{rule}");
    assert!(rule.contains("context.space == \"work\""), "{rule}");
    assert!(rule.contains("[\"undoable_write\", \"outbound\""), "{rule}");
    assert!(rule.contains("say \\\"ask\\\" first"), "the quote is escaped: {rule}");
    let never = Tighten {
        level: LimitLevel::Never,
        scope: LimitScope::Everything,
        space: SpaceScope::Any,
        from: Effect::Read,
        said: None,
    }
    .limit(at(6));
    assert!(rule_text(&never).contains("action == Quire::Action::\"perform\""));
    // Added twice is one rule; a hand-written rule beside it stays; removal is by id.
    let by_hand = "forbid (principal, action, resource) when { false };";
    let file = with_rule(&with_rule(by_hand, &made), &made);
    assert_eq!(rule_ids(&file), vec![made.id.clone()]);
    let both = with_rule(&file, &never);
    assert_eq!(rule_ids(&both).len(), 2);
    let (left, done) = without_rule(&both, &made.id);
    assert_eq!(done, Revocation::Revoked);
    assert_eq!(rule_ids(&left), vec![never.id.clone()]);
    assert!(left.contains(by_hand));
    let (_, none) = without_rule(&left, &made.id);
    assert_eq!(none, Revocation::NotHeld);
    assert_eq!(RuleFile::of(LimitMaker::Floor), None);
}
