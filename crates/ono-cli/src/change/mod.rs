//! The v0.6 change family: the thirteen commands of §5 and §37.5, as this shell binds them
//! (spec v0.6 §5, §36, §37, §50, §55.7).
//!
//! §50.1 splits v0.6 across a family of crates and gives none of them a shell. What is left for
//! `ono-cli` is exactly what §55.7 leaves it: the session state a command needs and cannot reach
//! — the plan store, the recovery provider registry, the spatial index, the temporal ledger and
//! the terminal a gate is asked at — and the dispatch that puts the two together. Planning,
//! coverage, execution, recovery and rendering are `ono-change-plan`, `-protection`, `-executor`,
//! `-recovery` and `-render`'s, and every command below composes them.
//!
//! # The one behaviour worth stating twice
//!
//! §2.1 and §62.3 both turn on `plan` changing nothing, and [`plan::answer`] is written to it: it
//! reads providers, reads the topology, asks the recovery providers what they *could* offer, and
//! writes one row to the plan store. The view it produces ends with
//! [`ono_change_render::PLAN_NOT_EXECUTED`], which is the whole of how an operator tells the two
//! worlds apart.
//!
//! # Where each command lives
//!
//! | module | commands |
//! |---|---|
//! | [`plan`] | `plan` (§5.1, §5.2, §5.3) — claimed by the evaluator, not the registry (ADR-0814) |
//! | [`lifecycle`] | `impact`, `protect`, `apply`, `verify` (§5.4 – §5.7) |
//! | [`inspect`] | `get plan`, `inspect plan`, `rebase plan`, `resume plan` (§36.4, §7.5, §41.3) |
//! | [`recovery`] | `recover`, `get recovery`, `inspect recovery`, `remove recovery` (§5.8, §37.5) |
//! | [`actions`] | which operations §6.1 makes plannable, and what refuses |
//! | [`world`] | the four things the change layer asks of a provider |
//! | [`session`] | the store, the registry and the settings one shell holds |

pub mod actions;
pub mod gates;
pub mod inspect;
pub mod lifecycle;
pub mod plan;
pub mod recovery;
pub mod render;
pub mod session;
pub mod world;

pub use inspect::{GetPlan, InspectPlan, RebasePlan, ResumePlan};
pub use lifecycle::{Apply, Impact, Protect, Verify};
pub use plan::{answer, claims};
pub use recovery::{GetRecovery, InspectRecovery, Recover, RemoveRecovery};
pub use session::{change_session, configure_from};

use std::sync::Arc;

use ono_change_core::{ChangePlan, PlanId, RecoveryAssetId, error};
use ono_change_plan::{PlanStore, Reference};
use ono_command::Invocation;
use ono_core::ErrorCode;
use ono_value::{ErrorValue, Value};

use self::session::ChangeState;

/// The plan a command was pointed at (§36.4, ADR-0803).
///
/// §36.4 asks for three spellings and this resolves all of them: `a82f` and `plan/a82f` are an
/// identity or an unambiguous prefix of one, and `@` is the plan this shell just produced. A
/// record arriving through a pipe carries its own identity, which is the fourth way the same
/// question is asked.
///
/// # Errors
///
/// `change.plan_not_found` for a reference nothing matches, and
/// `change.plan_reference_ambiguous` where a prefix matches more than one plan.
pub fn resolve_plan(store: &PlanStore, reference: &str) -> Result<PlanId, ErrorValue> {
    let trimmed = reference.trim();
    if trimmed == "@" {
        return session::last_plan().ok_or_else(|| {
            error::plan_not_found("@").with_help(
                "`@` is the plan this shell just produced, and this shell has produced none — \
                 name the plan, or `get plan` to list them (§36.4)",
            )
        });
    }
    let parsed = Reference::parse(trimmed)?;
    if !parsed.names_a_plan() {
        return Err(error::plan_not_found(trimmed).with_help(
            "`recovery/…` names a recovery asset; a plan is `a82f` or `plan/a82f` (§36.4)",
        ));
    }
    store.resolve(parsed.body())
}

/// The recovery asset a command was pointed at (§37.5).
///
/// # Errors
///
/// `recovery.asset_not_found` for a reference nothing matches.
pub fn resolve_asset(store: &PlanStore, reference: &str) -> Result<RecoveryAssetId, ErrorValue> {
    let trimmed = reference.trim();
    let parsed = Reference::parse(trimmed)?;
    if !parsed.names_an_asset() {
        return Err(error::asset_not_found(trimmed)
            .with_help("`plan/…` names a plan; an asset is `r-9f21` or `recovery/9f21` (§37.5)"));
    }
    store.resolve_asset(parsed.body())
}

/// The reference an invocation names, from its selector or from the record it was piped.
///
/// # Errors
///
/// `type.mismatch` naming the selector and the pipe, where neither carried one.
pub async fn reference_of(ctx: &mut Invocation<'_>, selector: &str) -> Result<String, ErrorValue> {
    let arguments = ctx.arguments().evaluated(ctx.scope())?;
    if let Some(value) = arguments.selector(selector)
        && let Some(text) = identity_of(value)
    {
        return Ok(text);
    }
    // §5.3 and every one of these contracts declare `input: null | ono.change-plan/1`, so a
    // plan that arrived through the pipe is a plan that was named: `plan … | apply` is the
    // sequence §5's examples are written in, and refusing it would make the declared input type
    // a promise nothing keeps.
    if let Some(text) = piped_reference(ctx).await {
        return Ok(text);
    }
    Err(ErrorValue::new(
        ErrorCode::TypeMismatch,
        format!(
            "`{}` needs a plan reference, and none was given",
            arguments.spelling()
        ),
    )
    .with_help(
        "`a82f` or `plan/a82f` (§36.4). `\"@\"` is the plan this shell just produced, and the \
         quotes are needed: v0.2 §6.4 already gives a bare `@` to the current value (ADR-0803)",
    ))
}

/// The reference the one record on this command's input carries, where there is exactly one.
///
/// Exactly one, because §5.3's pipeline of *objects* becomes one plan and a pipeline of *plans*
/// is a set somebody has to choose from: applying several plans because they arrived together is
/// not a decision this shell makes for an operator (§2.7, §40.1).
async fn piped_reference(ctx: &mut Invocation<'_>) -> Option<String> {
    let mut input = ctx.take_input()?;
    let mut found = None;
    while let Some(event) = input.recv().await {
        let ono_pipeline::StreamEvent::Value(value) = event else {
            continue;
        };
        if found.is_some() {
            return None;
        }
        found = identity_of(&value);
    }
    found
}

/// A selector value as the reference it carries: a word, or a record's own identity.
fn identity_of(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) if text.trim().is_empty() => None,
        Value::String(text) => Some(text.to_string()),
        Value::Record(record) => record
            .get("plan_id")
            .or_else(|| record.get("asset_id"))
            .or_else(|| record.get("id"))
            .and_then(|held| held.as_str().ok().map(str::to_owned)),
        Value::List(items) => items.first().and_then(identity_of),
        other => ono_value::canonical_text(other).ok(),
    }
}

/// The plan a command was pointed at, read back out of the store (§36.1).
///
/// # Errors
///
/// The store's refusal, or `change.plan_not_found` for a reference nothing matches.
pub fn plan_of(state: &ChangeState, reference: &str) -> Result<ChangePlan, ErrorValue> {
    let id = resolve_plan(state.store(), reference)?;
    state.store().get(&id)
}

/// Whether there is a person at the other end of this invocation (§40.2, §40.3).
///
/// §40.3 forbids a script waiting for a question, so this decides which of the two paths a gate
/// takes: a terminal is asked, and everything else is refused with the structured error and the
/// machine-readable plan on its metadata. Both halves are needed — values about to be consumed by
/// another stage are values whatever terminal the shell is attached to (v0.4 §29.1).
#[must_use]
pub fn is_interactive(ctx: &Invocation<'_>) -> bool {
    ctx.displays() && crate::spatial::at_terminal()
}

/// The runtime handle a command's synchronous seams run on.
///
/// # Errors
///
/// `provider.unavailable` where no runtime is running, which cannot happen from a command the
/// evaluator awaited and is reported rather than unwrapped.
pub fn runtime_handle() -> Result<tokio::runtime::Handle, ErrorValue> {
    tokio::runtime::Handle::try_current().map_err(|_| {
        ErrorValue::new(
            ErrorCode::ProviderUnavailable,
            "the shell could not reach the runtime this command runs on",
        )
    })
}

/// `record` with `value` attached under the namespaced key `key` (v0.2 §10.4).
///
/// §46's schemas are the contract and this never widens one: a v0.6 view that needs a fact the
/// schema does not declare — Appendix B.10's persistence resolution, §21.2's plan overlay — puts
/// it in the provider extension space, where `to json` carries it and a reader can tell it apart
/// from a declared field by its namespaced name.
#[must_use]
pub fn extended(
    record: &ono_value::RecordValue,
    key: &str,
    value: Value,
) -> ono_value::RecordValue {
    let mut rebuilt =
        ono_value::RecordValue::builder(Arc::clone(record.schema()), record.provenance().clone());
    for (index, field) in record.schema().fields().iter().enumerate() {
        let held = record.field_at(index).cloned().unwrap_or(Value::Null);
        rebuilt = match rebuilt.set(field.name(), held) {
            Ok(next) => next,
            Err(_) => return record.clone(),
        };
    }
    for (existing, held) in record.extra().iter() {
        rebuilt = rebuilt.set_extra(existing, held.clone());
    }
    rebuilt.set_extra(key, value).build()
}

/// Whether one `ono.temporal-event/1` belongs to the plan `reference` names (v0.6 §22.4).
///
/// §22.4 makes the plan identity a causal anchor rather than a second history: every event
/// `ono-change-executor` writes carries the identity in its payload, and this reads it back. The
/// comparison is by prefix because §36.4's reference is an unambiguous prefix of an identity, and
/// `plan/` is stripped for the same reason.
#[must_use]
pub fn names_plan(event: &Value, reference: &str) -> bool {
    let wanted = reference
        .trim()
        .trim_start_matches(ono_change_core::PlanId::PREFIX);
    let Ok(record) = event.as_record() else {
        return false;
    };
    let Some(payload) = record.get("payload") else {
        return false;
    };
    let Ok(map) = payload.as_map() else {
        return false;
    };
    map.get("plan")
        .and_then(|value| value.as_str().ok())
        .is_some_and(|held| held.starts_with(wanted))
}

/// The `ono.change-plan/1` value of `plan`, for a command that answers with one.
///
/// # Errors
///
/// Whatever the schema refused the record for.
pub fn plan_value(plan: &ChangePlan) -> Result<Value, ErrorValue> {
    Ok(Value::Record(Arc::new(
        ono_change_core::value::plan_record(plan)?,
    )))
}

/// The map with a plan overlaid on it (§21.2, §21.3, §21.4).
///
/// Three rules shape what this attaches, and all three are §21's:
///
/// - **§21.2** the overlay is over the map that was drawn. Nothing is added to it and nothing is
///   re-selected; each drawn node is marked or it is not.
/// - **§21.3** a node a recovery asset covers says so, because "protected" and "about to change"
///   are different facts and an operator reading a map before a change window needs both.
/// - **§21.4** no future identity is fabricated. A restarted service's workers do not exist yet,
///   so the overlay records that a replacement is expected and never a process id.
///
/// # Errors
///
/// `change.plan_not_found` where the reference names no plan, and the store's own refusals.
pub async fn overlay(
    record: ono_value::RecordValue,
    reference: &str,
) -> Result<ono_value::RecordValue, ErrorValue> {
    let state = change_session().await?;
    let plan = plan_of(&state, reference)?;
    let assets = state.store().assets_for(plan.id()).unwrap_or_default();
    let mut covered: Vec<String> = Vec::new();
    for id in &assets {
        let Ok(asset) = state.store().get_asset(id) else {
            continue;
        };
        for object in asset.scope().covers() {
            covered.push(object.to_string());
        }
    }
    let mut rows = Vec::new();
    for target in plan.targets() {
        let mut row = ono_value::MapValue::new();
        row.insert("object".into(), Value::string(target.label()));
        row.insert("identity".into(), Value::string(target.identity()));
        row.insert(
            "place".into(),
            target.spatial_id().map_or(Value::Null, Value::string),
        );
        row.insert(
            "covered".into(),
            Value::Bool(covered.iter().any(|object| object == target.label())),
        );
        // §21.4: the word is what the plan expects to happen to the object, never a name for an
        // object that does not exist. `replacement expected` is the honest form of a restart.
        row.insert(
            "expectation".into(),
            Value::string(expectation(&plan, target)),
        );
        rows.push(Value::Map(Arc::new(row)));
    }
    let mut summary = ono_value::MapValue::new();
    summary.insert("plan".into(), Value::string(plan.id().as_str()));
    summary.insert("state".into(), Value::string(plan.state().as_str()));
    summary.insert(
        "protection".into(),
        Value::string(plan.protection().level().as_str()),
    );
    summary.insert("objects".into(), Value::list(rows));
    Ok(extended(
        &record,
        "ono.change/plan-overlay",
        Value::Map(Arc::new(summary)),
    ))
}

/// What the plan expects to happen to one target, in words rather than in identities (§21.4).
fn expectation(plan: &ChangePlan, target: &ono_change_core::FrozenTarget) -> &'static str {
    let kind = plan
        .effects()
        .into_iter()
        .find(|effect| effect.object() == Some(target.label()))
        .map(ono_change_core::ProposedEffect::kind);
    match kind {
        Some(ono_change_core::EffectKind::Create) => "created",
        Some(ono_change_core::EffectKind::Remove) => "removed",
        // §21.4's own example: a restarted service's worker set is replaced, and the replacement
        // has no identity yet. Saying that it is expected is the whole of what is known.
        Some(ono_change_core::EffectKind::Replace) => "replacement expected",
        Some(ono_change_core::EffectKind::Interrupt) => "interruption possible",
        Some(ono_change_core::EffectKind::Emit) => "outward call",
        Some(ono_change_core::EffectKind::Modify) => "modified",
        _ => "unknown",
    }
}

/// `explain plan <ref>` — why the plan says what it says (§5, §19.2, §10.3, §4.4).
///
/// `None` where `subject` is not `plan <reference>`, so `explain` falls through to its ordinary
/// pipeline report and `explain plan restart service nginx` still explains the command.
///
/// The three things it shows are the three a reader is actually asking about:
///
/// - **the provider bindings** §4.4 sealed, because a plan resolved against one version of a
///   provider is not the same plan against another (Appendix G.4);
/// - **the rules that fired**, each with the sentence the rule itself carries — §19.2 makes risk
///   rule-based rather than a score, and `ono-change-impact`'s rule table is where the sentence
///   lives, so the rule and its explanation cannot drift apart;
/// - **the coverage reasoning** Appendix A produced, per mutation domain, including what it
///   excluded — §10.3 is what makes "partially protected" a statable answer rather than a
///   footnote under a boolean.
///
/// # Errors
///
/// `change.plan_not_found` where the reference names no plan, and the plan store's own refusals.
pub fn explanation(
    session: &mut crate::session::Session,
    subject: &str,
) -> Result<Option<Vec<String>>, ErrorValue> {
    let words: Vec<&str> = subject.split_whitespace().collect();
    let [head, reference] = words.as_slice() else {
        return Ok(None);
    };
    if *head != "plan" || Reference::parse(reference).is_err() {
        return Ok(None);
    }
    let Some(handle) = session.runtime().map(|runtime| runtime.handle().clone()) else {
        return Ok(None);
    };
    let reference = (*reference).to_owned();
    handle
        .clone()
        .block_on(async move { explain_plan(&reference).await })
        .map(Some)
}

/// The lines `explain plan` prints.
async fn explain_plan(reference: &str) -> Result<Vec<String>, ErrorValue> {
    let state = change_session().await?;
    let plan = plan_of(&state, reference)?;
    let mut lines = vec![
        format!(
            "  plan {} revision {} is {}",
            plan.id().short(),
            plan.revision(),
            plan.state().as_str()
        ),
        format!("  intent: {}", plan.intent().text()),
        String::new(),
        "  provider bindings — what §4.4 sealed this plan against".to_owned(),
    ];
    if plan.providers().is_empty() {
        lines.push("    none: the plan carries no action a provider owns".to_owned());
    }
    for binding in plan.providers() {
        lines.push(format!("    {} at {}", binding.id(), binding.version()));
    }
    lines.push(String::new());
    lines.push(format!(
        "  rules that fired — §19.2 composes them to {}",
        plan.risk().classify().as_str()
    ));
    if plan.risk().findings().is_empty() {
        lines.push("    none: no rule objected to this plan".to_owned());
    }
    for finding in plan.risk().findings() {
        lines.push(format!(
            "    {} [{}] {}",
            finding.rule(),
            finding.class().as_str(),
            finding.reason()
        ));
        if let Some(rule) = ono_change_impact::risk::rule(finding.rule()) {
            lines.push(format!("      the rule: {}", rule.doc()));
        }
    }
    lines.push(String::new());
    lines.push(format!(
        "  coverage reasoning — Appendix A composed {}",
        plan.protection().level().as_str()
    ));
    if plan.protection().rows().is_empty() {
        lines.push("    none: the plan declares no mutation domain".to_owned());
    }
    for row in plan.protection().rows() {
        lines.push(format!(
            "    {} needs {} and is {}",
            row.domain().as_str(),
            row.objective().as_str(),
            row.protection().as_str()
        ));
        lines.push(format!("      {}", row.note()));
    }
    for exclusion in plan.protection().exclusions() {
        lines.push(format!(
            "    excluded: {} — {}",
            exclusion.subject(),
            exclusion.reason()
        ));
    }
    Ok(lines)
}
