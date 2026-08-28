//! The spatial commands the shell dispatches (spec v0.4 §6, §45.6).
//!
//! §45.6: "`ono-cli` should parse/dispatch spatial commands and own session current-place state,
//! but SHOULD NOT implement graph selection, identity reconciliation or map layout directly."
//! That is exactly the split here. This module reads the arguments, asks the providers for the
//! objects a query needs and hands the answer on:
//!
//! - which record is which place, and whether two records are one — `ono-spatial-index`'s
//!   provider bridge (§45.2);
//! - which places answer a search, in which order, and what a search is allowed to cost —
//!   `ono-spatial-query` (§45.3).
//!
//! Nothing here decides either. What it does own is the one thing neither of those crates can
//! know: which host and which boot the observation belongs to (§10.2), because that is session
//! state and §46 puts session state in the shell.

use std::sync::Arc;

use jiff::Timestamp;
use ono_command::{CommandImpl, Invocation, Outcome, OutcomeFuture, Scope};
use ono_core::ErrorCode;
use ono_parser::Expr;
use ono_pipeline::ValueStream;
use ono_provider_api::{ProviderRegistry, Query};
use ono_spatial_core::{BootIdentity, Projection, SpatialScope, SpatialType};
use ono_spatial_index::{FreshnessPolicy, ProviderBridge, SpatialIndex};
use ono_spatial_query::discovery::{TargetPlan, root_fields, targets_for};
use ono_spatial_query::{FindRequest, FoundPlace, SelectorContext, resolve};
use ono_value::{ErrorValue, Provenance, RecordValue, SchemaId, Value, builtin_schemas};

/// The schema every place the spatial commands emit satisfies.
const PLACE_SCHEMA: &str = "ono.spatial-place";

/// The provider id the spatial layer signs its own composition with.
///
/// It is not a provider of facts: every field of a place comes from a record a real provider
/// produced, and the record's own provenance travels with it (§2.16, §27.4).
const COMPOSER: &str = "ono.spatial";

/// `find place` (spec v0.4 §6.8, ADR-0124).
#[derive(Debug)]
pub struct FindPlace {
    id: &'static str,
}

impl FindPlace {
    /// The implementation registered against `ono.place.find`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            id: "ono.place.find",
        }
    }
}

impl Default for FindPlace {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandImpl for FindPlace {
    fn id(&self) -> &str {
        self.id
    }

    fn invoke(&self, _ctx: &mut Invocation<'_>) -> Result<Outcome, ErrorValue> {
        Err(ono_command::must_be_awaited(self.id))
    }

    fn invoke_async<'a>(&'a self, ctx: &'a mut Invocation<'_>) -> OutcomeFuture<'a> {
        Box::pin(async move {
            let arguments = ctx.arguments();
            let predicate = arguments.option_expression("where").cloned();
            let object_type = match arguments.option("type") {
                Some(value) => Some(spatial_type(value)?),
                None => None,
            };
            let mut request = FindRequest::new().all(arguments.flag("all"));
            if let Some(text) = arguments.selector("name").and_then(text_of) {
                request = request.matching(text);
            }
            if let Some(object_type) = object_type {
                request = request.of_type(object_type);
            }
            if let Some(Value::Int(limit)) = arguments.option("limit") {
                request = request.limit(usize::try_from(*limit).unwrap_or(usize::MAX));
            }
            let anchor = arguments.option("near").and_then(text_of);
            let scope = Arc::clone(ctx.scope());

            let now = Timestamp::now();
            let mut index = SpatialIndex::new(FreshnessPolicy::recommended());
            let mut bridge = ProviderBridge::new(Projection::new(local_scope(), now));

            let plan = plan_for(ctx.providers(), object_type, predicate.as_ref());
            let mut subjects: Vec<ono_spatial_core::SpatialId> = Vec::new();
            for target in plan.asked() {
                let records =
                    observe(ctx.providers(), target, predicate.as_ref(), &scope, now).await?;
                // Which places these records *are*, before absorbing places they merely mention.
                // A predicate was evaluated against these and against nothing else (§42.3).
                subjects.extend(
                    records
                        .iter()
                        .filter_map(|record| bridge.project(record).ok())
                        .map(|object| object.spatial_id().clone()),
                );
                bridge.absorb(&mut index, &records, now);
            }
            if predicate.is_some() {
                request = request.among(subjects);
            }

            if let Some(anchor) = anchor {
                let found =
                    resolve(&index, &anchor, &SelectorContext::anywhere(), now).require(&anchor)?;
                request = request.near(found.spatial_id().clone());
            }

            let pins = ono_spatial_index::PinRegistry::new();
            let places = ono_spatial_query::find_places(&index, &request, &pins, now);
            let values: Vec<Value> = places
                .iter()
                .map(|place| place_record(&index, place).map(Value::Record))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Outcome::Values(ValueStream::from_values(values)))
        })
    }
}

/// The spatial type a `--type` option names (§3.3, ADR-0124).
///
/// # Errors
///
/// `spatial.unsupported` naming what the geography does know, because a type that names nothing
/// is a question the shell cannot answer rather than a search that finds nothing.
fn spatial_type(value: &Value) -> Result<SpatialType, ErrorValue> {
    let text = text_of(value).unwrap_or_default();
    // The registry spells the types `Process`, `Listener`, `BlockDevice`; a user types
    // `--type process`. Case is not information here, and `near --type <type>` will read the
    // same word (§6.2).
    SpatialType::ALL
        .iter()
        .copied()
        .find(|kind| kind.as_str().eq_ignore_ascii_case(&text))
        .ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::SpatialUnsupported,
                format!("`{text}` is not a spatial type"),
            )
            .with_help(format!(
                "the types are {}",
                SpatialType::ALL
                    .iter()
                    .map(|kind| kind.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
}

fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        other => ono_value::canonical_text(other)
            .ok()
            .filter(|text| !text.is_empty()),
    }
}

/// Which provider targets this search asks (§34, §45.3, ADR-0139).
fn plan_for(
    providers: &ProviderRegistry,
    object_type: Option<SpatialType>,
    predicate: Option<&Expr>,
) -> TargetPlan {
    let fields = root_fields(predicate.into_iter().flat_map(field_paths));
    targets_for(
        object_type,
        &fields,
        &|target| !providers.for_target(target).is_empty(),
        &|target, field| {
            providers
                .for_target(target)
                .iter()
                .flat_map(|provider| provider.schemas())
                .any(|schema| schema.field(field).is_some())
        },
    )
}

/// Every root field path an expression reads.
///
/// `state == "running"` reads `state`; `local.port == 8080` reads `local`, because a nested field
/// belongs to whatever record the root holds and only the root is a field of the schema.
fn field_paths(expression: &Expr) -> Vec<String> {
    let mut found = Vec::new();
    walk(expression, &mut found);
    found
}

fn walk(expression: &Expr, found: &mut Vec<String>) {
    match expression {
        Expr::Path(path) => found.push(path.name.clone()),
        Expr::Field(access) => walk(&access.base, found),
        Expr::Index(index) => {
            walk(&index.base, found);
            walk(&index.index, found);
        }
        Expr::Unary(unary) => walk(&unary.operand, found),
        Expr::Binary(binary) => {
            walk(&binary.lhs, found);
            walk(&binary.rhs, found);
        }
        Expr::Call(call) => {
            walk(&call.callee, found);
            for argument in &call.arguments {
                walk(argument, found);
            }
        }
        Expr::List(list) => {
            for item in &list.items {
                walk(item, found);
            }
        }
        _ => {}
    }
}

/// The records of one target that satisfy the predicate.
///
/// A provider that cannot answer here — no systemd, no container runtime — is not a failure of
/// the search: it is a part of the system that is not present, and the search says nothing about
/// it rather than refusing (§4, §35.2). A predicate that a record cannot be evaluated against is
/// the same: the record does not match, and the search goes on.
async fn observe(
    providers: &ProviderRegistry,
    target: &str,
    predicate: Option<&Expr>,
    scope: &Arc<Scope>,
    now: Timestamp,
) -> Result<Vec<RecordValue>, ErrorValue> {
    let _ = now;
    let Ok(stream) = providers.snapshot(&Query::target(target).for_verb("find")) else {
        return Ok(Vec::new());
    };
    let collected = stream.collect().await;
    let mut records = Vec::new();
    for value in collected.into_values() {
        let Value::Record(record) = value else {
            continue;
        };
        if let Some(predicate) = predicate {
            let subject = Value::Record(Arc::clone(&record));
            match ono_command::evaluate(predicate, &subject, scope) {
                Ok(answer) if ono_command::is_true(&answer) => {}
                _ => continue,
            }
        }
        records.push(RecordValue::clone(&record));
    }
    Ok(records)
}

/// One found place as an `ono.spatial-place/1` record.
fn place_record(index: &SpatialIndex, place: &FoundPlace) -> Result<Arc<RecordValue>, ErrorValue> {
    let id = SchemaId::new(PLACE_SCHEMA, 1);
    let schema = builtin_schemas().get(&id).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::ProviderSchemaViolation,
            "the `ono.spatial-place/1` contract is not in this build",
        )
    })?;
    let entry = index.get(place.spatial_id());
    let capabilities: Vec<Value> = entry
        .map(|entry| {
            entry
                .object()
                .capabilities()
                .iter()
                .map(|capability| Value::string(capability.as_str()))
                .collect()
        })
        .unwrap_or_default();
    let parent = ono_spatial_query::resolve::parent_of(index, place.spatial_id())
        .map_or(Value::Null, |parent| Value::string(&parent.to_string()));
    let tier = place
        .spatial_id()
        .tier()
        .map_or(Value::Null, |tier| Value::string(tier.as_str()));

    let record = RecordValue::builder(schema, Provenance::local(COMPOSER, id))
        .set("spatial_id", Value::string(&place.spatial_id().to_string()))?
        .set("name", Value::string(place.name()))?
        .set("display_name", Value::string(place.name()))?
        .set("object_type", Value::string(place.schema()))?
        .set("spatial_type", Value::string(place.object_type().as_str()))?
        .set("place_path", Value::string(place.place_path()))?
        .set("scope", Value::string(&place.scope().to_string()))?
        .set("parent", parent)?
        .set("freshness", Value::string(place.freshness().as_str()))?
        .set("observed_at", Value::Timestamp(place.observed_at()))?
        .set("identity_tier", tier)?
        .set("capabilities", Value::list(capabilities))?
        .set("pinned", Value::Bool(place.is_pinned()))?
        .set(
            "provenance",
            ono_command::provenance_value(place.provenance()),
        )?
        .build();
    Ok(Arc::new(record))
}

/// The scope every local observation belongs to (§3.2, §10.2).
///
/// §10.2 makes the boot identity part of a process's identity, and no `ono.process/1` record
/// carries one, so the shell reads it: the kernel's own hostname and boot id. Where either cannot
/// be read the scope says so rather than inventing one — an unknown boot is visible, and a
/// [`BootIdentity::unknown_boot`] never compares equal to a known one (§2.17).
#[must_use]
pub fn local_scope() -> SpatialScope {
    let hostname = read_kernel("/proc/sys/kernel/hostname")
        .or_else(|| read_kernel("/etc/hostname"))
        .unwrap_or_else(|| "localhost".to_owned());
    match read_kernel("/proc/sys/kernel/random/boot_id") {
        Some(boot) => SpatialScope::host(&hostname, BootIdentity::new(&hostname, &boot)),
        None => SpatialScope::host(&hostname, BootIdentity::unknown_boot(&hostname)),
    }
}

fn read_kernel(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}
