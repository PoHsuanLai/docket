//! Checking a call against the declaration it names: every argument is declared and fits its
//! type, every required one is there, defaults fill in, and the target is what the action acts
//! on.

use crate::labels::app_label;
use docket_core::{
    ActionDecl, ArgFault, Args, ParamName, ParamNeed, TargetKind, TargetValue, fits,
};
use porter_core::AppName;
use prov::Labelled;

/// The name a fault about the target carries.
fn target_name() -> ParamName {
    ParamName::parse("target").expect("`target` is a valid parameter name")
}

/// The arguments with defaults filled in, or the first parameter that is wrong and why.
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
