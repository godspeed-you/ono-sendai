//! `plan` — the command that changes nothing (spec v0.6 §2.1, §5.1, §5.2, §5.3, §6, §62.3).
//!
//! §2.1 is the invariant the whole tranche rests on: *creating or inspecting a change plan MUST
//! NOT mutate the target system*. Everything here reads. The providers are asked what exists, the
//! spatial index is asked what is connected, the recovery providers are asked what they *could*
//! offer, and the result is a sealed object in a store. Not one line of it creates a snapshot,
//! starts a unit or writes a target file, and the view ends with §62.3's `PLAN NOT EXECUTED` so
//! an operator who reads it and walks away knows which world they are in.
//!
//! # Why the shell answers it rather than the command table
//!
//! `plan <mutation>` carries another command inside it, and that command has options of its own:
//! `plan copy file ./nginx.conf /etc/nginx/nginx.conf --overwrite` writes `--overwrite`, which
//! belongs to `copy file`. A registry-bound implementation would bind every argument against
//! `plan`'s own contract and refuse the first inner option with `type.unknown_field`, so `plan`
//! is claimed by the evaluator before the registry path, exactly as `present` is (ADR-0814).
//! `plan`'s own options come first and end at the first word that is not one; everything from
//! there on is the operation, bound against *its* contract — the shape `sudo -u x cmd --flag`
//! and `timeout 5 cmd --flag` already have.

use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{
    ChangePlan, EffectDomain, FrozenTarget, Intent, PlanAction, ProtectionMode, Strategy,
    VerificationClass, VerificationContract, error,
};
use ono_change_impact::derive::ImpactRequest;
use ono_change_impact::risk::{BulkThresholds, RiskRequest};
use ono_change_plan::{BlockPlan, BlockStatement, PlanBuilder, PlanGranularity};
use ono_change_protection::coverage::{CoverageRequest, MutationDomain, mutation_domains};
use ono_command::CommandRegistry;
use ono_core::ErrorCode;
use ono_parser::{Argument, Expr, Stage, StageHead};
use ono_value::{ErrorValue, RecordValue, Value};

use crate::eval::{Eval, Flow};
use crate::session::Session;

use super::actions::{OpaquePermission, Resolution, Statement};
use super::session::{ChangeState, change_session};

/// Whether `stage` is the `plan` command (§5.1, ADR-0814).
///
/// Claimed by the head word alone, exactly as `present` is: `ono:plan` means the same thing, and
/// a program named `plan` on `PATH` stays reachable as `exec:plan`.
#[must_use]
pub fn claims(stage: &Stage) -> bool {
    let StageHead::Command(name) = &stage.head else {
        return false;
    };
    matches!(name.namespace.as_deref(), None | Some("ono")) && name.name == "plan"
}

/// The `ono.change-plan/1` values `plan` produces (§5.1, §5.2, §5.3).
///
/// `input` is what a pipeline in front of it resolved: §5.3 makes those the objects the mutation
/// acts on, frozen once, and §2.6 keeps that membership fixed for the life of the sealed plan.
///
/// # Errors
///
/// `change.historical_context_read_only` from a historical coordinate (§6.4),
/// `change.opaque_action_forbidden` for an external command without §6.3's escape,
/// `change.action_not_plannable` for an operation no contract declares, and whatever the plan
/// store refused with.
pub fn answer(
    session: &mut Session,
    stage: &Stage,
    _source: &str,
    input: &[Value],
) -> Eval<Vec<Value>> {
    plan(session, stage, input).map_err(Flow::Failed)
}

/// The whole of `plan`, as one fallible function.
fn plan(session: &mut Session, stage: &Stage, input: &[Value]) -> Result<Vec<Value>, ErrorValue> {
    // §6.4, before anything else. §2.5 makes historical context read-only, and a plan resolved
    // against the past would be an executable object built on state that is not there any more.
    ono_change_plan::refuse_in_past(
        "plan",
        crate::temporal::session::coordinate().is_historical(),
    )?;
    // §29.1 and §7.1: a target on another host records that host, and this build freezes targets
    // only on the machine the shell runs on. Inside `enter link` a plan would seal this machine's
    // objects while the operator believes they are the linked host's, so it refuses instead.
    if let Some(link) = session
        .context()
        .iter()
        .find(|frame| matches!(frame.kind(), ono_command::FrameKind::Link))
    {
        return Err(ono_change_core::error::action_not_plannable(
            "plan",
            &format!(
                "`plan` inside `enter {}` would freeze this machine's objects and seal them as the \
                 linked host's (§29.1, §7.1). This build does not orchestrate a plan over a link; \
                 leave the link to plan on this machine",
                link.spelling()
            ),
        ));
    }

    // §53's settings reach the change layer through `crate::eval::native::implementations`, and
    // `plan` is dispatched before the command table is built (ADR-0814) — so the reading happens
    // here too. It is idempotent: both call sites write the same resolved configuration.
    super::session::configure_from(session.settings());
    let registry = crate::eval::native::registry()?;
    let (own, rest) = split(&stage.arguments);
    let contract = registry.get("ono.change.plan").ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::ResolveCommandNotFound,
            "`plan` has no contract in this build",
        )
    })?;
    let options = contract.bind(&own)?;
    let statements = statements_of(&rest)?;
    let handle = runtime_handle(session)?;
    let interactive = session.is_interactive();
    let providers = session.providers().clone();
    let configured = configured_strategy(session.settings());
    handle.clone().block_on(async move {
        build(
            &handle,
            registry,
            &providers,
            &options,
            &statements,
            input,
            interactive,
            configured.as_deref(),
            Timestamp::now(),
        )
        .await
    })
}

/// `change.default_strategy` as the operator wrote it (§53), where anything was written.
///
/// Read as text rather than through the change settings, because §28.4's strategies carry their
/// bound — `batch 2`, `parallel 4` — and a kind without its bound is a strategy nobody can run.
/// The environment spelling is honoured exactly as `configure_from` honours it (ADR-0010).
fn configured_strategy(settings: &crate::settings::Settings) -> Option<String> {
    // Only a strategy somebody wrote counts: the built-in `sequential` is what applies when nobody
    // did, and a profile's default (ADR-0836) comes before it.
    settings
        .effective("change.default_strategy")
        .filter(|resolved| resolved.layer != crate::settings::Layer::Default)
        .and_then(|resolved| resolved.value.as_str().ok().map(str::to_owned))
        .or_else(|| {
            std::env::var("ONO_CHANGE_DEFAULT_STRATEGY")
                .ok()
                .filter(|text| !text.trim().is_empty())
        })
}

/// The strategy a profile makes the default, spelled for a plan over `targets` objects (ADR-0836).
///
/// Appendix H.3's `fleet` writes its strategy as `canary 1 then batch 10%`: a canary of fixed size,
/// then batches that are a share of the fleet. §28.4's strategies carry counts, so the share is
/// taken of this plan's frozen targets — never less than one.
fn profile_strategy(targets: usize) -> Option<String> {
    super::session::configured()
        .profile_strategy()
        .and_then(|spelled| spelled_for(spelled, targets))
}

/// `canary <n> then batch <p>%` as `canary <n> <count>`, where it is written that way.
fn spelled_for(spelled: &str, targets: usize) -> Option<String> {
    let words: Vec<&str> = spelled.split_whitespace().collect();
    match words.as_slice() {
        ["canary", canary, "then", "batch", share] => {
            let percent: usize = share.strip_suffix('%')?.parse().ok()?;
            let count = (targets * percent).div_ceil(100).max(1);
            Some(format!("canary {canary} {count}"))
        }
        _ => Some(spelled.to_owned()),
    }
}

/// `plan`'s own options, and the operation that follows them (ADR-0814).
///
/// The split ends at the first word that is not an option, and an option that takes a value takes
/// the word after it. Everything from the first bare word on belongs to the inner command, so an
/// option of the same name on both sides means whichever side it was written on.
fn split(arguments: &[Argument]) -> (Vec<Argument>, Vec<Argument>) {
    let mut own = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        match &arguments[index] {
            Argument::Option(option) => {
                own.push(arguments[index].clone());
                index += 1;
                // A value-taking option of `plan` swallows the next word. The flags do not, so
                // `plan --opaque sh -c …` reads `sh` as the operation rather than as a value.
                if option.value.is_none()
                    && takes_a_value(&option.name)
                    && index < arguments.len()
                    && matches!(
                        &arguments[index],
                        Argument::Word(_) | Argument::Value(Expr::Str(_))
                    )
                {
                    own.push(arguments[index].clone());
                    index += 1;
                }
            }
            _ => break,
        }
    }
    (own, arguments[index..].to_vec())
}

/// Whether one of `plan`'s own options takes a value (`docs/contracts/commands/change.yaml`).
fn takes_a_value(name: &str) -> bool {
    matches!(name, "protection" | "strategy" | "expires")
}

/// The statements this invocation plans: a block's, or the single action's (§5.1, §5.2).
fn statements_of(rest: &[Argument]) -> Result<Vec<Statement>, ErrorValue> {
    if let [Argument::Value(Expr::Block(block))] = rest {
        // §5.2's bound and its four forbidden constructs are the plan crate's rule, applied to
        // exactly what the parser found — so a `for` inside a block is refused by name rather
        // than by this module deciding which construct to mention.
        let mut described = Vec::with_capacity(block.statements.len());
        let mut statements = Vec::with_capacity(block.statements.len());
        for parsed in &block.statements {
            let (described_statement, statement) = describe(parsed);
            described.push(described_statement);
            if let Some(statement) = statement {
                statements.push(statement);
            }
        }
        BlockPlan::of(described)?;
        return Ok(statements);
    }
    let stage = Stage {
        head: StageHead::Command(ono_parser::QualifiedName {
            namespace: None,
            name: head_word(rest)?,
            span: ono_core::Span::new(0, 0),
        }),
        mode: ono_parser::ArgMode::Words,
        arguments: rest.get(1..).unwrap_or_default().to_vec(),
        redirections: Vec::new(),
        span: ono_core::Span::new(0, 0),
    };
    let statement = Statement::of_stage(&stage).ok_or_else(|| {
        error::action_not_plannable(
            "plan",
            "§5.1: `plan <mutation>` needs a mutation to describe — `plan restart service nginx`.",
        )
    })?;
    Ok(vec![statement])
}

/// The head word of the single-action form.
fn head_word(rest: &[Argument]) -> Result<String, ErrorValue> {
    match rest.first() {
        Some(Argument::Word(word)) => Ok(word.text.clone()),
        _ => Err(error::action_not_plannable(
            "plan",
            "§5.1: `plan <mutation>` needs a mutation to describe, and §5.2's block form is \
             `plan { … }`.",
        )),
    }
}

/// One parsed block statement, as §5.2 classifies it and as this module will resolve it.
fn describe(parsed: &ono_parser::Statement) -> (BlockStatement, Option<Statement>) {
    let source = |label: &str| -> Arc<str> { Arc::from(label) };
    match parsed {
        ono_parser::Statement::Pipeline(pipeline) if pipeline.background => (
            BlockStatement::BackgroundJob {
                source: source("a pipeline ending in `&`"),
            },
            None,
        ),
        ono_parser::Statement::Pipeline(pipeline) => {
            let Some(stage) = pipeline.head.stages.first() else {
                return (
                    BlockStatement::Action(ono_change_plan::builder::ActionDescription::new(
                        "",
                        "an empty line",
                    )),
                    None,
                );
            };
            match Statement::of_stage(stage) {
                Some(statement) => (
                    BlockStatement::action(statement.head.clone(), statement.source.clone()),
                    Some(statement),
                ),
                None => (
                    BlockStatement::Action(ono_change_plan::builder::ActionDescription::new(
                        "",
                        "a statement with no command word",
                    )),
                    None,
                ),
            }
        }
        ono_parser::Statement::For(_) | ono_parser::Statement::While(_) => (
            BlockStatement::Loop {
                source: source("a loop"),
            },
            None,
        ),
        ono_parser::Statement::Fn(_) => (
            BlockStatement::Function {
                source: source("a function definition"),
            },
            None,
        ),
        _ => (
            BlockStatement::ControlFlow {
                source: source("a conditional, a branch or a binding"),
            },
            None,
        ),
    }
}

/// The runtime a change command runs on, built on first use like every other native command's.
fn runtime_handle(session: &mut Session) -> Result<tokio::runtime::Handle, ErrorValue> {
    session
        .runtime()
        .map(|runtime| runtime.handle().clone())
        .ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                "the shell could not start the runtime a plan is resolved on",
            )
        })
}

/// Resolves, freezes, analyses, seals and stores the plan (§4.2 to §4.4).
#[allow(clippy::too_many_arguments)]
async fn build(
    handle: &tokio::runtime::Handle,
    registry: &'static CommandRegistry,
    providers: &ono_provider_api::ProviderRegistry,
    options: &ono_command::BoundArguments,
    statements: &[Statement],
    input: &[Value],
    interactive: bool,
    configured_strategy: Option<&str>,
    now: Timestamp,
) -> Result<Vec<Value>, ErrorValue> {
    let state = change_session().await?;
    let opaque = OpaquePermission {
        requested: options.flag("opaque"),
        configured: state.settings().allow_opaque_actions(),
    };
    let mut resolutions = Vec::with_capacity(statements.len());
    for statement in statements {
        match super::actions::resolve(registry, statement, opaque, input) {
            Ok(resolution) => resolutions.push(resolution),
            // §6.3's escape is an explicit request: the refusal above is the default, and only a
            // caller who wrote `--opaque` on a host that permits it gets past it.
            Err(refusal)
                if refusal.code() == ErrorCode::ChangeOpaqueActionForbidden
                    && opaque.is_granted() =>
            {
                resolutions.push(super::actions::resolve_opaque(statement, opaque)?);
            }
            Err(refusal) => return Err(refusal),
        }
    }

    let intent = Intent::new(intent_text(statements), spelling(statements));
    let mut builder = PlanBuilder::for_intent(intent, state.session_id(), now);
    let (targets, resolved) = freeze_all(providers, &state, &resolutions).await?;
    builder = builder.resolve(targets.clone())?;

    let mut ordinal = 0usize;
    // §23.1's contracts are collected rather than pushed as they are found. A `verify` line in a
    // §5.2 block may restate the postcondition the operation already declares, and a check's
    // identity is derived from its subject and its expression — so two of them would share one
    // identity and a result could not be attributed to either. One condition, one contract, and
    // the one that states what it expects wins (ADR-0813).
    let mut contracts: Vec<VerificationContract> = Vec::new();
    let mut stated: Vec<VerificationContract> = Vec::new();
    for resolution in &resolutions {
        match resolution {
            Resolution::Verification {
                subject,
                expression,
            } => {
                stated.push(
                    VerificationContract::new(
                        builder.plan_id(),
                        VerificationClass::Required,
                        subject.clone(),
                        expression.clone(),
                    )
                    .about(ono_change_core::EquivalenceDomain::RuntimeState),
                );
            }
            Resolution::Operation {
                subjects,
                operation,
                arguments,
                object,
                object_selector,
                ..
            } => {
                // A contract may name the object the operation reads by its selector — a copy's
                // `{digest:source}` — so the selector's value travels beside the options.
                let mut named = arguments.clone();
                if let Some(object) = object {
                    named.push((Arc::clone(object_selector), Value::string(object)));
                }
                for subject in subjects {
                    // Every subject was frozen above, so every one has a target. One without is
                    // a plan that would seal without the action the operator asked for, and §4.3
                    // says so rather than sealing it (ADR-0814).
                    let target =
                        target_for(&targets, &resolved, operation, subject).ok_or_else(|| {
                            error::target_unresolved(
                                subject,
                                "§4.3: the subject was frozen, but no frozen target answers for \
                                 it, so the plan would carry no action for it.",
                            )
                        })?;
                    ordinal += 1;
                    let provider = actor_for(providers, resolution);
                    let fragment = super::actions::fragment_for(
                        builder.plan_id(),
                        ordinal,
                        resolution,
                        &provider,
                        target,
                    );
                    builder = builder.contributing(&fragment)?;
                    contracts.extend(super::actions::contracts_for(
                        builder.plan_id(),
                        operation,
                        target,
                        &named,
                    ));
                }
            }
            Resolution::Opaque { description, .. } => {
                ordinal += 1;
                let Some(target) = targets.first() else {
                    continue;
                };
                let fragment = super::actions::fragment_for(
                    builder.plan_id(),
                    ordinal,
                    resolution,
                    "opaque",
                    target,
                );
                builder = builder.contributing(&fragment)?;
                contracts.push(super::actions::opaque_contract(
                    builder.plan_id(),
                    description,
                ));
            }
        }
    }
    // §4.3: a mutation that resolved to no object produced no plan. Sealing one anyway would
    // apply as `PREPARE none / APPLY none / VERIFY none` and change nothing without saying why.
    // A block of `verify` lines alone asks for no mutation, and is not this case.
    let asked_to_mutate = resolutions.iter().any(|resolution| {
        matches!(
            resolution,
            Resolution::Operation { .. } | Resolution::Opaque { .. }
        )
    });
    if asked_to_mutate && ordinal == 0 {
        return Err(error::target_unresolved(
            &intent_text(statements),
            "§4.3 freezes the objects that match now, and this mutation matched none, so there is \
             no action to seal. Nothing was planned.",
        ));
    }
    for contract in stated {
        if !contracts.iter().any(|kept| same_condition(kept, &contract)) {
            contracts.push(contract);
        }
    }
    for contract in contracts {
        builder = builder.verifying(contract);
    }

    // §9: what else the plan reaches, derived from the v0.4 topology the session already holds.
    // §9.3 keeps an inferred edge inferred, which is the index's business and not this module's.
    let spatial = crate::spatial::spatial_session().await;
    let mut impact = ono_change_impact::derive::derive(&ImpactRequest::new(
        spatial.index(),
        builder.targets(),
        builder.actions(),
        now,
    ));
    // §32.2: a recursive deletion's persistence boundaries and size are calculated before the
    // plan is sealed, so they are part of the impact the risk is assessed over.
    let recursive = recursive_removals(&state, &resolutions, &targets, &resolved);
    for removal in &recursive {
        for boundary in &removal.beneath {
            impact.add_boundary(ono_change_core::UnknownBoundary::new(
                removal.path.clone(),
                boundary.mount_point(),
                format!(
                    "§32.3: a `{}` filesystem{} is mounted at `{}` beneath the directory, and it \
                     is a recovery scope of its own — whatever protects `{}` does not reach it",
                    boundary.domain().mount().filesystem(),
                    boundary
                        .size()
                        .map_or_else(String::new, |size| format!(" holding {size}")),
                    boundary.mount_point(),
                    removal.path
                ),
            ));
        }
    }
    let risk = ono_change_impact::risk::assess(
        &RiskRequest::new(builder.actions(), builder.targets())
            .over_impact(&impact)
            .over_topology(spatial.index())
            .with_thresholds(BulkThresholds {
                warn_targets: state.settings().bulk_warn_targets(),
                high_risk_targets: state.settings().bulk_high_risk_targets(),
            }),
    );
    drop(spatial);
    let risk = recursive
        .iter()
        .fold(risk, |risk, removal| risk.with(removal.finding()));
    // §19.4: HIGH and CRITICAL plans need "explicit interactive acknowledgement or non-interactive
    // policy flag", and §53's `change.high_risk_requires_ack` / `critical_risk_requires_ack` are
    // that flag. Switched off, the class is acknowledged by policy in the sealed revision, where
    // §19.4 stores every acknowledgement. Irreversibility is its own gate and is untouched.
    let class = risk.classify();
    let risk = if (class == ono_change_core::RiskClass::High
        && !state.settings().high_risk_requires_ack())
        || (class == ono_change_core::RiskClass::Critical
            && !state.settings().critical_risk_requires_ack())
    {
        risk.risk_accepted()
    } else {
        risk
    };

    let mode = protection_mode(options)?;
    let policy = state.policy(mode);
    // ADR-0815 and ADR-0835: the sealed plan keeps the mode in force, not what was asked for, so
    // a request the configuration raised — or one it could not deliver — is said now or never.
    let notes: Vec<String> = [policy.narrowing_note(), policy.raising_note()]
        .into_iter()
        .flatten()
        .collect();
    if !notes.is_empty() {
        let reporter = crate::report::Reporter::new(ono_render::Presentation::choose(
            std::io::IsTerminal::is_terminal(&std::io::stderr()),
            &[],
        ));
        for note in &notes {
            reporter.note(note);
        }
    }
    let analysis = ono_change_protection::analyse(&coverage_request(&state, &policy, &builder));

    let risk = if options.flag("accept-risk") {
        risk.risk_accepted()
    } else {
        risk
    };
    let risk = if options.flag("accept-irreversible") {
        risk.irreversible_accepted()
    } else {
        risk
    };
    builder = builder
        .with_impact(impact)
        .with_protection(analysis.summary().clone())
        .with_protection_mode(policy.mode())
        .with_risk(risk)
        .with_strategy(strategy_of(
            options,
            configured_strategy.or(profile_strategy(targets.len()).as_deref()),
        )?);
    // §17.1: `prefer` "include[s] protection actions in the plan". They are actions an operator
    // may inspect and refuse before anything runs, exactly like the mutating ones — a coverage
    // matrix says what *could* protect, and this says what *will* be created. They come first,
    // because §4.5 runs them before the first mutation and §2.3 aborts if one of them fails.
    let provisional = builder.plan_id().clone();
    for action in protection_actions(&provisional, analysis.actions()) {
        builder = builder.acting(&action);
    }
    if let Some(expires_at) = expiry(options, now)? {
        builder = builder.expiring_at(expires_at);
    }

    let granularity = if options.flag("one-per-object") {
        PlanGranularity::PlanPerObject
    } else {
        PlanGranularity::SinglePlan
    };
    let plans = builder.seal_with(granularity, now)?;
    // §26.3: "an auto-recovery declaration that does not meet these conditions MUST be rejected
    // at seal time". The check runs on the sealed revision and before anything is stored, so a
    // rejected declaration leaves no plan behind. No recovery plan exists before the plan's
    // protection does, and no policy enables automatic recovery (§26.1), so in this build the
    // answer is the rejection naming every condition that does not hold.
    if options.flag("auto-recover") {
        for plan in &plans {
            ono_change_recovery::admits_auto_recovery(plan, None, false)?;
        }
    }
    let mut values = Vec::with_capacity(plans.len());
    for plan in &plans {
        state.store().put(plan)?;
        super::session::note_last_plan(plan.id());
        record_lifecycle(handle, plan, now);
        values.push(Value::Record(Arc::new(
            ono_change_core::value::plan_record(plan)?,
        )));
    }
    // §17.3: `require` and `maximize` are promises about coverage, and a plan that cannot keep
    // them says so at the moment it is made rather than at the moment it is applied. The plan is
    // stored first, so the operator can inspect the thing that was refused.
    if let Some(plan) = plans.first() {
        policy.enforce(plan.id(), &analysis)?;
    }
    let _ = interactive;
    Ok(values)
}

/// The §17.1 protection actions the plan carries, one per asset the policy will create.
///
/// `Execution::RecoveryOperation` is the shape §12.2 gives them: a provider, the capability being
/// exercised and typed arguments. There is no program and no command line here — §12.3 forbids
/// generating one, and the provider is asked in its own terms.
///
/// The ordinals are provisional: [`PlanBuilder::acting`] renumbers each action onto the end of
/// the plan, so what matters here is the summary the identity is derived from.
fn protection_actions(
    plan: &ono_change_core::PlanId,
    actions: &[ono_change_core::ProtectionAction],
) -> Vec<ono_change_core::PlanAction> {
    actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let asset = action.proposed_asset();
            let execution = ono_change_core::Execution::RecoveryOperation {
                provider: Arc::from(action.provider()),
                capability: Arc::from(ono_change_core::RecoveryCapability::Prepare.as_str()),
                arguments: vec![
                    (Arc::from("domain"), Value::string(asset.scope().domain())),
                    (
                        Arc::from("domain_kind"),
                        Value::string(asset.scope().domain_kind()),
                    ),
                    (
                        Arc::from("asset_type"),
                        Value::string(asset.asset_type().as_str()),
                    ),
                ],
            };
            ono_change_core::PlanAction::new(
                plan,
                index + 1,
                ono_change_core::ActionRole::Prepare,
                action.summary(),
                execution,
            )
            .with_idempotency(ono_change_core::Idempotency::Idempotent)
            .on(asset.scope().domain())
            .recovery_semantics(format!(
                "§17.1: the recovery asset this change is protected by. §2.1 keeps it unmade \
                 until `apply`, and §2.3 stops the mutation if it cannot be created{}",
                if action.is_required() {
                    ""
                } else {
                    ". This one is optional, so a failure to create it does not stop the apply"
                }
            ))
        })
        .collect()
}

/// Whether two contracts ask the same question of the same object (§23.1).
///
/// The comparison is the one a check's identity is derived from, so two contracts this calls
/// equal would be indistinguishable in a result — which is the reason for asking.
fn same_condition(left: &VerificationContract, right: &VerificationContract) -> bool {
    left.subject() == right.subject() && left.expression() == right.expression()
}

/// The coverage request Appendix A runs over: the plan's mutation domains and the persistence
/// domains its file targets resolve to (§10.3, Appendix B).
///
/// The request resolves against the session's mount table (§32.3, Appendix B.1): a recursive
/// removal is then analysed over every filesystem mounted beneath its directory, each a
/// persistence domain of its own, rather than as if the directory's domain held them all.
fn coverage_request<'a>(
    state: &'a ChangeState,
    policy: &'a ono_change_protection::ProtectionPolicy,
    builder: &PlanBuilder,
) -> CoverageRequest<'a> {
    let mut request = CoverageRequest::new(state.providers(), policy).within(state.mounts());
    for mutation in mutation_domains(builder.actions()) {
        request = request.mutating(mutation);
    }
    for target in builder.targets() {
        if target.schema() == ono_change_plan::freeze::FILE_SCHEMA {
            let path = std::path::Path::new(target.label());
            request = request.over(state.mounts().resolve(path));
        }
    }
    request
}

/// One recursive removal a plan carries, measured before it is sealed (§32.2, §32.3).
struct RecursiveRemoval {
    /// The canonical path of the directory.
    path: String,
    /// The filesystems mounted beneath it, each a recovery scope of its own.
    beneath: Vec<ono_change_protection::MountBoundary>,
    /// What the walk beneath it found.
    measured: Measured,
}

impl RecursiveRemoval {
    /// §32.2's size and boundaries, as the plan's risk states them.
    fn finding(&self) -> ono_change_core::RiskFinding {
        let Measured {
            entries,
            bytes,
            complete,
        } = self.measured;
        let size = ono_value::ByteSize::from_bytes(bytes);
        let (entries, size) = if complete {
            (entries.to_string(), size.to_string())
        } else {
            (format!("at least {entries}"), format!("at least {size}"))
        };
        let mounts = if self.beneath.is_empty() {
            "no filesystem is mounted beneath it".to_owned()
        } else {
            let points: Vec<&str> = self
                .beneath
                .iter()
                .map(ono_change_protection::MountBoundary::mount_point)
                .collect();
            format!(
                "{} filesystem(s) are mounted beneath it and are separate recovery scopes \
                 (§32.3): {}",
                points.len(),
                points.join(", ")
            )
        };
        ono_change_core::RiskFinding::new(
            ono_change_core::RiskDimension::Scope,
            ono_change_core::RiskClass::Moderate,
            "risk.scope.multi-object",
            format!(
                "§32.2: the recursive removal of `{}` reaches {entries} entries holding {size} on \
                 its own filesystem; {mounts}",
                self.path
            ),
        )
    }
}

/// What a bounded walk beneath a directory found (§32.2).
#[derive(Debug, Clone, Copy)]
struct Measured {
    entries: u64,
    bytes: u128,
    /// Whether every entry was read. A walk that hit its budget or an unreadable directory
    /// states a lower bound, never a total (§35.3).
    complete: bool,
}

/// How many entries §32.2's walk reads before it states a lower bound instead of a total.
///
/// §52 budgets what planning may cost; a directory with more entries than this is already a
/// deletion whose size is not the interesting part of its risk.
const WALK_BUDGET: u64 = 200_000;

/// The recursive removals among `resolutions`, with their boundaries and size (§32.2).
fn recursive_removals(
    state: &ChangeState,
    resolutions: &[Resolution],
    targets: &[FrozenTarget],
    resolved: &Resolved,
) -> Vec<RecursiveRemoval> {
    let mut removals = Vec::new();
    for resolution in resolutions {
        let Resolution::Operation {
            operation,
            subjects,
            arguments,
            ..
        } = resolution
        else {
            continue;
        };
        let recursive = arguments.iter().any(|(name, value)| {
            name.as_ref() == "recursive" && matches!(value, Value::Bool(true))
        });
        if !recursive || !matches!(operation.command, "ono.dir.remove" | "ono.file.remove") {
            continue;
        }
        for subject in subjects {
            let Some(target) = target_for(targets, resolved, operation, subject) else {
                continue;
            };
            let path = std::path::Path::new(target.label());
            if !path.is_dir() {
                continue;
            }
            if removals
                .iter()
                .any(|kept: &RecursiveRemoval| kept.path == target.label())
            {
                continue;
            }
            removals.push(RecursiveRemoval {
                path: target.label().to_owned(),
                beneath: state
                    .mounts()
                    .boundaries_beneath(path)
                    .into_iter()
                    .map(|boundary| match used_bytes(boundary.mount_point()) {
                        Some(used) => boundary.sized(used),
                        None => boundary,
                    })
                    .collect(),
                measured: measure(path),
            });
        }
    }
    removals
}

/// The bytes in use on the filesystem mounted at `mount_point`, from `statvfs(3)` (§32.2).
///
/// A recursive removal reaches into every filesystem mounted beneath it, so what it would take
/// away there is that filesystem's contents. `None` where the filesystem did not answer: an
/// unknown size is left unknown rather than reported as nothing (§2.4).
fn used_bytes(mount_point: &str) -> Option<ono_value::ByteSize> {
    let stats = nix::sys::statvfs::statvfs(mount_point).ok()?;
    let fragment = u128::from(stats.fragment_size());
    let used = u128::from(stats.blocks()).checked_sub(u128::from(stats.blocks_free()))?;
    Some(ono_value::ByteSize::from_bytes(used.checked_mul(fragment)?))
}

/// The entries and bytes beneath `root` on `root`'s own filesystem (§32.2, §32.3).
///
/// Read with `lstat` and never across a mount: a symlink is one entry of its own size and not the
/// tree it points at, and a filesystem mounted beneath is a boundary the plan names separately
/// rather than bytes this directory holds.
fn measure(root: &std::path::Path) -> Measured {
    use std::os::unix::fs::MetadataExt;
    let mut measured = Measured {
        entries: 0,
        bytes: 0,
        complete: true,
    };
    let Ok(top) = root.symlink_metadata() else {
        measured.complete = false;
        return measured;
    };
    let device = top.dev();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            measured.complete = false;
            continue;
        };
        for entry in entries {
            if measured.entries >= WALK_BUDGET {
                measured.complete = false;
                return measured;
            }
            let Ok(entry) = entry else {
                measured.complete = false;
                continue;
            };
            let Ok(metadata) = entry.path().symlink_metadata() else {
                measured.complete = false;
                continue;
            };
            if metadata.dev() != device {
                continue;
            }
            measured.entries += 1;
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                measured.bytes = measured.bytes.saturating_add(u128::from(metadata.len()));
            }
        }
    }
    measured
}

/// Which frozen identity each `(target word, subject)` was frozen as (§4.3).
///
/// Kept beside the targets because the two are not spelled alike: `/etc/app.conf` written as a
/// symlink freezes the canonical path it resolves to, and a process named by its pid freezes an
/// identity carrying its start time. Matching a subject back by its spelling would miss both.
type Resolved = std::collections::BTreeMap<(&'static str, String), String>;

/// Every subject of every resolution, frozen once (§4.3, §2.6).
async fn freeze_all(
    providers: &ono_provider_api::ProviderRegistry,
    state: &ChangeState,
    resolutions: &[Resolution],
) -> Result<(Vec<FrozenTarget>, Resolved), ErrorValue> {
    let mut targets: Vec<FrozenTarget> = Vec::new();
    let mut resolved = Resolved::new();
    for resolution in resolutions {
        let Resolution::Operation {
            operation,
            subjects,
            ..
        } = resolution
        else {
            continue;
        };
        for subject in subjects {
            // The selector name travels beside the value: §4.3 resolves `process 4211` by `pid`
            // and `service nginx` by `name`, and asking a provider for the wrong field is asking
            // it about nothing. It is the contract's own first selector (ADR-0082 §1).
            let frozen = super::world::freeze(
                providers,
                state.mounts(),
                operation.shape,
                subject,
                operation.subject,
                operation.creates,
            )
            .await?;
            // §4.3 and §7.2: a removal, a move or a permission change declares that its object
            // exists, and a path with nothing at it can never satisfy that. Sealing it would hand
            // `apply` a plan that can only be refused, so it is refused here, by name.
            if operation.requires_existence() && frozen.selector().is_none() {
                return Err(error::target_unresolved(
                    subject,
                    &format!(
                        "§4.3 freezes the objects that match now, and nothing exists there: \
                         `{}` needs the object to exist (§7.2), so the plan could never apply. \
                         Nothing was planned.",
                        operation.command
                    ),
                ));
            }
            resolved.insert(
                (operation.shape.target_word(), subject.clone()),
                frozen.identity().to_owned(),
            );
            if !targets
                .iter()
                .any(|kept| kept.identity() == frozen.identity())
            {
                targets.push(frozen);
            }
        }
    }
    if targets.is_empty() {
        // An opaque action has no object Ono can name, and §6.3 says so out loud: the plan's one
        // target is the host it runs on, and its scope is unknown.
        if resolutions
            .iter()
            .any(|resolution| matches!(resolution, Resolution::Opaque { .. }))
        {
            targets.push(FrozenTarget::new("ono.host/1", "localhost", "localhost"));
        }
    }
    Ok((targets, resolved))
}

/// The frozen target `subject` was frozen as.
fn target_for<'a>(
    targets: &'a [FrozenTarget],
    resolved: &Resolved,
    operation: &super::actions::PlannableOperation,
    subject: &str,
) -> Option<&'a FrozenTarget> {
    let identity = resolved.get(&(operation.shape.target_word(), subject.to_owned()))?;
    targets.iter().find(|target| target.identity() == identity)
}

/// The provider that will carry a resolution out, for §4.4's provider binding.
fn actor_for(providers: &ono_provider_api::ProviderRegistry, resolution: &Resolution) -> String {
    let Resolution::Operation { operation, .. } = resolution else {
        return "opaque".to_owned();
    };
    let target = operation.shape.target_word();
    providers
        .provider_for(target)
        .map_or_else(|_| target.to_owned(), |provider| provider.id().to_owned())
}

/// The intent, as a person reads it (§3.1).
fn intent_text(statements: &[Statement]) -> String {
    let actions: Vec<&str> = statements
        .iter()
        .filter(|statement| !statement.is_verification())
        .map(|statement| statement.source.as_str())
        .collect();
    match actions.len() {
        0 => "verify only".to_owned(),
        1 => actions[0].to_owned(),
        _ => actions.join("; "),
    }
}

/// What the operator actually typed (§3.1).
fn spelling(statements: &[Statement]) -> String {
    let lines: Vec<&str> = statements
        .iter()
        .map(|statement| statement.source.as_str())
        .collect();
    if lines.len() == 1 {
        format!("plan {}", lines[0])
    } else {
        format!("plan {{ {} }}", lines.join("; "))
    }
}

/// `--protection` as §17.2's mode, or `None` for the configured default (§17.1).
fn protection_mode(
    options: &ono_command::BoundArguments,
) -> Result<Option<ProtectionMode>, ErrorValue> {
    let Some(value) = options.option("protection") else {
        return Ok(None);
    };
    let Ok(text) = value.as_str() else {
        return Ok(None);
    };
    ProtectionMode::from_name(text).map(Some).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("`--protection {text}` is not one of §17.2's modes"),
        )
        .with_help("the modes are off, prefer, require and maximize")
    })
}

/// `--strategy` as §28.4's strategy, or `change.default_strategy` where the plan states none (§53).
///
/// Both are read by one parser, so a configured `batch 2` means exactly what `--strategy 'batch
/// 2'` means. A configured strategy that does not parse is refused by name rather than replaced:
/// §53 asks for a configuration nobody can read to be reported, and §28.5 seals the strategy
/// because it changes operational risk — a plan quietly sealed with another one would carry a
/// risk the operator did not configure.
fn strategy_of(
    options: &ono_command::BoundArguments,
    configured: Option<&str>,
) -> Result<Strategy, ErrorValue> {
    let (text, source) = match options.option("strategy") {
        Some(value) => match value.as_str() {
            Ok(text) => (text.to_owned(), format!("`--strategy {text}`")),
            Err(_) => return Ok(Strategy::Sequential),
        },
        None => match configured {
            Some(text) => (
                text.to_owned(),
                format!("`change.default_strategy = {text}`"),
            ),
            None => return Ok(Strategy::Sequential),
        },
    };
    let words: Vec<&str> = text.split_whitespace().collect();
    let refusal = |detail: &str| {
        ErrorValue::new(
            ErrorCode::TypeMismatch,
            format!("{source} is not one of §28.4's strategies"),
        )
        .with_help(detail.to_owned())
    };
    match words.as_slice() {
        ["sequential"] => Ok(Strategy::Sequential),
        ["batch", size] => size
            .parse()
            .ok()
            .and_then(Strategy::batch)
            .ok_or_else(|| refusal("`batch <n>` needs a batch size above zero")),
        ["canary", canary, batch] => canary
            .parse()
            .ok()
            .zip(batch.parse().ok())
            .and_then(|(canary, batch)| Strategy::canary(canary, batch))
            .ok_or_else(|| refusal("`canary <n> <n>` needs two counts above zero")),
        ["parallel", width] => width
            .parse()
            .ok()
            .and_then(Strategy::parallel)
            .ok_or_else(|| {
                refusal("`parallel <n>` needs a bound: §28.4 does not offer unlimited parallelism")
            }),
        _ => Err(refusal(
            "the strategies are `sequential`, `batch <n>`, `canary <n> <n>` and `parallel <n>`",
        )),
    }
}

/// `--expires` as the instant the sealed plan stops being appliable (§4.1, §5.6).
fn expiry(
    options: &ono_command::BoundArguments,
    now: Timestamp,
) -> Result<Option<Timestamp>, ErrorValue> {
    let Some(Value::Duration(span)) = options.option("expires") else {
        return Ok(None);
    };
    Timestamp::from_nanosecond(now.as_nanosecond().saturating_add(span.nanoseconds()))
        .map(Some)
        .map_err(|_| {
            ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`--expires {span}` reaches outside representable time"),
            )
        })
}

/// Writes §22.1's `PlanCreated` and `PlanSealed` into the v0.5 ledger (§22.1, §22.4).
///
/// Recording never fails the command: a ledger that refused an append is a reason to lose
/// history, and §16.5 does not make it a reason to lose the plan.
fn record_lifecycle(_handle: &tokio::runtime::Handle, plan: &ChangePlan, now: Timestamp) {
    let scope = crate::spatial::local_scope();
    let mut lifecycle = ono_change_executor::PlanLifecycle::created(plan, scope, now);
    lifecycle.sealed(plan.digest(), now);
    let ledger = crate::temporal::session::writable_ledger();
    let _ = lifecycle.record(ledger.as_ref());
}

/// The mutation domains a plan's actions declare, for a caller outside this module (§10.3).
#[must_use]
pub fn domains_of(actions: &[PlanAction]) -> Vec<MutationDomain> {
    mutation_domains(actions)
}

/// Whether a plan's protection summary leaves a persistent domain uncovered (Appendix A.5).
#[must_use]
pub fn uncovered_persistent(plan: &ChangePlan) -> Vec<EffectDomain> {
    plan.protection()
        .shortfall()
        .iter()
        .map(|row| row.domain())
        .filter(|domain| domain.is_persistent())
        .collect()
}

/// The `ono.change-plan/1` record of `plan` (§46.1).
///
/// # Errors
///
/// Whatever the schema refused the record for.
pub fn record_of(plan: &ChangePlan) -> Result<RecordValue, ErrorValue> {
    ono_change_core::value::plan_record(plan)
}

#[cfg(test)]
mod profile_strategy_tests {
    #[test]
    fn should_take_a_profiles_batch_share_of_the_frozen_targets() {
        assert_eq!(
            super::spelled_for("canary 1 then batch 10%", 40).as_deref(),
            Some("canary 1 4")
        );
        assert_eq!(
            super::spelled_for("canary 1 then batch 10%", 3).as_deref(),
            Some("canary 1 1"),
            "a share of a small fleet is still at least one"
        );
    }
}
