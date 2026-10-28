//! Checking a call against the declaration it names: every argument is declared and fits its
//! type, every required one is there, defaults fill in, and the target is what the action acts
//! on.

use docket_context::labels::app_label;
use docket_core::ArgFault;
use docket_manifest::{ActionDecl, ParamNeed, TargetKind, fits};
use docket_vocab::{Args, ParamName, ParamType, TargetValue, Value};
use porter_core::AppName;
use prov::Labelled;

/// The name a fault about the target carries.
pub(crate) fn target_name() -> ParamName {
    ParamName::parse("target").expect("`target` is a valid parameter name")
}

/// Whether `text` is nothing but a handle's name, as `#3`.
fn is_handle_name(text: &str) -> bool {
    text.trim()
        .strip_prefix('#')
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// The first text parameter whose whole value is a handle's name, as given (before handles are
/// resolved): a handle goes in as `{"handle": n}`, and its name as words searches for the name.
pub(crate) fn no_handle_names(decl: &ActionDecl, args: &Args) -> Result<(), (ParamName, ArgFault)> {
    let named = decl.params.iter().find(|p| {
        matches!(p.ty, ParamType::Text { .. })
            && matches!(args.get(&p.name), Some(given) if matches!(&given.value, Value::Text(t) if is_handle_name(t)))
    });
    match named {
        Some(p) => Err((p.name.clone(), ArgFault::HandleAsText)),
        None => Ok(()),
    }
}

/// The arguments with defaults filled in (a parameter the router fills is not the caller's to
/// send: `WrongType`), or the first parameter that is wrong and why.
pub(crate) fn check_call(
    decl: &ActionDecl,
    app: &AppName,
    target: &TargetValue,
    mut args: Args,
) -> Result<Args, (ParamName, ArgFault)> {
    check_target(decl, target).map_err(|why| (target_name(), why))?;
    if let Some(stray) = args
        .keys()
        .find(|k| !decl.params.iter().any(|p| &p.name == *k))
    {
        return Err((stray.clone(), ArgFault::WrongType));
    }
    for p in &decl.params {
        match (args.get(&p.name), &p.need) {
            // The router fills these itself, after the check: a caller that sends one is wrong.
            (Some(_), _) if p.filled.is_some() => {
                return Err((p.name.clone(), ArgFault::WrongType));
            }
            (Some(given), _) if fits(&given.value, &p.ty) => {}
            (Some(_), _) => return Err((p.name.clone(), ArgFault::WrongType)),
            (None, ParamNeed::Required) => return Err((p.name.clone(), ArgFault::Missing)),
            (None, ParamNeed::Defaulted(v)) => {
                args.insert(
                    p.name.clone(),
                    Labelled {
                        value: v.clone(),
                        label: app_label(app),
                    },
                );
            }
            (None, ParamNeed::Optional) => {}
        }
    }
    Ok(args)
}

fn check_target(decl: &ActionDecl, target: &TargetValue) -> Result<(), ArgFault> {
    match (&decl.on, target) {
        (TargetKind::Nothing, TargetValue::Nothing) => Ok(()),
        (TargetKind::One(kind), TargetValue::Entities(ids)) => match ids.as_slice() {
            [one] if one.kind == *kind => Ok(()),
            [] => Err(ArgFault::Missing),
            _ => Err(ArgFault::WrongType),
        },
        (TargetKind::Many(kind), TargetValue::Entities(ids)) => {
            if ids.is_empty() {
                Err(ArgFault::Missing)
            } else if ids.iter().all(|e| e.kind == *kind) {
                Ok(())
            } else {
                Err(ArgFault::WrongType)
            }
        }
        (TargetKind::Text, TargetValue::Text(_)) => Ok(()),
        (TargetKind::Files, TargetValue::Files(files)) if !files.is_empty() => Ok(()),
        (TargetKind::Files, TargetValue::Files(_)) => Err(ArgFault::Missing),
        (
            TargetKind::One(_) | TargetKind::Many(_) | TargetKind::Text | TargetKind::Files,
            TargetValue::Nothing,
        ) => Err(ArgFault::Missing),
        _ => Err(ArgFault::WrongType),
    }
}
