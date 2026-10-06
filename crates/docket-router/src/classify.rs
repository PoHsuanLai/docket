//! Per-call effect, as a pure step: what the router gates a call on once its provider has
//! classified it. The declared effect is a ceiling; the answer can lower it and never raise it.
//!
//! The rules, in one table:
//!
//! | Provider's answer              | Gated on | Passed to `Perform`    |
//! |--------------------------------|----------|------------------------|
//! | `Effect(e)`                    | min(e, ceiling) | `Effect(min)`    |
//! | `Delegates(inner)`, inner known and not the action itself | `Read` | `Delegates(inner)` |
//! | `Delegates(..)` otherwise      | ceiling  | `Effect(ceiling)`      |
//! | an error, a timeout, no method | ceiling  | `Effect(ceiling)`      |
//!
//! A delegating call's own effect is `Read`, so `Delegates` cannot run a `Destructive` effect of
//! its own without an ask: the real action is a separate typed call (the provider's
//! `Follow::Next`, a child of this call in the same chain), and it gets the full gate.

use docket_core::{
    ActionRef, CallClass, Classification, ClassifyAnswer, ClassifyFault, DelegationEnd, Follow,
    Outcome,
};
use prov::Effect;

/// The classification of one call, given the ceiling and the provider's answer. `known` says
/// whether a delegate is an action in the registry.
pub fn classify_step(
    ceiling: Effect,
    this: &ActionRef,
    answer: Result<CallClass, ClassifyFault>,
    known: impl Fn(&ActionRef) -> bool,
) -> Classification {
    let at_ceiling = |answer: ClassifyAnswer| Classification {
        ceiling,
        answer,
        used: ceiling,
        sent: CallClass::Effect(ceiling),
    };
    match answer {
        Err(fault) => at_ceiling(ClassifyAnswer::Failed(fault)),
        Ok(CallClass::Effect(said)) => {
            let used = said.min(ceiling);
            Classification {
                ceiling,
                answer: ClassifyAnswer::Said(CallClass::Effect(said)),
                used,
                sent: CallClass::Effect(used),
            }
        }
        Ok(CallClass::Delegates(inner)) if inner != *this && known(&inner) => Classification {
            ceiling,
            answer: ClassifyAnswer::Said(CallClass::Delegates(inner.clone())),
            used: Effect::Read,
            sent: CallClass::Delegates(inner),
        },
        Ok(CallClass::Delegates(_)) => {
            at_ceiling(ClassifyAnswer::Failed(ClassifyFault::BadDelegate))
        }
    }
}

/// The classification of a retry: `Perform` said the call changed, so the call is gated again
/// at the ceiling, which a provider must accept.
pub fn at_ceiling(ceiling: Effect) -> Classification {
    Classification {
        ceiling,
        answer: ClassifyAnswer::Failed(ClassifyFault::Changed),
        used: ceiling,
        sent: CallClass::Effect(ceiling),
    }
}

/// How a delegation ended, from the outer call's outcome: the inner call must be the follow-up
/// the provider answered, for the action it said. (A different one is gated normally anyway.)
pub fn delegation_end(to: &ActionRef, outcome: &Outcome) -> DelegationEnd {
    match &outcome.follow {
        Follow::Next(call) if call.action == *to => DelegationEnd::Followed,
        Follow::Next(call) => DelegationEnd::Different(call.action.clone()),
        Follow::Nothing | Follow::Open(_) => DelegationEnd::Unused,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(name: &str) -> ActionRef {
        ActionRef {
            app: prov::AppName::parse("org.quire.Shell").expect("app"),
            name: prov::ActionName::parse(name).expect("name"),
        }
    }

    fn run(
        ceiling: Effect,
        answer: Result<CallClass, ClassifyFault>,
    ) -> (Effect, CallClass, ClassifyAnswer) {
        let c = classify_step(ceiling, &action("shell.menu.activate"), answer, |a| {
            a.name.as_str() == "shell.window.hide" || a.name.as_str() == "shell.menu.activate"
        });
        (c.used, c.sent, c.answer)
    }

    #[test]
    fn the_answer_is_clamped_to_the_ceiling() {
        use Effect::*;
        let table = [
            (Destructive, Read, Read),
            (Destructive, UndoableWrite, UndoableWrite),
            (Destructive, Destructive, Destructive),
            (Outbound, Destructive, Outbound),
            (Read, Destructive, Read),
            (UndoableWrite, Outbound, UndoableWrite),
        ];
        for (ceiling, said, used) in table {
            let (got, sent, _) = run(ceiling, Ok(CallClass::Effect(said)));
            assert_eq!(got, used, "{ceiling:?} {said:?}");
            assert_eq!(sent, CallClass::Effect(used));
        }
    }

    #[test]
    fn a_failure_means_the_ceiling() {
        for fault in [
            ClassifyFault::Refused,
            ClassifyFault::TimedOut,
            ClassifyFault::Unavailable,
            ClassifyFault::Unsupported,
        ] {
            let (used, sent, answer) = run(Effect::Destructive, Err(fault));
            assert_eq!(used, Effect::Destructive);
            assert_eq!(sent, CallClass::Effect(Effect::Destructive));
            assert_eq!(answer, ClassifyAnswer::Failed(fault));
        }
    }

    #[test]
    fn a_known_delegate_is_read_and_is_passed_on() {
        let inner = action("shell.window.hide");
        let (used, sent, _) = run(Effect::Destructive, Ok(CallClass::Delegates(inner.clone())));
        assert_eq!(used, Effect::Read);
        assert_eq!(sent, CallClass::Delegates(inner));
    }

    #[test]
    fn an_unknown_delegate_or_a_self_delegate_means_the_ceiling() {
        for inner in [action("shell.no.such"), action("shell.menu.activate")] {
            let (used, sent, answer) = run(Effect::Destructive, Ok(CallClass::Delegates(inner)));
            assert_eq!(used, Effect::Destructive);
            assert_eq!(sent, CallClass::Effect(Effect::Destructive));
            assert_eq!(answer, ClassifyAnswer::Failed(ClassifyFault::BadDelegate));
        }
    }

    #[test]
    fn nothing_is_ever_gated_above_the_ceiling() {
        use Effect::*;
        for ceiling in [Read, UndoableWrite, Outbound, Destructive] {
            for said in [Read, UndoableWrite, Outbound, Destructive] {
                let (used, ..) = run(ceiling, Ok(CallClass::Effect(said)));
                assert!(used <= ceiling);
            }
        }
    }

    fn outcome(follow: Follow) -> Outcome {
        Outcome {
            value: None,
            said: None,
            show: docket_core::Preview::None,
            undo: docket_core::Undoable::No,
            follow,
        }
    }

    #[test]
    fn a_delegation_is_followed_unused_or_different() {
        let to = action("shell.window.hide");
        let call = |a: ActionRef| docket_core::CallRequest {
            action: a,
            target: docket_core::TargetValue::Nothing,
            args: Default::default(),
            origin: docket_core::Origin::AppInternal,
        };
        assert_eq!(
            delegation_end(&to, &outcome(Follow::Next(call(to.clone())))),
            DelegationEnd::Followed
        );
        let other = action("shell.window.close");
        assert_eq!(
            delegation_end(&to, &outcome(Follow::Next(call(other.clone())))),
            DelegationEnd::Different(other)
        );
        assert_eq!(
            delegation_end(&to, &outcome(Follow::Nothing)),
            DelegationEnd::Unused
        );
    }
}
