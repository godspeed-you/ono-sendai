//! `find place` (spec v0.4 §6.8, §9.3, §27.4, §29.4; ADR-0124, ADR-0140, ADR-0141).
//!
//! The command reads its arguments, plans which provider targets can hold an answer, asks those
//! and no others (§34), filters the records by the predicate, and hands the survivors to the
//! provider bridge and the query layer. Nothing about which place is which, or which answers
//! first, is decided here.

use std::collections::BTreeSet;
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

            let fields = root_fields(predicate.iter().flat_map(field_paths));
            let plan = plan_for(ctx.providers(), object_type, predicate.as_ref());
            // v0.2 §11.3's pre-flight check, in the shape a cross-type search takes it: a field
            // *some* candidate declares narrows the search, and a field *none* of them declares
            // is a word about nothing. Answering the second with an empty stream made a typo in
            // a script indistinguishable from an empty system (ADR-0210).
            let declared = declared_fields(ctx.providers(), &plan, &session);
            if let Some(field) = plan
                .unknown_fields()
                .iter()
                .find(|field| !declared.contains(field.as_str()))
            {
                return Err(undeclared_field(field, &declared));
            }
            let mut subjects: Vec<ono_spatial_core::SpatialId> = Vec::new();
            for target in plan.asked() {
                let records = observe(
                    ctx.providers(),
                    target,
                    predicate.as_ref(),
                    &fields,
                    &scope,
                    now,
                )
                .await?;
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
                let candidates: Vec<ono_spatial_core::SpatialId> = session
                    .index()
                    .entries()
                    .map(|entry| entry.object().spatial_id().clone())
                    .filter(|id| !subjects.contains(id))
                    .collect();
                for id in candidates {
                    let Some(record) = session.record_of(&id).cloned() else {
                        continue;
                    };
                    // The same rule the target plan applies to a provider target: a record whose
                    // schema does not declare a field the predicate reads is not a candidate for
                    // it at all, so it is not asked and cannot fail.
                    if !fields
                        .iter()
                        .all(|field| record.schema().field(field).is_some())
                    {
                        continue;
                    }
                    let subject = Value::Record(record);
                    // §2.17 and §29.3: an evaluation error is an error, not a non-match. A
                    // search that swallowed it would answer `0` for a question it never managed
                    // to ask (ADR-0210).
                    if ono_command::is_true(&ono_command::evaluate(predicate, &subject, &scope)?) {
                        subjects.push(id);
                    }
                }
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

/// Every field name a place this search could reach declares.
///
/// Two sources, because §6.8 gives the search two: the schemas of the provider targets the plan
/// still considers candidates, and the schemas of the records this session's index already holds
/// — a place only a v0.3 adapter observed is in the second and in neither of the first
/// (ADR-0201, ADR-0193).
fn declared_fields(
    providers: &ProviderRegistry,
    plan: &TargetPlan,
    session: &crate::spatial::session::SpatialSessionState,
) -> BTreeSet<String> {
    let mut declared: BTreeSet<String> = BTreeSet::new();
    let mut add = |schema: &ono_value::Schema| {
        declared.extend(
            schema
                .fields()
                .iter()
                .map(|declaration| declaration.name().to_owned()),
        );
    };
    for target in plan.candidates() {
        for provider in providers.for_target(target) {
            for schema in provider.schemas() {
                add(&schema);
            }
        }
    }
    for entry in session.index().entries() {
        if let Some(record) = session.record_of(entry.object().spatial_id()) {
            add(record.schema());
        }
    }
    declared
}

/// The refusal v0.2 §11.3 asks for when a predicate names a field no place this search reaches
/// declares.
///
/// The suggestion comes from the same pool the check consulted, so the answer and the suggestion
/// cannot disagree — `--where memroy > 1` is answered with `memory` (§15.4).
fn undeclared_field(field: &str, declared: &BTreeSet<String>) -> ErrorValue {
    let error = ErrorValue::new(
        ErrorCode::TypeUnknownField,
        format!("unknown field `{field}`: no kind of place this search reaches declares it"),
    );
    match ono_command::closest(field, declared.iter().map(String::as_str)) {
        Some(near) => error.with_help(format!("perhaps: {near}")),
        None => error.with_help(
            "`find place --type <kind>` narrows the search to one kind of place, and `type` on \
             its stream names the fields that kind declares (spec v0.2 §11.3, §15.4)",
        ),
    }
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
/// it rather than refusing (§4, §35.2). A predicate the record cannot be evaluated against is
/// **not** the same: only records whose own schema declares every field the predicate reads are
/// asked at all, so a failure to evaluate is a failure of the question, and §2.17 forbids
/// reporting it as a row that did not match (ADR-0210).
async fn observe(
    providers: &ProviderRegistry,
    target: &str,
    predicate: Option<&Expr>,
    fields: &BTreeSet<String>,
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
            // The plan decides which *targets* can match, and a target's providers may serve more
            // than one schema — `filesystem` is a target whose records are `ono.filesystem/1`
            // *and* `ono.mount/1`, and only the second declares `filesystem`. The record is the
            // honest granularity: one whose schema does not declare a field the predicate reads
            // is not a candidate for it, so it is skipped rather than asked and failed
            // (ADR-0210's rule 2).
            if !fields
                .iter()
                .all(|field| record.schema().field(field).is_some())
            {
                continue;
            }
            let subject = Value::Record(Arc::clone(&record));
            // §2.17 and §29.3: a predicate a record that *does* declare the field cannot be
            // evaluated against is a refusal, not a non-match — `memory > 1` compares a bytesize
            // with an int, and the v0.2 pipeline says so rather than filtering the row away
            // (ADR-0210).
            if !ono_command::is_true(&ono_command::evaluate(predicate, &subject, scope)?) {
                continue;
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
