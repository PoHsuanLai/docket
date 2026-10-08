//! The call lifecycle as a table: one row per transition of the lifecycle section 4.1.

use action_review::{ReviewReason, ReviewVerdict};
use docket_core::*;
use docket_router::*;
use prov::{ConfirmReceipt, InputProof, SpaceScope, UnixSeconds};

fn app() -> porter_core::AppName {
    porter_core::AppName::parse("org.quire.Mail").expect("app")
}

fn confirm_id() -> ConfirmId {
    ConfirmId::parse("c-1").expect("id")
}

fn sheet(id: &str) -> ConfirmRequest {
    ConfirmRequest {
        id: ConfirmId::parse(id).expect("id"),
        space: prov::SpaceId::parse("work").expect("space"),
        actor: prov::Actor::Unknown,
        app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
        action: LabelText::parse("Send 1 message").expect("words"),
        effect: prov::Effect::Outbound,
        count: porter_core::Count(1),
        detail: ConfirmDetail::Plain,
        lines: vec![],
        why: vec![AskReason::FirstUse],
        taint: TaintNote::Clean,
        offer: ConfirmOffer::OnceOnly,
        always: Default::default(),
        gesture: Gesture::Press,
        anchor: Anchor::Launcher,
        expires: Seconds(120),
        editor: None,
    }
}

fn allow() -> Result<ReviewVerdict, ReviewError> {
    Ok(ReviewVerdict::Allow)
}

fn flag() -> Result<ReviewVerdict, ReviewError> {
    Ok(ReviewVerdict::Ask {
        why: ReviewReason {
            code: ReasonCode::Uncertain,
            text: ReasonText("unsure".into()),
        },
    })
}

fn deny() -> Result<ReviewVerdict, ReviewError> {
    Ok(ReviewVerdict::Deny {
        why: ReviewReason {
            code: ReasonCode::Exfiltration,
            text: ReasonText("no".into()),
        },
    })
}

fn receipt() -> ConfirmReceipt {
    ConfirmReceipt {
        id: confirm_id(),
        input: InputProof::HardwareSeat,
        at: UnixSeconds(1),
        covers: prov::Confidentiality::Secret,
    }
}

fn outcome(undo: Undoable) -> Outcome {
    Outcome {
        value: None,
        said: None,
        show: Preview::None,
        undo,
        follow: Follow::Nothing,
    }
}

fn reviewing(
    planned: &[Stage],
    done: Vec<(Stage, Result<ReviewVerdict, ReviewError>)>,
) -> CallState {
    CallState::Reviewing {
        planned: planned.to_vec(),
        done,
    }
}

fn refused(why: CallRefusal) -> CallState {
    CallState::Done(CallEnd::Refused(why))
}

fn answered(scope: GrantScope) -> CallEvent {
    CallEvent::Answered(ConfirmAnswer::Allowed {
        scope,
        receipt: receipt(),
    })
}

type Row = (
    &'static str,
    CallState,
    CallEvent,
    CallState,
    Vec<CallEffect>,
);

fn rows() -> Vec<Row> {
    use CallEffect as E;
    use CallEvent as V;
    use CallState as S;
    let token = UndoToken::parse("tok-1").expect("token");
    let dispatch = || vec![E::Progress(CallProgress::Dispatched), E::Dispatch];
    let preview = || vec![E::Progress(CallProgress::Previewing), E::DryRun];
    vec![
        (
            "bad arguments end the call",
            S::Received,
            V::ArgsChecked(Err(ArgsRefused {
                param: ParamName::parse("to").expect("param"),
                why: ArgFault::Missing,
            })),
            refused(CallRefusal::BadArgs {
                param: ParamName::parse("to").expect("param"),
                why: ArgFault::Missing,
            }),
            vec![],
        ),
        (
            "good arguments go to the gate",
            S::Received,
            V::ArgsChecked(Ok(())),
            S::Gating,
            vec![],
        ),
        (
            "a halt before the gate",
            S::Received,
            V::Halted,
            refused(CallRefusal::Halted(SpaceScope::Any)),
            vec![],
        ),
        (
            "allow dispatches",
            S::Gating,
            V::Gated(Pending::Run),
            S::Dispatched,
            dispatch(),
        ),
        (
            "review starts at the first planned stage",
            S::Gating,
            V::Gated(Pending::NeedsReview(vec![Stage::Quick, Stage::Deliberate])),
            reviewing(&[Stage::Quick, Stage::Deliberate], vec![]),
            vec![
                E::Progress(CallProgress::Reviewing),
                E::StartReview(Stage::Quick),
            ],
        ),
        (
            "ask previews first",
            S::Gating,
            V::Gated(Pending::Confirm(vec![AskReason::Tainted])),
            S::Previewing,
            preview(),
        ),
        (
            "a refusal by policy counts against the breaker",
            S::Gating,
            V::Gated(Pending::Refuse(CallRefusal::Denied(DenyCode::NotAllowed))),
            refused(CallRefusal::Denied(DenyCode::NotAllowed)),
            vec![E::NoteDenial],
        ),
        (
            "a halt or a budget is no denial",
            S::Gating,
            V::Gated(Pending::Refuse(CallRefusal::OverBudget(BudgetKind::Calls))),
            refused(CallRefusal::OverBudget(BudgetKind::Calls)),
            vec![],
        ),
        (
            "a halt at the gate",
            S::Gating,
            V::Halted,
            refused(CallRefusal::Halted(SpaceScope::Any)),
            vec![],
        ),
        (
            "a quick pass of a low-impact call runs it",
            reviewing(&[Stage::Quick], vec![]),
            V::Verdict(Stage::Quick, allow()),
            S::Dispatched,
            dispatch(),
        ),
        (
            "a quick flag escalates to the deliberate stage",
            reviewing(&[Stage::Quick], vec![]),
            V::Verdict(Stage::Quick, flag()),
            reviewing(
                &[Stage::Quick, Stage::Deliberate],
                vec![(Stage::Quick, flag())],
            ),
            vec![E::StartReview(Stage::Deliberate)],
        ),
        (
            "a flag that the deliberate stage allows still asks",
            reviewing(
                &[Stage::Quick, Stage::Deliberate],
                vec![(Stage::Quick, flag())],
            ),
            V::Verdict(Stage::Deliberate, allow()),
            S::Previewing,
            preview(),
        ),
        (
            "a high-impact call waits for every planned stage",
            reviewing(
                &[Stage::Quick, Stage::Deliberate, Stage::SecondOpinion],
                vec![(Stage::Quick, allow())],
            ),
            V::Verdict(Stage::Deliberate, allow()),
            reviewing(
                &[Stage::Quick, Stage::Deliberate, Stage::SecondOpinion],
                vec![(Stage::Quick, allow()), (Stage::Deliberate, allow())],
            ),
            vec![E::StartReview(Stage::SecondOpinion)],
        ),
        (
            "all three stages allowing runs it",
            reviewing(
                &[Stage::Quick, Stage::Deliberate, Stage::SecondOpinion],
                vec![(Stage::Quick, allow()), (Stage::Deliberate, allow())],
            ),
            V::Verdict(Stage::SecondOpinion, allow()),
            S::Dispatched,
            dispatch(),
        ),
        (
            "a deny ends the review at once and counts",
            reviewing(
                &[Stage::Quick, Stage::Deliberate, Stage::SecondOpinion],
                vec![],
            ),
            V::Verdict(Stage::Quick, deny()),
            refused(CallRefusal::Denied(DenyCode::NotAllowed)),
            vec![E::NoteDenial],
        ),
        (
            "a reviewer that fails asks",
            reviewing(&[Stage::Quick], vec![]),
            V::Verdict(Stage::Quick, Err(ReviewError::Timeout)),
            S::Previewing,
            preview(),
        ),
        (
            "a halt drops the review",
            reviewing(&[Stage::Quick], vec![]),
            V::Halted,
            refused(CallRefusal::Halted(SpaceScope::Any)),
            vec![],
        ),
        (
            "a built sheet goes to the person, under the id the router minted",
            S::Previewing,
            V::Previewed(Box::new(sheet("c-7"))),
            S::Confirming(ConfirmId::parse("c-7").expect("id")),
            vec![
                E::Progress(CallProgress::Confirming(
                    ConfirmId::parse("c-7").expect("id"),
                )),
                E::Confirm(Box::new(sheet("c-7"))),
            ],
        ),
        (
            "a halt while previewing",
            S::Previewing,
            V::Halted,
            refused(CallRefusal::Halted(SpaceScope::Any)),
            vec![],
        ),
        (
            "a yes once dispatches",
            S::Confirming(confirm_id()),
            answered(GrantScope::Once),
            S::Dispatched,
            dispatch(),
        ),
        (
            "an always is remembered, then dispatches",
            S::Confirming(confirm_id()),
            answered(GrantScope::Always),
            S::Dispatched,
            [vec![E::RecordGrant], dispatch()].concat(),
        ),
        (
            "an allow from the terminal is remembered for the terminal, then dispatches",
            S::Confirming(confirm_id()),
            V::Answered(ConfirmAnswer::AllowedFromTerminal { receipt: receipt() }),
            S::Dispatched,
            [vec![E::RecordTerminalGrant], dispatch()].concat(),
        ),
        (
            "a no ends the call and counts",
            S::Confirming(confirm_id()),
            V::Answered(ConfirmAnswer::Ended(ConfirmEnd::Refused)),
            refused(CallRefusal::Unconfirmed(ConfirmEnd::Refused)),
            vec![E::NoteDenial, E::Unconfirmed(ConfirmEnd::Refused)],
        ),
        (
            "a halt withdraws the sheet",
            S::Confirming(confirm_id()),
            V::Halted,
            refused(CallRefusal::Halted(SpaceScope::Any)),
            vec![E::CancelConfirm],
        ),
        (
            "an undoable outcome is journalled",
            S::Dispatched,
            V::AppAnswered(Box::new(Ok(outcome(Undoable::Yes(token))))),
            S::Done(CallEnd::Done),
            vec![E::Journal],
        ),
        (
            "an outcome with no undo is not",
            S::Dispatched,
            V::AppAnswered(Box::new(Ok(outcome(Undoable::No)))),
            S::Done(CallEnd::Done),
            vec![],
        ),
        (
            "the app refuses",
            S::Dispatched,
            V::AppAnswered(Box::new(Err(AppRefusal::Busy))),
            refused(CallRefusal::App(AppRefusal::Busy)),
            vec![],
        ),
        (
            "the app is silent",
            S::Dispatched,
            V::AppTimedOut,
            refused(CallRefusal::Timeout),
            vec![],
        ),
        (
            "the app is not there",
            S::Dispatched,
            V::AppUnavailable(app()),
            refused(CallRefusal::AppUnavailable(app())),
            vec![],
        ),
        (
            "a halt does not recall a call in flight",
            S::Dispatched,
            V::Halted,
            S::Dispatched,
            vec![],
        ),
        (
            "an ended call takes no more events",
            S::Done(CallEnd::Done),
            V::Halted,
            S::Done(CallEnd::Done),
            vec![],
        ),
        (
            "an event that does not apply changes nothing",
            S::Gating,
            V::AppTimedOut,
            S::Gating,
            vec![],
        ),
    ]
}

#[test]
fn call_step_table() {
    for (name, state, event, want_state, want_effects) in rows() {
        let (got_state, got_effects) = call_step(state, event);
        assert_eq!(got_state, want_state, "case: {name}");
        assert_eq!(got_effects, want_effects, "case: {name}");
    }
}

#[test]
fn a_halt_cancels_the_sheet_and_the_review() {
    let (state, effects) = call_step(CallState::Confirming(confirm_id()), CallEvent::Halted);
    assert_eq!(state, refused(CallRefusal::Halted(SpaceScope::Any)));
    assert_eq!(effects, vec![CallEffect::CancelConfirm]);
    let (state, effects) = call_step(
        CallState::Reviewing {
            planned: vec![Stage::Quick, Stage::Deliberate],
            done: vec![],
        },
        CallEvent::Halted,
    );
    assert_eq!(state, refused(CallRefusal::Halted(SpaceScope::Any)));
    assert!(
        effects.is_empty(),
        "no further stage starts once the review is dropped: {effects:?}"
    );
}

#[test]
fn only_the_tighten_rule_lets_a_reviewed_call_run() {
    // Every verdict a planned stage can give, in every position of a two-stage plan.
    let verdicts = [allow(), flag(), deny(), Err(ReviewError::Unavailable)];
    for quick in &verdicts {
        for deliberate in &verdicts {
            let (state, _) = call_step(
                CallState::Reviewing {
                    planned: vec![Stage::Quick, Stage::Deliberate],
                    done: vec![(Stage::Quick, quick.clone())],
                },
                CallEvent::Verdict(Stage::Deliberate, deliberate.clone()),
            );
            let both_allow = quick == &allow() && deliberate == &allow();
            assert_eq!(
                state == CallState::Dispatched,
                both_allow,
                "quick {quick:?}, deliberate {deliberate:?}"
            );
        }
    }
}
