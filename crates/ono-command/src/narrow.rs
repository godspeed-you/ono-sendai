//! What a context frame fills in for a command (spec §14.3, §14.5, ADR-0023, ADR-0076).
//!
//! A frame narrows; it never redirects. Its whole contribution is arguments the user did not
//! type, applied once at the seam every implementation runs through: inside `enter process 1`,
//! `get process` is `get process 1` and `trace process` is `trace process 1`; inside
//! `enter user root`, `get process` is `get process --user root`. Every contribution is a
//! parameter the command declares — or, where the command declares none and the answering
//! schema carries the frame's target as a field, a selector on that field — so what a frame does
//! can always be written out by hand.

use ono_core::ErrorCode;
use ono_provider_api::ProviderRegistry;
use ono_value::ErrorValue;

use crate::bind::BoundArguments;
use crate::contract::CommandContract;
use crate::invoke::{ContextFrame, FrameKind};

/// The arguments `frames` fill in for `contract`, or `None` when no frame contributes.
///
/// # Errors
///
/// `resolve.target_not_found` when an object frame can narrow the command neither through a
/// parameter nor through a field of the answering schema. Spec §14.3 forbids the alternative:
/// a command that quietly widens to the whole machine while the prompt names one object.
pub(crate) fn narrow(
    contract: &CommandContract,
    providers: &ProviderRegistry,
    frames: &[ContextFrame],
    arguments: &BoundArguments,
) -> Result<Option<BoundArguments>, ErrorValue> {
    // Only a command that asks a provider about a target has anything to narrow: `get context`,
    // `help` and every transform mean the same thing in every frame.
    let Some(target) = contract.target() else {
        return Ok(None);
    };
    if contract.provider_capability().is_none() {
        return Ok(None);
    }

    let mut narrowed: Option<BoundArguments> = None;
    for frame in frames {
        if frame.kind() != FrameKind::Object {
            continue;
        }
        let current = narrowed.as_ref().unwrap_or(arguments);
        narrowed = Some(contribute(contract, providers, target, frame, current)?);
    }
    Ok(narrowed)
}

fn contribute(
    contract: &CommandContract,
    providers: &ProviderRegistry,
    target: &str,
    frame: &ContextFrame,
    arguments: &BoundArguments,
) -> Result<BoundArguments, ErrorValue> {
    if frame.target() == target {
        // The entered object itself: the first declared parameter the object has a handle for —
        // `pid` for a process, `name` for an interface, `port` for a socket.
        if let Some((spec, value)) = contract
            .selectors()
            .iter()
            .find_map(|spec| frame.handle(spec.name()).map(|value| (spec, value)))
        {
            return Ok(arguments.clone().with_selector(spec.name(), value.clone()));
        }
        if let Some((spec, value)) = contract
            .options()
            .iter()
            .find_map(|spec| frame.handle(spec.name()).map(|value| (spec, value)))
        {
            return Ok(arguments.clone().with_option(spec.name(), value.clone()));
        }
        // No parameter fits; the schema's identity does, because every provider matches a
        // selector on a field it declares.
        let mut identity = arguments.clone();
        let mut narrowed = false;
        for field in identity_fields(providers, target) {
            if let Some(value) = frame.handle(&field) {
                identity = identity.with_ambient(&field, value.clone());
                narrowed = true;
            }
        }
        if narrowed {
            return Ok(identity);
        }
        return Err(cannot_narrow(contract, frame));
    }

    // Another target: the parameter named after the frame's target — `--user root`,
    // `--interface lo` — carries the frame's identity.
    if contract
        .selectors()
        .iter()
        .any(|spec| spec.name() == frame.target())
    {
        return Ok(arguments
            .clone()
            .with_selector(frame.target(), frame.identity().clone()));
    }
    if contract
        .options()
        .iter()
        .any(|spec| spec.name() == frame.target())
    {
        return Ok(arguments
            .clone()
            .with_option(frame.target(), frame.identity().clone()));
    }

    // The schema that decides is the one the answering provider advertises, because that is the
    // schema the selector will be matched against — a KUANG/11 provider may extend a target with
    // fields the built-in registry has never heard of (spec §31.23).
    let narrows = providers
        .for_target(target)
        .iter()
        .flat_map(|provider| provider.schemas())
        .any(|schema| schema.field(frame.target()).is_some());
    if narrows {
        return Ok(arguments
            .clone()
            .with_ambient(frame.target(), frame.identity().clone()));
    }

    Err(cannot_narrow(contract, frame))
}

/// The identity fields of the schema the providers answer `target` with.
fn identity_fields(providers: &ProviderRegistry, target: &str) -> Vec<String> {
    providers
        .for_target(target)
        .iter()
        .flat_map(|provider| provider.schemas())
        .flat_map(|schema| {
            schema
                .identity()
                .iter()
                .map(|name| name.to_string())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn cannot_narrow(contract: &CommandContract, frame: &ContextFrame) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ResolveTargetNotFound,
        format!(
            "`{}` has no meaning inside the context `{}`",
            contract.spelling(),
            frame.spelling(),
        ),
    )
    .with_help(format!(
        "the {} of `{}` carries no `{}` field to narrow by. `leave` the context, or write the \
         query explicitly (spec §14.5)",
        contract.output().text(),
        contract.spelling(),
        frame.target(),
    ))
}
