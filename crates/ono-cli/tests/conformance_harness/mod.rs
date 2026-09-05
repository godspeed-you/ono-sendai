//! What the generated conformance suite of spec §35.3 actually asks the providers.
//!
//! `crates/ono-cli/tests/provider_conformance.rs` is generated from `docs/contracts/providers/*.yaml`
//! and `docs/contracts/schemas/*.v1.yaml`; it carries the declarations and nothing else. The questions
//! live here, written once and asked of every provider: what it advertises, the shape of every
//! record it emits, whether its identity identifies, and whether a target it serves answers the
//! way the declaration says it does.
//!
//! ADR-0331 records why the split is this way round: a generated file that also carried the
//! assertions would be a file nobody reads, and the assertions are the part a human has to be
//! able to argue with.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    dead_code,
    reason = "a shared test harness states its preconditions the way a test does (AGENTS.md §16)"
)]

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use ono_pipeline::StreamEvent;
use ono_provider_api::{Provider, ProviderRegistry, Query};
use ono_value::Value;

/// How long a bounded snapshot has to finish before the provider is considered hung.
const DEADLINE: Duration = Duration::from_secs(20);

/// How many values an unbounded stream is read for before the case is satisfied.
const UNBOUNDED_SAMPLE: usize = 4;

/// One capability, as `docs/contracts/capabilities.yaml` defines it.
pub struct CapabilityClaim {
    /// The capability id.
    pub id: &'static str,
    /// `read`, `observe`, `mutate` or `destructive`.
    pub risk: &'static str,
    /// `none`, `conditional` or `required`.
    pub elevation: &'static str,
}

/// What one provider entry advertises.
pub struct Surface {
    /// The provider id.
    pub provider: &'static str,
    /// The targets it answers about.
    pub targets: &'static [&'static str],
    /// The capabilities it claims.
    pub capabilities: &'static [CapabilityClaim],
    /// The schemas it emits.
    pub schemas: &'static [&'static str],
    /// The token its records give for a `provider` identity field, when its target is one two
    /// providers can claim (ADR-0559).
    pub identity_token: Option<&'static str>,
}

/// One field, as `docs/contracts/schemas/*.v1.yaml` fixes it.
pub struct FieldContract {
    /// The field name.
    pub name: &'static str,
    /// The type name, as `ono_value::FieldType::name` renders it.
    pub ty: &'static str,
    /// Whether a record must carry it.
    pub required: bool,
    /// Whether it may be null.
    pub nullable: bool,
    /// The unit it is measured in, when it has one.
    pub unit: Option<&'static str>,
}

/// The shape of one schema a provider emits.
pub struct SchemaContract {
    /// The provider that emits it.
    pub provider: &'static str,
    /// The targets that provider entry serves, which is what tells a repeated id apart.
    pub targets: &'static [&'static str],
    /// The schema id.
    pub schema: &'static str,
    /// The fields that identify one object.
    pub identity: &'static [&'static str],
    /// The fields that join the identity when the declared one is incomplete (ADR-0553).
    pub identity_fallback: &'static [&'static str],
    /// The columns a table shows by default.
    pub default_view: &'static [&'static str],
    /// Every field, in the order the registry declares them.
    pub fields: &'static [FieldContract],
}

/// How a bare snapshot of a target behaves, as the declaration says it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exercise {
    /// The snapshot ends and yields records — possibly none.
    Enumerable,
    /// The snapshot must refuse: the provider cannot answer without an argument.
    SelectorRequired,
    /// The snapshot is a live stream that does not end on its own.
    Unbounded,
}

/// One target a provider serves.
pub struct TargetCase {
    /// The provider that serves it.
    pub provider: &'static str,
    /// Every target that provider entry serves.
    pub targets: &'static [&'static str],
    /// The target name.
    pub target: &'static str,
    /// What a bare snapshot must do.
    pub exercise: Exercise,
    /// The schemas the provider is allowed to emit.
    pub schemas: &'static [&'static str],
    /// The identity strategy the provider claims, when it makes a spatial claim at all.
    pub identity_strategy: Option<&'static str>,
}

/// How a declared capability is exercised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Through {
    /// By the snapshot case of the target it reads.
    Snapshot(&'static str),
    /// By the commands a user types to reach it.
    Command(&'static [&'static str]),
}

/// One declared capability, and what reaches it.
pub struct Account {
    /// The provider that declares it.
    pub provider: &'static str,
    /// Every target that provider entry serves.
    pub targets: &'static [&'static str],
    /// The capability id.
    pub capability: &'static str,
    /// Its risk.
    pub risk: &'static str,
    /// What exercises it.
    pub through: Through,
}

/// The registry the shell itself builds, including the providers reached asynchronously.
pub async fn registry() -> ProviderRegistry {
    let mut registry = ono_cli::providers::registry(std::iter::empty());
    ono_cli::providers::register_async(&mut registry).await;
    registry
}

/// The registered provider with this id serving these targets.
fn entry<'a>(registry: &'a ProviderRegistry, id: &str, targets: &[&str]) -> &'a Arc<dyn Provider> {
    registry
        .providers()
        .iter()
        .find(|provider| provider.id() == id && provider.targets() == targets)
        .unwrap_or_else(|| {
            panic!(
                "docs/contracts/providers/ declares `{id}` serving {targets:?}, and no such provider \
                 is registered"
            )
        })
}

/// Every provider the declarations name is registered, and no other one is.
pub async fn assert_registry(declared: &[(&str, &[&str])]) {
    let registry = registry().await;
    let mut advertised: Vec<(String, Vec<String>)> = registry
        .providers()
        .iter()
        .map(|provider| {
            (
                provider.id().to_owned(),
                provider.targets().iter().map(|t| (*t).to_owned()).collect(),
            )
        })
        .collect();
    advertised.sort();
    let mut expected: Vec<(String, Vec<String>)> = declared
        .iter()
        .map(|(id, targets)| {
            (
                (*id).to_owned(),
                targets.iter().map(|t| (*t).to_owned()).collect(),
            )
        })
        .collect();
    expected.sort();
    assert_eq!(
        advertised, expected,
        "docs/contracts/providers/*.yaml and the built registry must name the same providers: one the \
         code registers and no file declares is undocumented surface, one a file declares and \
         nothing registers is a promise nobody keeps (spec §35.3)"
    );
}

/// The provider advertises exactly the surface its declaration promises.
pub async fn assert_surface(surface: &Surface) {
    let registry = registry().await;
    let provider = entry(&registry, surface.provider, surface.targets);

    let mut advertised: Vec<(String, String, bool)> = provider
        .capabilities()
        .iter()
        .map(|capability| {
            (
                capability.id().to_owned(),
                capability.risk().as_str().to_owned(),
                capability.needs_elevation(),
            )
        })
        .collect();
    advertised.sort();
    // `Capability::needs_elevation` is a boolean, and `elevation` has three values: a capability
    // that is privileged for some targets and not for others works unprivileged for the ones it
    // works for, so only `required` is elevation the shell has to hold up front.
    let mut declared: Vec<(String, String, bool)> = surface
        .capabilities
        .iter()
        .map(|claim| {
            (
                claim.id.to_owned(),
                claim.risk.to_owned(),
                claim.elevation == "required",
            )
        })
        .collect();
    declared.sort();
    assert_eq!(
        advertised, declared,
        "`{}` must advertise the capabilities its declaration promises, at the risk and \
         elevation docs/contracts/capabilities.yaml fixes",
        surface.provider
    );

    assert_eq!(
        provider.identity_token(),
        surface.identity_token,
        "`{}` must answer for the identity token its declaration names, or an action on a record \
         it made would be performed by whichever provider registered first (ADR-0559)",
        surface.provider
    );

    let mut emitted: Vec<String> = provider
        .schemas()
        .iter()
        .map(|schema| schema.id().to_string())
        .collect();
    emitted.sort();
    let mut promised: Vec<String> = surface.schemas.iter().map(|id| (*id).to_owned()).collect();
    promised.sort();
    assert_eq!(
        emitted, promised,
        "`{}` must carry the schemas its declaration promises",
        surface.provider
    );
}

/// The schema the provider carries is field-for-field the one the registry fixes.
pub async fn assert_schema_contract(contract: &SchemaContract) {
    let registry = registry().await;
    let provider = entry(&registry, contract.provider, contract.targets);
    let schema = provider
        .schemas()
        .into_iter()
        .find(|schema| schema.id().to_string() == contract.schema)
        .unwrap_or_else(|| {
            panic!(
                "`{}` declares `{}` and does not carry it",
                contract.provider, contract.schema
            )
        });

    let declared: Vec<(String, String, bool, bool, Option<String>)> = schema
        .fields()
        .iter()
        .map(|field| {
            (
                field.name().to_owned(),
                field.ty().name(),
                field.is_required(),
                field.is_nullable(),
                field.unit().map(|unit| unit.as_str().to_owned()),
            )
        })
        .collect();
    let wanted: Vec<(String, String, bool, bool, Option<String>)> = contract
        .fields
        .iter()
        .map(|field| {
            (
                field.name.to_owned(),
                field.ty.to_owned(),
                field.required,
                field.nullable,
                field.unit.map(str::to_owned),
            )
        })
        .collect();
    assert_eq!(
        declared, wanted,
        "{} must declare exactly the fields, types, nullability and units of its contract",
        contract.schema
    );

    let identity: Vec<&str> = schema.identity().iter().map(|name| &**name).collect();
    assert_eq!(
        identity, contract.identity,
        "{} identifies an object by the fields its contract names",
        contract.schema
    );
    let fallback: Vec<&str> = schema
        .identity_fallback()
        .iter()
        .map(|name| &**name)
        .collect();
    assert_eq!(
        fallback, contract.identity_fallback,
        "{} falls back to the fields its contract names when its declared identity is \
         incomplete",
        contract.schema
    );
    let view: Vec<&str> = schema.default_view().iter().map(|name| &**name).collect();
    assert_eq!(
        view, contract.default_view,
        "{} shows the columns its contract names",
        contract.schema
    );
}

/// A bare snapshot of the target behaves the way the declaration says it does.
pub async fn assert_target_conforms(case: &TargetCase) {
    let registry = registry().await;
    let provider = entry(&registry, case.provider, case.targets);

    // A provider that cannot answer here says why. That is a different answer from an empty one,
    // and spec §35.3 requires the difference to be visible rather than fabricated away.
    if let Some(reason) = provider.availability().reason() {
        assert!(
            !reason.trim().is_empty(),
            "`{}` reports itself unavailable and must say why",
            case.provider
        );
        return;
    }

    let query = Query::target(case.target).limit(16);
    let outcome = provider.snapshot(&query);

    if case.exercise == Exercise::SelectorRequired {
        let error = outcome.err().unwrap_or_else(|| {
            panic!(
                "`{}` declares that `{}` cannot be answered without a selector, so a bare \
                 snapshot must refuse rather than answer (spec §35.3)",
                case.provider, case.target
            )
        });
        assert!(
            !error.message().trim().is_empty(),
            "the refusal of `{} {}` must say what is missing",
            case.provider,
            case.target
        );
        return;
    }

    let stream = outcome.unwrap_or_else(|error| {
        panic!(
            "`{}` declares `{}` answerable and refused with {}: {}",
            case.provider,
            case.target,
            error.code(),
            error.message()
        )
    });

    let mut values: Vec<Value> = Vec::new();
    match case.exercise {
        Exercise::Enumerable => {
            let collected = tokio::time::timeout(DEADLINE, stream.collect())
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "`{}` declares `{}` enumerable, so its snapshot must end",
                        case.provider, case.target
                    )
                });
            for error in collected.errors() {
                assert!(
                    !error.message().trim().is_empty(),
                    "a per-item failure from `{}` must say what could not be read (spec §16.5)",
                    case.provider
                );
            }
            values.extend(collected.into_values());
        }
        Exercise::Unbounded | Exercise::SelectorRequired => {
            let mut stream = stream;
            while values.len() < UNBOUNDED_SAMPLE {
                match tokio::time::timeout(DEADLINE, stream.recv()).await {
                    Ok(Some(StreamEvent::Value(value))) => values.push(value),
                    Ok(Some(StreamEvent::Failure(error))) => assert!(
                        !error.message().trim().is_empty(),
                        "a failure from `{}` must say what could not be read",
                        case.provider
                    ),
                    // An unbounded stream is allowed to end and allowed to stay quiet; what it
                    // may not do is emit something that breaks its contract.
                    Ok(None) | Err(_) => break,
                }
            }
        }
    }

    assert_records_conform(case, &values);
}

/// Every record a provider emitted satisfies the contract it claims.
fn assert_records_conform(case: &TargetCase, values: &[Value]) {
    let mut identities: BTreeSet<String> = BTreeSet::new();
    for value in values {
        let Value::Record(record) = value else {
            panic!(
                "`{}` answered `{}` with {} rather than a record",
                case.provider,
                case.target,
                value.type_name()
            );
        };
        let id = record.schema_id().to_string();
        assert!(
            case.schemas.contains(&id.as_str()),
            "`{}` emitted a `{id}` for `{}`, and its declaration promises only {:?}",
            case.provider,
            case.target,
            case.schemas
        );
        record.validate().unwrap_or_else(|error| {
            panic!(
                "`{}` emitted a record that does not satisfy `{id}`: {}. Unknown data is null, \
                 never fabricated (spec §35.3)",
                case.provider,
                error.message()
            )
        });

        // Identity is the first thing spec §35.3 exercises, and spec v0.4 §42.1 turns it into a
        // requirement: repeated observations of one live object must resolve to the same id. Two
        // things follow that a snapshot can check. Every field the identity is made of has to be
        // carried, or the object cannot be found again at all; and two objects whose identities
        // are *fully known* may not share one, because an identity that does not distinguish
        // cannot resolve anything. An identity with a null component is exempt from the second:
        // the provider is saying it does not know, which spec §35.3 requires it to say rather
        // than to invent.
        if case.identity_strategy.is_some() {
            for field in record.schema().identity() {
                assert!(
                    record.get(field).is_some(),
                    "`{}` claims the `{}` identity strategy for `{id}`, so every record must \
                     carry `{field}`",
                    case.provider,
                    case.identity_strategy.unwrap_or_default()
                );
            }
            // The identity a record actually carries, which for a schema with a fallback is
            // wider than the declared one (ADR-0553).
            let complete = record
                .schema()
                .identity_for(record)
                .iter()
                .all(|field| !matches!(record.get(field), None | Some(Value::Null)));
            if complete {
                let identity = format!("{:?}", record.identity());
                assert!(
                    identities.insert(identity.clone()),
                    "`{}` answered `{}` with two objects sharing the identity {identity}; an \
                     identity that does not distinguish cannot resolve one object twice \
                     (spec §35.3, v0.4 §42.1)",
                    case.provider,
                    case.target
                );
            }
        }
    }
}

/// Every declared capability is reached by something a test runs, or by a command a user types.
///
/// The generator refuses to emit an account it cannot fill, so this list is complete by
/// construction: what is asserted here is that each account is *true*. A read capability is
/// exercised by the snapshot case of the target it reads. One that changes the world is not —
/// running it would change the machine the suite runs on — so it is accounted for by the command
/// that reaches it, and the command has to exist in the registry and ask for this very
/// capability. What binds that command is spec §27.2's business, and `cargo xtask spec-check`
/// is where it is checked.
pub async fn assert_accounts(accounts: &[Account]) {
    let registry = registry().await;
    let commands = ono_command::CommandRegistry::embedded()
        .expect("the embedded command contracts must parse");

    for account in accounts {
        let provider = entry(&registry, account.provider, account.targets);
        match account.through {
            Through::Snapshot(target) => assert!(
                provider.targets().contains(&target),
                "`{}` is accounted for by the snapshot of `{target}`, which `{}` does not serve",
                account.capability,
                account.provider
            ),
            Through::Command(ids) => {
                let reaching: Vec<&&str> = ids
                    .iter()
                    .filter(|id| {
                        commands
                            .get(id)
                            .and_then(ono_command::CommandContract::provider_capability)
                            == Some(account.capability)
                    })
                    .collect();
                assert!(
                    !reaching.is_empty(),
                    "`{}` is accounted for by {ids:?}, and no command of that name asks the \
                     registry for it, so nothing a user can type reaches the capability \
                     (spec §27.2, §35.3)",
                    account.capability
                );
            }
        }
    }
}
