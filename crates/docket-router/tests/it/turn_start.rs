//! The start of a person's turn may read memory unasked, and nothing else may: the claim
//! (`Origin::TurnStart`) is checked against the action, the session's newest turn and the
//! caller, and a claim that fails is refused, never turned into a sheet. The planner's own
//! call of the same action still asks.

use crate::support::*;
use docket_core::*;
use docket_fake::{BoxFut, HostedApp};
use prov::{ActionName, AgentRef, Labelled};
use std::sync::Arc;

#[derive(Debug)]
struct Facts;

impl HostedApp for Facts {
    fn perform(&self, _inv: Invocation) -> BoxFut<Result<Outcome, AppRefusal>> {
        Box::pin(async {
            Ok(Outcome {
                value: Some(Labelled {
                    value: Value::Entities(vec![prov::EntityId {
                        app: app(RECALL_APP),
                        kind: prov::EntityKind::parse("memory.fact").expect("kind"),
                        key: prov::EntityKey::parse("f1").expect("key"),
                    }]),
                    label: trusted(),
                }),
                said: LabelText::parse("Found 1 thing").ok(),
                show: Preview::None,
                undo: Undoable::No,
                follow: Follow::Nothing,
            })
        })
    }
}

fn recall(origin: Origin, action: &str) -> CallRequest {
    CallRequest {
        action: ActionRef {
            app: app(RECALL_APP),
            name: ActionName::parse(action).expect("action"),
        },
        target: TargetValue::Nothing,
        args: [(
            param("query"),
            Labelled {
                value: Value::Text("capital of Taiwan".into()),
                label: trusted(),
            },
        )]
        .into(),
        origin,
    }
}

fn memory_router() -> docket_router::Router<docket_fake::FakeSeams> {
    let router = router();
    router.seams.link.host(app(RECALL_APP), Arc::new(Facts));
    router
}

#[tokio::test]
async fn only_the_search_on_a_turn_the_person_typed_runs_unasked_and_the_planners_own_call_asks() {
    // (what is said first, the call, what comes of it)
    let table = [
        (true, Origin::TurnStart, RECALL_ACTION, "runs"),
        (true, Origin::Companion, RECALL_ACTION, "asks"),
        (false, Origin::TurnStart, RECALL_ACTION, "refused"),
        (true, Origin::TurnStart, "memory.facts", "refused"),
    ];
    for (spoken, origin, action, want) in table {
        let router = memory_router();
        let opened = open(&router, "work", AgentRef::Companion).await;
        if spoken {
            say(&router, &opened.session, "what is the capital of Taiwan").await;
        }
        let end = perform(&router, recall(origin, action)).await;
        let sheets = router.seams.confirmer.requests().len();
        match want {
            "runs" => {
                assert!(end.is_ok(), "{end:?}");
                assert_eq!(sheets, 0, "no sheet");
            }
            "asks" => {
                assert_eq!(end, Err(CallRefusal::Unconfirmed(ConfirmEnd::Dismissed)));
                assert_eq!(sheets, 1, "the model's call asks as before");
            }
            _ => {
                assert_eq!(
                    end,
                    Err(CallRefusal::Denied(DenyCode::NotAllowed)),
                    "{action}"
                );
                assert_eq!(sheets, 0, "refused, not asked");
            }
        }
    }
}

#[tokio::test]
async fn a_turn_start_read_is_audited_as_the_companions_call() {
    let router = memory_router();
    let opened = open(&router, "work", AgentRef::Companion).await;
    say(&router, &opened.session, "what is the capital of Taiwan").await;
    perform(&router, recall(Origin::TurnStart, RECALL_ACTION))
        .await
        .expect("runs");
    let audited = router.seams.sink.records().into_iter().any(|r| {
        matches!(
            r,
            AuditRecord::Call { actor: prov::Actor::Companion { .. }, action, end: CallEnd::Done, .. }
                if action.name.as_str() == RECALL_ACTION
        )
    });
    assert!(audited, "one Call record, actor companion");
}
