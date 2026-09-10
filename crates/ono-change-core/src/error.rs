//! The refusals of v0.6 (spec §45).
//!
//! Every refusal in the change and recovery families is raised through a constructor here, so a
//! code and the metadata that travels with it are decided once. §45 requires errors to be
//! "structured values with provenance and remediation hints where safe", and the shape each
//! constructor produces is the same one the rest of the shell uses: a code, a sentence, a `help`
//! that says what to do, and metadata a script can match on without reading prose.
//!
//! One convention runs through the whole module and is worth stating: a refusal in this family
//! nearly always means *nothing was changed*, and where that is true the message says so. §2.3,
//! §7.3, §17.2 and Appendix F all turn on the operator being able to tell "it refused" from "it
//! stopped partway", and prose is the only place that distinction can live for a person.

use ono_core::ErrorCode;
use ono_value::{ErrorValue, Value};

use crate::id::{ActionId, PlanId, RecoveryAssetId};
use crate::protection::ProtectionLevel;
use crate::state::{LifecycleEvent, PlanState};

fn plan_metadata(error: ErrorValue, plan: &PlanId) -> ErrorValue {
    error.with_metadata("plan", Value::string(plan.as_str()))
}

/// A sealed plan was given to something that edits one (§4.4).
#[must_use]
pub fn plan_not_editable(plan: &PlanId, state: PlanState) -> ErrorValue {
    plan_metadata(
        ErrorValue::new(
            ErrorCode::ChangePlanSealed,
            format!("plan {} is {state} and cannot be edited", plan.short()),
        )
        .with_help(format!(
            "a sealed plan is immutable (v0.6 §4.4). `rebase plan {}` opens a new revision \
             against current state and leaves this one exactly as it is",
            plan.short()
        )),
        plan,
    )
    .with_metadata("state", Value::string(state.as_str()))
}

/// A plan that is not sealed was given to `apply`, `protect` or `verify` (§5.6).
#[must_use]
pub fn plan_not_sealed(plan: &PlanId, state: PlanState) -> ErrorValue {
    plan_metadata(
        ErrorValue::new(
            ErrorCode::ChangePlanNotSealed,
            format!(
                "plan {} is {state}, and only a sealed plan can be applied",
                plan.short()
            ),
        )
        .with_help(help_for_unappliable(state)),
        plan,
    )
    .with_metadata("state", Value::string(state.as_str()))
}

/// Why a plan in this state cannot be applied, in the words of the state it is actually in.
///
/// §4.2's draft is one of three quite different situations, and telling an operator about a draft
/// when the plan already applied is worse than saying nothing: §2.7 makes a sealed plan apply
/// once, and the next step after "it already ran" is `inspect` or `recover`, not `seal`.
fn help_for_unappliable(state: PlanState) -> String {
    if state.has_mutated() {
        return "v0.6 §2.7: a sealed plan applies once, and this one already did. `inspect plan` \
                shows what it did and `recover` plans the way back. Nothing was changed."
            .to_owned();
    }
    if state.is_editable() {
        return "v0.6 §4.2 keeps a draft out of the executor deliberately: a plan is sealed, and \
                therefore digest-bearing and immutable, before it may apply. Nothing was changed."
            .to_owned();
    }
    "v0.6 §4.1 draws no edge from this state to APPLYING. `inspect plan` shows where the plan is \
     and what may follow it. Nothing was changed."
        .to_owned()
}

/// The plan's validity window has closed (§5.6).
#[must_use]
pub fn plan_expired(plan: &PlanId) -> ErrorValue {
    plan_metadata(
        ErrorValue::new(
            ErrorCode::ChangePlanExpired,
            format!("plan {} has expired", plan.short()),
        )
        .with_help(format!(
            "its targets were frozen against a world that has had time to move (v0.6 §5.6). \
             `rebase plan {}` resolves it again. Nothing was changed",
            plan.short()
        )),
        plan,
    )
}

/// Another session holds the apply claim on this plan (§42.4).
#[must_use]
pub fn plan_already_applying(plan: &PlanId, holder: &str) -> ErrorValue {
    plan_metadata(
        ErrorValue::new(
            ErrorCode::ChangePlanAlreadyApplying,
            format!("plan {} is already being applied by {holder}", plan.short()),
        )
        .with_help(
            "v0.6 §42.4: the plan store prevents two sessions from applying one sealed plan at \
             once. Nothing was changed here."
                .to_owned(),
        ),
        plan,
    )
    .with_metadata("holder", Value::string(holder))
}

/// A lifecycle transition §4.1 does not draw.
#[must_use]
pub fn invalid_transition(plan: &PlanId, from: PlanState, event: LifecycleEvent) -> ErrorValue {
    plan_metadata(
        ErrorValue::new(
            ErrorCode::ChangePlanStateInvalid,
            format!(
                "plan {} is {from}, and `{event}` is not a transition from there",
                plan.short()
            ),
        )
        .with_help("v0.6 §4.1 is a state machine, not a set of labels".to_owned()),
        plan,
    )
    .with_metadata("state", Value::string(from.as_str()))
    .with_metadata("transition", Value::string(event.as_str()))
}

/// No plan matches the reference (§36.4).
#[must_use]
pub fn plan_not_found(reference: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ChangePlanNotFound,
        format!("no plan matches `{reference}`"),
    )
    .with_help(
        "a plan is referenced by its identity or an unambiguous prefix of it (v0.6 §36.4). \
         `get plan` lists what the store holds"
            .to_owned(),
    )
    .with_metadata("reference", Value::string(reference))
}

/// The reference matches more than one plan (§36.4).
#[must_use]
pub fn plan_reference_ambiguous(reference: &str, candidates: &[String]) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ChangePlanReferenceAmbiguous,
        format!(
            "`{reference}` matches {} plans, so it names none of them",
            candidates.len()
        ),
    )
    .with_help("write enough of the identity to tell them apart (v0.6 §36.4)".to_owned())
    .with_metadata("reference", Value::string(reference))
    .with_metadata(
        "candidates",
        Value::list(candidates.iter().map(|id| Value::string(id))),
    )
}

/// The plan store could not be opened or written (§36.1).
#[must_use]
pub fn store_unavailable(detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ChangePlanStoreUnavailable,
        "the plan store cannot be read",
    )
    .with_help(detail.to_owned())
    .with_retryable(true)
}

/// The plan store failed its integrity check (§36.2).
#[must_use]
pub fn store_corrupt(detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ChangePlanStoreCorrupt,
        "the plan store failed its integrity check",
    )
    .with_help(detail.to_owned())
    .with_retryable(false)
}

/// A persisted record could not be read back (§36.2).
///
/// This is `change.plan_store_corrupt` rather than a parse error, and the distinction matters:
/// the bytes came out of Ono's own store, so a field that will not read is a store this build
/// cannot trust rather than input a user got wrong. §36.2's versioned schema is what should have
/// prevented it, and naming the field is what makes the version that broke findable.
#[must_use]
pub fn record_malformed(field: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ChangePlanStoreCorrupt,
        format!("the stored record's `{field}` field cannot be read"),
    )
    .with_help(format!(
        "v0.6 §36.2: the plan store is versioned and checked on open, so a field this build \
         cannot read is a store it cannot trust. {detail}"
    ))
    .with_metadata("field", Value::string(field))
    .with_retryable(false)
}

/// A selector resolved to nothing (§4.3).
#[must_use]
pub fn target_unresolved(selector: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ChangeTargetUnresolved,
        format!("`{selector}` does not resolve to an object this plan could freeze"),
    )
    .with_help(format!(
        "v0.6 §4.3 freezes identities, not selectors, so a selector matching nothing cannot \
         become a plan. {detail}"
    ))
    .with_metadata("selector", Value::string(selector))
}

/// A frozen target is no longer the object the plan resolved (§7.1).
#[must_use]
pub fn target_changed(target: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ChangeTargetChanged,
        format!("`{target}` is no longer the object this plan resolved"),
    )
    .with_help(format!(
        "v0.6 §7.1: identity includes generation, inode or unit identity, so a replaced object \
         is a different object. {detail} Nothing was changed"
    ))
    .with_metadata("target", Value::string(target))
}

/// The operation has no contract v0.6 can plan from (§6.1).
#[must_use]
pub fn action_not_plannable(operation: &str, missing: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ChangeActionNotPlannable,
        format!("`{operation}` cannot be planned"),
    )
    .with_help(format!(
        "v0.6 §6.1 requires a provider contract that declares target schema, mutation semantics, \
         capabilities, preconditions, effects, execution method, idempotency class, recovery \
         semantics and verification options. {missing}"
    ))
    .with_metadata("operation", Value::string(operation))
    .with_metadata("missing", Value::string(missing))
}

/// The action graph has a cycle (§3.2).
#[must_use]
pub fn action_graph_cyclic(plan: &PlanId, cycle: &[ActionId]) -> ErrorValue {
    plan_metadata(
        ErrorValue::new(
            ErrorCode::ChangeActionNotPlannable,
            format!(
                "plan {} has {} actions that depend on each other in a cycle",
                plan.short(),
                cycle.len()
            ),
        )
        .with_help(
            "v0.6 §3.2: a plan contains a directed acyclic action graph unless a provider-local \
             transaction encapsulates the cycle"
                .to_owned(),
        ),
        plan,
    )
    .with_metadata(
        "actions",
        Value::list(cycle.iter().map(|id| Value::string(id.as_str()))),
    )
}

/// An opaque external command was given to `plan` (§6.2).
#[must_use]
pub fn opaque_action_forbidden(command: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ChangeOpaqueActionForbidden,
        format!("`{command}` cannot be planned, because Ono cannot reason about what it touches"),
    )
    .with_help(
        "v0.6 §6.2: an arbitrary command has no declared target scope or side effects. §6.3's \
         `opaque action` entry exists where an operator accepts that; it classifies impact and \
         reversibility as unknown and earns no protection from a snapshot taken for something else"
            .to_owned(),
    )
    .with_metadata("command", Value::string(command))
}

/// A plan for application was asked for from a historical coordinate (§6.4).
#[must_use]
pub fn historical_context_read_only(command: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ChangeHistoricalContextReadOnly,
        format!(
            "`{command}` resolves against the world it would change, and the session is in the past"
        ),
    )
    .with_help(
        "v0.6 §6.4: historical state can be inspected but not used as the executable target base. \
         `now` returns to the present, where the plan resolves against what it would touch"
            .to_owned(),
    )
    .with_metadata("command", Value::string(command))
}

/// A declared precondition did not hold (§7.2).
#[must_use]
pub fn precondition_failed(action: &ActionId, subject: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ChangePreconditionFailed,
        format!("a precondition of `{subject}` did not hold"),
    )
    .with_help(format!("{detail} Nothing was changed"))
    .with_metadata("action", Value::string(action.as_str()))
    .with_metadata("subject", Value::string(subject))
}

/// Material drift stopped the plan before preparation (§7.3).
#[must_use]
pub fn drift_detected(plan: &PlanId, findings: &[(String, String, String)]) -> ErrorValue {
    let rendered: Vec<Value> = findings
        .iter()
        .map(|(subject, field, detail)| Value::string(&format!("{subject}.{field}: {detail}")))
        .collect();
    plan_metadata(
        ErrorValue::new(
            ErrorCode::ChangePlanDriftDetected,
            format!(
                "plan {} was resolved against state that has since changed",
                plan.short()
            ),
        )
        .with_help(format!(
            "v0.6 §7.3: material drift stops execution before anything is prepared or mutated. \
             Nothing was changed. `rebase plan {}` creates a new revision against the world as it \
             is now",
            plan.short()
        )),
        plan,
    )
    .with_metadata("drift", Value::list(rendered))
}

/// Preparation failed, and nothing was mutated (§2.3, §4.5).
#[must_use]
pub fn prepare_failed(
    plan: &PlanId,
    action: &str,
    detail: &str,
    retained: &[String],
) -> ErrorValue {
    plan_metadata(
        ErrorValue::new(
            ErrorCode::ChangePrepareFailed,
            format!("preparing plan {} failed at `{action}`", plan.short()),
        )
        .with_help(format!(
            "v0.6 §2.3: if a required recovery asset cannot be created, mutation MUST NOT begin. \
             Nothing was changed. {detail}"
        )),
        plan,
    )
    .with_metadata("action", Value::string(action))
    .with_metadata(
        "retained_assets",
        Value::list(retained.iter().map(|id| Value::string(id))),
    )
}

/// A mutating action failed (§4.7, Appendix F).
#[must_use]
pub fn apply_failed(
    plan: &PlanId,
    action: &str,
    completed: usize,
    not_executed: usize,
) -> ErrorValue {
    plan_metadata(
        ErrorValue::new(
            ErrorCode::ChangeApplyFailed,
            format!("applying plan {} failed at `{action}`", plan.short()),
        )
        .with_help(format!(
            "v0.6 Appendix F: {completed} action(s) completed and {not_executed} were not \
             executed. Protection created for this plan is retained. `inspect plan {}` shows the \
             exact state; `recover plan/{}` builds a recovery plan without applying one",
            plan.short(),
            plan.short()
        )),
        plan,
    )
    .with_metadata("action", Value::string(action))
    .with_metadata(
        "completed",
        Value::Int(i128::try_from(completed).unwrap_or(0)),
    )
    .with_metadata(
        "not_executed",
        Value::Int(i128::try_from(not_executed).unwrap_or(0)),
    )
}

/// A required postcondition did not hold (§23.2).
#[must_use]
pub fn verification_failed(plan: &PlanId, failures: &[String]) -> ErrorValue {
    plan_metadata(
        ErrorValue::new(
            ErrorCode::ChangeVerificationFailed,
            format!(
                "plan {} did not verify: {} required check(s) failed",
                plan.short(),
                failures.len()
            ),
        )
        .with_help(
            "v0.6 §2.14: a command that returned success has not proved that the intended state \
             exists. The plan is FAILED and its recovery assets are retained (§37.2)"
                .to_owned(),
        ),
        plan,
    )
    .with_metadata(
        "failed_checks",
        Value::list(failures.iter().map(|check| Value::string(check))),
    )
}

/// A verification check could not be answered (§23.3).
#[must_use]
pub fn verification_unknown(check: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ChangeVerificationUnknown,
        format!("`{check}` could not be observed"),
    )
    .with_help(format!(
        "v0.6 §2.4 and §23.5: an unanswerable check is UNKNOWN, and that is not a pass. {detail}"
    ))
    .with_metadata("check", Value::string(check))
}

/// A mutating plan carries no verification contract (§23.1).
#[must_use]
pub fn verification_missing(plan: &PlanId) -> ErrorValue {
    plan_metadata(
        ErrorValue::new(
            ErrorCode::ChangeVerificationMissing,
            format!(
                "plan {} changes the system and declares nothing to check afterwards",
                plan.short()
            ),
        )
        .with_help(
            "v0.6 §23.1: every plan containing a MUTATE action MUST have at least one \
             verification contract, even where the minimum is provider-level state \
             acknowledgement. A plan that cannot be checked cannot be sealed"
                .to_owned(),
        ),
        plan,
    )
}

/// An irreversible plan has not been acknowledged (§19.4).
#[must_use]
pub fn irreversible_not_accepted(plan: &PlanId, reasons: &[String]) -> ErrorValue {
    plan_metadata(
        ErrorValue::new(
            ErrorCode::ChangeIrreversibleNotAccepted,
            format!(
                "plan {} contains {} irreversible action(s) that have not been acknowledged",
                plan.short(),
                reasons.len()
            ),
        )
        .with_help(format!(
            "v0.6 §19.4 stores the acknowledgement in the sealed revision. \
             `apply plan/{} --accept-irreversible` re-seals the plan with it. Nothing was changed",
            plan.short()
        )),
        plan,
    )
    .with_metadata(
        "irreversible",
        Value::list(reasons.iter().map(|reason| Value::string(reason))),
    )
}

/// A high or critical plan has not been acknowledged (§19.4, §40.2).
#[must_use]
pub fn risk_not_accepted(plan: &PlanId, class: &str, reasons: &[String]) -> ErrorValue {
    plan_metadata(
        ErrorValue::new(
            ErrorCode::ChangeRiskNotAccepted,
            format!(
                "plan {} is {class} risk and was not acknowledged",
                plan.short()
            ),
        )
        .with_help(format!(
            "v0.6 §40.2: the reason is in the metadata rather than behind a generic question. \
             `apply plan/{} --accept-risk` acknowledges it. A script supplies it as policy and \
             never waits for a prompt (§40.3). Nothing was changed",
            plan.short()
        )),
        plan,
    )
    .with_metadata("risk", Value::string(class))
    .with_metadata(
        "reasons",
        Value::list(reasons.iter().map(|reason| Value::string(reason))),
    )
}

/// A bulk plan reaches further than its guard permits (§28.3).
#[must_use]
pub fn bulk_guard_failed(plan: &PlanId, targets: usize, guard: usize) -> ErrorValue {
    plan_metadata(
        ErrorValue::new(
            ErrorCode::ChangeBulkGuardFailed,
            format!(
                "plan {} would act on {targets} objects, more than the guard of {guard}",
                plan.short()
            ),
        )
        .with_help(
            "v0.6 §28.3: scope is a risk dimension of its own. Narrow the selection, raise \
             `change.bulk.high_risk_targets` deliberately, or acknowledge the risk. Nothing was \
             changed"
                .to_owned(),
        ),
        plan,
    )
    .with_metadata("targets", Value::Int(i128::try_from(targets).unwrap_or(0)))
    .with_metadata("guard", Value::Int(i128::try_from(guard).unwrap_or(0)))
}

/// A remote action's outcome could not be established (§29.3, Appendix F.2).
#[must_use]
pub fn remote_state_unknown(host: &str, action: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ChangeRemoteStateUnknown,
        format!("what `{action}` did on {host} could not be established"),
    )
    .with_help(
        "v0.6 §29.3: Ono MUST NOT mark an unknown remote action failed or successful without \
         evidence. Querying again when the link returns is the way to resolve it"
            .to_owned(),
    )
    .with_metadata("host", Value::string(host))
    .with_metadata("action", Value::string(action))
    .with_retryable(true)
}

/// An auto-recovery declaration §26.3 does not permit.
#[must_use]
pub fn auto_recovery_rejected(plan: &PlanId, unmet: &[String]) -> ErrorValue {
    plan_metadata(
        ErrorValue::new(
            ErrorCode::ChangeAutoRecoveryRejected,
            format!(
                "plan {} declares automatic recovery and does not meet {} of §26.3's conditions",
                plan.short(),
                unmet.len()
            ),
        )
        .with_help(
            "v0.6 §26.3 permits the declaration only where the recovery plan can be fully \
             constructed before mutation, no known irreversible external side effect exists, \
             recovery destroys no unrelated newer state, protection is PROTECTED or TRANSACTIONAL \
             for the required domains, recovery verification exists, and policy enables it"
                .to_owned(),
        ),
        plan,
    )
    .with_metadata(
        "unmet",
        Value::list(unmet.iter().map(|reason| Value::string(reason))),
    )
}

/// An action or its recovery needs privilege this session does not hold (§43.3, §43.4).
#[must_use]
pub fn privilege_required(subject: &str, privilege: &str, for_recovery: bool) -> ErrorValue {
    let purpose = if for_recovery {
        "recovering it"
    } else {
        "changing it"
    };
    ErrorValue::new(
        ErrorCode::ChangePrivilegeRequired,
        format!("{purpose} needs {privilege}, which this session does not hold"),
    )
    .with_help(
        "v0.6 §43.3 and §43.4: the plan shows which actions require privilege, and recovery may \
         need more than the mutation did. Nothing was changed"
            .to_owned(),
    )
    .with_metadata("subject", Value::string(subject))
    .with_metadata("privilege", Value::string(privilege))
    .with_metadata("for_recovery", Value::Bool(for_recovery))
}

/// The session does not hold the capability an action or its recovery needs (§43.2, §48.3).
///
/// This is not [`privilege_required`], and the difference is the whole reason both exist: raising
/// operating-system privilege cannot supply a capability. §48.4's boundary is a grant, and a
/// session that was not given one is refused whatever it is running as.
#[must_use]
pub fn capability_missing(subject: &str, capability: &str, for_recovery: bool) -> ErrorValue {
    let purpose = if for_recovery {
        "recovering it"
    } else {
        "changing it"
    };
    ErrorValue::new(
        ErrorCode::ChangeCapabilityMissing,
        format!("{purpose} needs the `{capability}` capability, which this session was not granted"),
    )
    .with_help(
        "v0.6 §43.2 and §48.3: change capabilities and recovery capabilities are granted          separately, and elevation does not supply either. Nothing was changed"
            .to_owned(),
    )
    .with_metadata("subject", Value::string(subject))
    .with_metadata("capability", Value::string(capability))
    .with_metadata("for_recovery", Value::Bool(for_recovery))
}

/// An interrupted plan cannot be resumed in the state it was left in (§41.3).
///
/// `blocked` names one action and the reason resuming it is refused. §41.3 re-establishes the
/// preconditions of what has not run, and an action whose outcome could not be established is not
/// an action to run again — Appendix F.2's uncertainty is exactly what this refusal preserves.
#[must_use]
pub fn resume_refused(plan: &PlanId, blocked: &[(String, String)]) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ChangeResumeRefused,
        format!(
            "plan {} cannot be resumed: {} of its actions cannot be re-established",
            plan.short(),
            blocked.len()
        ),
    )
    .with_help(
        "v0.6 §41.3: resuming re-checks the preconditions of the actions that have not run, and          refuses where one already ran, where its outcome is unknown, or where the world moved.          The plan stays inspectable (§41.2)"
            .to_owned(),
    )
    .with_metadata("plan", Value::string(&plan.to_string()))
    .with_metadata(
        "blocked",
        Value::list(blocked.iter().map(|(action, _)| Value::string(action))),
    )
    .with_metadata(
        "reasons",
        Value::list(
            blocked
                .iter()
                .map(|(action, reason)| Value::string(&format!("{action}: {reason}"))),
        ),
    )
}

/// A recovery provider cannot run here (§12.2, Appendix G.4).
#[must_use]
pub fn provider_unavailable(provider: &str, reason: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryProviderUnavailable,
        format!("the {provider} recovery provider cannot run here"),
    )
    .with_help(format!(
        "v0.6 Appendix G.4: a provider degrades to unavailable rather than executing semantics it \
         has not validated. {reason}"
    ))
    .with_metadata("provider", Value::string(provider))
}

/// A tool a provider needs could not be run (§12.3).
#[must_use]
pub fn tool_failed(program: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryProviderUnavailable,
        format!("`{program}` could not be run"),
    )
    .with_help(detail.to_owned())
    .with_metadata("program", Value::string(program))
}

/// A recovery asset could not be created (§2.3, §55.3 case 13).
#[must_use]
pub fn asset_create_failed(provider: &str, scope: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryAssetCreateFailed,
        format!("{provider} could not create a recovery point for {scope}"),
    )
    .with_help(format!(
        "v0.6 §2.3: mutation MUST NOT begin without the protection the plan required. Nothing was \
         changed. {detail}"
    ))
    .with_metadata("provider", Value::string(provider))
    .with_metadata("scope", Value::string(scope))
}

/// An asset exists and cannot satisfy protection (§11.4).
#[must_use]
pub fn asset_invalid(asset: &RecoveryAssetId, failures: &[&str]) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryAssetInvalid,
        format!("recovery asset {} did not validate", asset.short()),
    )
    .with_help(
        "v0.6 §11.4: an asset is usable only once existence, identity, scope, restore \
         availability and permissions have been checked"
            .to_owned(),
    )
    .with_metadata("asset", Value::string(asset.as_str()))
    .with_metadata(
        "failures",
        Value::list(failures.iter().map(|failure| Value::string(failure))),
    )
}

/// The asset's retention has passed (§37.1).
#[must_use]
pub fn asset_expired(asset: &RecoveryAssetId, expired_at: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryAssetExpired,
        format!("recovery asset {} expired at {expired_at}", asset.short()),
    )
    .with_help(
        "v0.6 §37.1: assets are retained for a bounded window. `get recovery` shows what remains"
            .to_owned(),
    )
    .with_metadata("asset", Value::string(asset.as_str()))
    .with_metadata("expired_at", Value::string(expired_at))
}

/// No asset matches the reference (§37.5).
#[must_use]
pub fn asset_not_found(reference: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryAssetNotFound,
        format!("no recovery asset matches `{reference}`"),
    )
    .with_help(
        "`get recovery` lists the assets this shell knows about, with the plan each belongs to \
         and the scope each covers (v0.6 §37.5)"
            .to_owned(),
    )
    .with_metadata("reference", Value::string(reference))
}

/// An asset does not cover the object it was expected to (§11.2, §13.4).
#[must_use]
pub fn scope_mismatch(asset: &RecoveryAssetId, expected: &str, actual: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryScopeMismatch,
        format!(
            "recovery asset {} does not cover `{expected}`",
            asset.short()
        ),
    )
    .with_help(format!(
        "v0.6 §13.4 and §14.3: a snapshot of a parent dataset or subvolume does not cover a \
         nested one. This asset covers {actual}"
    ))
    .with_metadata("asset", Value::string(asset.as_str()))
    .with_metadata("expected", Value::string(expected))
    .with_metadata("actual", Value::string(actual))
}

/// The plan cannot reach the protection its policy requires (§17.2).
#[must_use]
pub fn coverage_insufficient(
    plan: &PlanId,
    reached: ProtectionLevel,
    required: ProtectionLevel,
    shortfall: &[String],
) -> ErrorValue {
    plan_metadata(
        ErrorValue::new(
            ErrorCode::RecoveryCoverageInsufficient,
            format!(
                "plan {} reaches {reached} protection and its policy requires {required}",
                plan.short()
            ),
        )
        .with_help(
            "v0.6 §17.2: `require` refuses to apply when a required mutation domain cannot reach \
             the plan's protection class. The uncovered domains are in the metadata. Nothing was \
             changed"
                .to_owned(),
        ),
        plan,
    )
    .with_metadata("reached", Value::string(reached.as_str()))
    .with_metadata("required", Value::string(required.as_str()))
    .with_metadata(
        "uncovered_domains",
        Value::list(shortfall.iter().map(|domain| Value::string(domain))),
    )
}

/// The consistency of a captured state could not be established (§39.1).
#[must_use]
pub fn consistency_unknown(scope: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryConsistencyUnknown,
        format!("the consistency of a recovery point over {scope} could not be established"),
    )
    .with_help(format!(
        "v0.6 §39.2: a filesystem snapshot containing an application's files is not \
         application-consistent unless an application-aware provider asserts that. {detail}"
    ))
    .with_metadata("scope", Value::string(scope))
}

/// Recovery would discard state written after the recovery point (§24.2, Appendix C.4).
#[must_use]
pub fn newer_state_conflict(objects: &[String]) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryNewerStateConflict,
        format!(
            "recovery would discard changes made to {} object(s) after the recovery point",
            objects.len()
        ),
    )
    .with_help(
        "v0.6 Appendix C.4: the object the recovery would restore has changed again since. \
         `inspect recovery` shows what each available method would preserve"
            .to_owned(),
    )
    .with_metadata(
        "conflicts",
        Value::list(objects.iter().map(|object| Value::string(object))),
    )
}

/// Recovery would destroy provider-native history that was not accepted (§13.6, §24.5).
#[must_use]
pub fn destructive_history_not_accepted(destroyed: &[String]) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryDestructiveHistoryNotAccepted,
        format!(
            "recovery would destroy {} newer snapshot(s), bookmark(s) or clone(s)",
            destroyed.len()
        ),
    )
    .with_help(
        "v0.6 §13.6: Ono never silently adds destructive flags. Every object that would be \
         destroyed is in the metadata. `--accept-newer-state-loss` accepts it explicitly; nothing \
         has been changed"
            .to_owned(),
    )
    .with_metadata(
        "destroyed",
        Value::list(destroyed.iter().map(|object| Value::string(object))),
    )
}

/// Recovery needs the filesystem offline (§13.7, §14.6).
#[must_use]
pub fn requires_offline(subject: &str, method: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryRequiresOffline,
        format!("recovering {subject} needs it taken offline first"),
    )
    .with_help(format!(
        "v0.6 §13.7: Ono does not promise online rollback merely because a snapshot exists. The \
         method is {method}"
    ))
    .with_metadata("subject", Value::string(subject))
    .with_metadata("method", Value::string(method))
}

/// Recovery needs a reboot (§13.7, §14.6).
#[must_use]
pub fn requires_reboot(subject: &str, method: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryRequiresReboot,
        format!("recovering {subject} takes effect on the next boot"),
    )
    .with_help(format!(
        "v0.6 §14.6: root recovery is a subvolume and boot workflow rather than an in-place \
         operation. The method is {method}"
    ))
    .with_metadata("subject", Value::string(subject))
    .with_metadata("method", Value::string(method))
}

/// A recovery action failed (Appendix F).
#[must_use]
pub fn recovery_apply_failed(action: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryApplyFailed,
        format!("recovery failed at `{action}`"),
    )
    .with_help(format!(
        "v0.6 Appendix F: the remaining assets and the exact partial state are preserved, and \
         nothing claims the state was recovered. {detail}"
    ))
    .with_metadata("action", Value::string(action))
}

/// Recovery verification did not hold (§25.3).
#[must_use]
pub fn recovery_verification_failed(domains: &[String]) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryVerificationFailed,
        "recovery verification did not hold",
    )
    .with_help(
        "v0.6 §25.3: nothing may claim the state was recovered. The metadata reports per \
         equivalence domain — persistent state, runtime state, external side effects"
            .to_owned(),
    )
    .with_metadata(
        "domains",
        Value::list(domains.iter().map(|domain| Value::string(domain))),
    )
}

/// Removing an asset would take away recovery a retained plan still offers (§2.15, §37.2).
#[must_use]
pub fn cleanup_blocked(asset: &RecoveryAssetId, plans: &[String]) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryCleanupBlocked,
        format!(
            "removing recovery asset {} would leave {} plan(s) unrecoverable",
            asset.short(),
            plans.len()
        ),
    )
    .with_help(
        "v0.6 §2.15: recovery assets required by a retained plan MUST NOT be deleted silently, \
         and §37.2 keeps the assets of a failed plan out of ordinary success retention. \
         `remove recovery <id> --confirm` overrides it"
            .to_owned(),
    )
    .with_metadata("asset", Value::string(asset.as_str()))
    .with_metadata(
        "plans",
        Value::list(plans.iter().map(|plan| Value::string(plan))),
    )
}

/// Creating an asset would push storage below the configured floor (Appendix D.3).
#[must_use]
pub fn storage_pressure(scope: &str, free: &str, floor: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryStoragePressure,
        format!("{scope} has {free} free, below the configured floor of {floor}"),
    )
    .with_help(
        "v0.6 Appendix D.3: automatic protection fails closed rather than worsening storage \
         exhaustion. Nothing was changed. `get recovery` shows what is already retained"
            .to_owned(),
    )
    .with_metadata("scope", Value::string(scope))
    .with_metadata("free", Value::string(free))
    .with_metadata("floor", Value::string(floor))
}

/// The asset no longer reflects the state that is about to change (§18.3).
#[must_use]
pub fn asset_stale(asset: &RecoveryAssetId, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryAssetStale,
        format!(
            "recovery asset {} was captured before the state drifted",
            asset.short()
        ),
    )
    .with_help(format!(
        "v0.6 §18.3: Ono does not pretend an asset of an earlier state is a just-before-change \
         recovery point. {detail}"
    ))
    .with_metadata("asset", Value::string(asset.as_str()))
}

/// An application could not be quiesced (§18.4).
#[must_use]
pub fn quiesce_failed(application: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryQuiesceFailed,
        format!("{application} could not be quiesced for an application-consistent recovery point"),
    )
    .with_help(format!(
        "v0.6 §18.4: the quiesce window is bounded and the application is resumed when snapshot \
         creation fails. Nothing was mutated. {detail}"
    ))
    .with_metadata("application", Value::string(application))
}

/// An application was quiesced and could not be resumed (§18.4).
#[must_use]
pub fn resume_failed(application: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryResumeFailed,
        format!("{application} is still quiesced and could not be resumed"),
    )
    .with_help(format!(
        "v0.6 §18.4 makes this a critical error surfaced separately from whatever else happened. \
         {detail}"
    ))
    .with_metadata("application", Value::string(application))
    .with_retryable(true)
}

/// A recovery fact required before destructive recovery could not be established (§56.3).
#[must_use]
pub fn recovery_plan_incomplete(fact: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RecoveryPlanIncomplete,
        format!("{fact} could not be established, so destructive recovery is blocked"),
    )
    .with_help(format!(
        "v0.6 §56.3: where any critical recovery fact cannot be established, destructive recovery \
         is blocked rather than guessed. {detail}"
    ))
    .with_metadata("fact", Value::string(fact))
}

/// A transaction was asked of a provider that does not offer one (§27.1).
#[must_use]
pub fn atomicity_unavailable(provider: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TransactionAtomicityUnavailable,
        format!("{provider} does not offer a transaction"),
    )
    .with_help(
        "v0.6 §27.1: the word is reserved for a provider that can state concrete atomicity and \
         rollback guarantees for its own domain"
            .to_owned(),
    )
    .with_metadata("provider", Value::string(provider))
}

/// A change spanning several providers cannot be atomic (§27.2).
#[must_use]
pub fn cross_provider_not_atomic(providers: &[String]) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TransactionCrossProviderNotAtomic,
        format!(
            "a change spanning {} providers is a ChangeSet, not a transaction",
            providers.len()
        ),
    )
    .with_help(
        "v0.6 §2.11 and §27.2: even where each domain has recovery actions, Ono MUST NOT imply \
         distributed transaction guarantees it does not possess. §27.3 makes generic two-phase \
         commit an explicit non-goal"
            .to_owned(),
    )
    .with_metadata(
        "providers",
        Value::list(providers.iter().map(|provider| Value::string(provider))),
    )
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    fn plan() -> PlanId {
        PlanId::derive(&["p"])
    }

    #[test]
    fn should_carry_the_plan_a_refusal_is_about() {
        let refused = plan_expired(&plan());
        assert_eq!(
            refused
                .field("metadata")
                .and_then(|metadata| match metadata {
                    Value::Map(map) => map.get("plan").cloned(),
                    _ => None,
                }),
            Some(Value::string(plan().as_str())),
            "§45: a refusal a script reads must carry what was refused"
        );
    }

    #[test]
    fn should_say_that_nothing_was_changed_where_nothing_was() {
        for refused in [
            plan_expired(&plan()),
            prepare_failed(&plan(), "snapshot", "the pool is full", &[]),
            drift_detected(&plan(), &[]),
            bulk_guard_failed(&plan(), 40, 10),
        ] {
            let help = refused.help().unwrap_or_default().to_lowercase();
            let message = refused.message().to_lowercase();
            assert!(
                help.contains("nothing was changed") || message.contains("nothing"),
                "Appendix F: an operator must be able to tell a refusal from a partial apply — \
                 got `{help}`"
            );
        }
    }

    #[test]
    fn should_enumerate_what_a_destructive_recovery_would_destroy() {
        let refused = destructive_history_not_accepted(&[
            "tank/data@later-1".to_owned(),
            "tank/data@later-2".to_owned(),
        ]);
        let Some(Value::Map(metadata)) = refused.field("metadata") else {
            panic!("a structured refusal carries metadata");
        };
        let Some(Value::List(destroyed)) = metadata.get("destroyed") else {
            panic!("§13.6 requires the RecoveryPlan to enumerate them");
        };
        assert_eq!(destroyed.len(), 2);
    }

    #[test]
    fn should_report_the_coverage_shortfall_a_require_policy_refused_on() {
        let refused = coverage_insufficient(
            &plan(),
            ProtectionLevel::PartiallyProtected,
            ProtectionLevel::Protected,
            &["application-persistent".to_owned()],
        );
        assert_eq!(refused.code(), ErrorCode::RecoveryCoverageInsufficient);
        let Some(Value::Map(metadata)) = refused.field("metadata") else {
            panic!("a structured refusal carries metadata");
        };
        assert_eq!(
            metadata.get("reached"),
            Some(&Value::string("partially-protected")),
            "§10.3: the plan-level summary must never hide the matrix"
        );
    }

    #[test]
    fn should_keep_every_code_in_the_families_section_forty_five_names() {
        for (error, expected) in [
            (plan_not_found("a82f"), "change.plan_not_found"),
            (asset_not_found("r-1"), "recovery.asset_not_found"),
            (
                atomicity_unavailable("linux.files"),
                "transaction.atomicity_unavailable",
            ),
        ] {
            assert_eq!(error.code().name(), expected);
        }
    }
}
