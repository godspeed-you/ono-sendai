//! `find place` (spec v0.4 §6.8, §9.3, §27.4, §29.4; ADR-0124, ADR-0140, ADR-0141).
//!
//! The command reads its arguments, plans which provider targets can hold an answer, asks those
//! and no others (§34), filters the records by the predicate, and hands the survivors to the
//! provider bridge and the query layer. Nothing about which place is which, or which answers
//! first, is decided here.

use std::sync::Arc;

use jiff::Timestamp;
use ono_command::{CommandImpl, Invocation, Outcome, OutcomeFuture, Scope};
use ono_core::ErrorCode;
use ono_parser::Expr;
use ono_pipeline::ValueStream;
use ono_provider_api::{ProviderRegistry, Query};
use ono_spatial_core::{BootIdentity, PermissionState, SpatialScope, SpatialType};
use ono_spatial_query::discovery::{TargetPlan, root_fields, targets_for};
use ono_spatial_query::{FindRequest, SelectorContext, resolve};
use ono_value::{ErrorValue, RecordValue, Value};

/// `find place` (spec v0.4 §6.8, ADR-0124).
#[derive(Debug)]
pub struct FindPlace {
    id: &'static str,
    /// Where this session's pins are, or `None` where it has no state directory (§46.1).
    pins: Option<crate::spatial::PinStore>,
}

impl FindPlace {
    /// The implementation registered against `ono.place.find`, reading `pins`.
    #[must_use]
    pub fn new(pins: Option<crate::spatial::PinStore>) -> Self {
        Self {
            id: "ono.place.find",
            pins,
        }
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
            let pin_store = self.pins.clone();

            let now = Timestamp::now();
            // §33.1: one index per session. A search that built its own would answer about places
            // the session cannot then enter, and `find place … | take 1 | enter` is exactly the
            // composition §28.2 requires (ADR-0141, superseded here).
            let mut session = crate::spatial::spatial_session().await;

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
                        .filter_map(|record| session.projection_of(record).ok()),
                );
                session.absorb(&records, now);
            }
            if let Some(predicate) = predicate.as_ref() {
                // §6.8: the search reads "the spatial index **and** provider registries". A place
                // this session already holds that no canonical provider serves — an adapted
                // observation of a host whose provider is not there (§37.1, ADR-0193) — is in the
                // index, so the predicate is put to what was last said about it too. It is the
                // same record the object was projected from, not a second reading of the system
                // (§2.16).
                let held: Vec<ono_spatial_core::SpatialId> = session
                    .index()
                    .entries()
                    .map(|entry| entry.object().spatial_id().clone())
                    .filter(|id| !subjects.contains(id))
                    .filter(|id| {
                        session.record_of(id).is_some_and(|record| {
                            let subject = Value::Record(Arc::clone(record));
                            ono_command::evaluate(predicate, &subject, &scope)
                                .is_ok_and(|answer| ono_command::is_true(&answer))
                        })
                    })
                    .collect();
                subjects.extend(held);
                request = request.among(subjects);
            }

            request = request.from_place(session.current_place().clone());
            if let Some(anchor) = anchor {
                // §27.1 gives every spatial command one selector grammar, and a path is one of
                // its spellings: `enter /srv` reaches the filesystem (§15.1, §33.3), so an anchor
                // spelled as a path reaches it here too. Without this the anchor was resolved
                // against the index alone, and a path nobody had looked at yet — or one this user
                // may not read — answered `spatial.not_found` (§35.2, ADR-0198).
                if crate::spatial::storage::looks_like_a_path(&anchor) {
                    crate::spatial::storage::observe_place_at(
                        ctx.providers(),
                        &mut session,
                        std::path::Path::new(&anchor),
                        now,
                    )
                    .await?;
                }
                let index = session.index();
                let found =
                    resolve(index, &anchor, &SelectorContext::anywhere(), now).require(&anchor)?;
                // §35.1: a search inside a scope this user may not read cannot come back empty and
                // complete-looking. The place is legible — that is how it was resolved — and what
                // is inside it is not, so the search says so rather than answering about nothing.
                if let Some((_, _, detail)) = index
                    .withheld(found.spatial_id())
                    .into_iter()
                    .find(|(_, state, _)| *state == PermissionState::PermissionDenied)
                {
                    return Err(ErrorValue::new(
                        ErrorCode::SpatialPermissionDenied,
                        format!("`{anchor}` cannot be searched by this user: {detail}"),
                    )
                    .with_help(
                        "denied is not empty: an answer from here would look complete and be \
                         nothing of the sort (spec v0.4 §35.1, §35.2, §40)",
                    ));
                }
                request = request.near(found.spatial_id().clone());
            }

            // A pin outranks every heuristic (§26.4), and it survives the session (§46.1), so
            // the store is read before anything is ranked. A store that cannot be read is
            // reported rather than treated as "no pins": silently losing what the user chose is
            // worse than failing the search that would have shown it.
            let mut pins = pin_store.map_or_else(
                || Ok(ono_spatial_index::PinRegistry::new()),
                |store| store.load(),
            )?;
            // A pin whose identity is gone but whose selector still finds the place is re-bound
            // for this answer — that is what the resilient selector of §20.4 is for. The store
            // is not rewritten: the pin follows its selector when it is read, never behind the
            // user's back.
            let index = session.index();
            for (name, id) in crate::spatial::pins::resolved_pins(&pins, index, now) {
                pins.rebind(&name, id);
            }
            let places = ono_spatial_query::find_places(index, &request, &pins, now);
            // The place record is the same one `look` and `near` emit (ADR-0140): one contract,
            // one builder, so a search result and a neighbour never read differently.
            let scope = local_scope();
            let values: Vec<Value> = places
                .iter()
                .map(|place| {
                    crate::spatial::view::place_record_of(
                        index,
                        place.spatial_id(),
                        &scope,
                        ono_spatial_core::PermissionState::Available,
                        place.is_pinned(),
                        // ADR-0140: one contract. A place view carries the state and the §24.1
                        // summary the provider last reported, so a search result carries them
                        // too — the same record, read from the same place.
                        session.record_of(place.spatial_id()).map(AsRef::as_ref),
                        None,
                        now,
                    )
                    .map(|record| Value::Record(Arc::new(record)))
                })
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
pub fn spatial_type(value: &Value) -> Result<SpatialType, ErrorValue> {
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
