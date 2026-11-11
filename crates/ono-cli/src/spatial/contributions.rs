//! KUANG/11 spatial contributions (spec v0.4 §36, §35.5, §31.64; v0.2 §31.5, §31.7, §31.17).
//!
//! §36 lets a package extend the spatial world while "Ono core retains control of identity,
//! security and rendering contracts", and §35.5 says exactly where the host stands between the
//! two: "the spatial host MUST filter plugin nodes/edges according to capability scope **before**
//! merging them into maps". So a contribution is a two-step thing here, and both steps are the
//! host's:
//!
//! 1. **Loading.** A package whose manifest declares `contributions.relations` — v0.2 §31.7's
//!    `<from>-><to>` shapes — contributes a relation per shape, named in the namespace §31.5
//!    gives it, *only* when it holds `relation.write`. A package denied the capability loads
//!    degraded and contributes nothing, so there is nothing to filter out of a map later: the
//!    filtering §35.5 asks for happens before the merge because the contribution never exists.
//! 2. **Merging.** `map` asks each contributing package for its edges, resolves both ends through
//!    the canonical provider, and records the edges it could resolve. A package asserts an edge;
//!    it never creates a place. §36.2's "uninspectable phantom edges" cannot arise, because an
//!    end nothing answers to contributes no edge — and §53 settles the rest: a plugin "cannot
//!    create untraceable truth", so every contributed edge carries the package as its provider
//!    and a confidence from §11.5 that the host never raises.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

use jiff::Timestamp;
use ono_kuang_supervisor::LoadedPlugin;
use ono_provider_api::{ProviderRegistry, Query, Selector};
use ono_spatial_core::{Confidence, RelationshipEdge, SpatialId, SpatialType};
use ono_value::{Provenance, RecordValue, SchemaId, Value};

use crate::spatial::session::SpatialSessionState;

/// The capability a package must hold before it may contribute relationship edges.
///
/// `docs/contracts/capabilities.yaml` already declares it: "Contribute relationship edges to the
/// graph." §35.5 makes holding it the condition for the contribution reaching a map at all.
pub const RELATION_WRITE: &str = "relation.write";

/// The core target a relation-contributing command answers for (§36.1).
const RELATION_TARGET: &str = "spatial-relation";

/// The packages that contributed relations this session, by package id.
fn contributors() -> &'static Mutex<BTreeMap<String, Arc<LoadedPlugin>>> {
    static CONTRIBUTORS: OnceLock<Mutex<BTreeMap<String, Arc<LoadedPlugin>>>> = OnceLock::new();
    CONTRIBUTORS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Takes a freshly loaded package's spatial contributions, if it may make any (§35.5, §36.1).
/// `directory` is where the package was loaded from.
///
/// The shapes come from the manifest — `contributions.relations: ["process->process"]` — and the
/// grant from the negotiated contract. A package without the grant is left alone: it is degraded,
/// `load plugin` says which capability it lacks (§31.17), and no relation of its bears its name.
pub fn adopt(id: &str, directory: &std::path::Path, plugin: &Arc<LoadedPlugin>, shapes: &[String]) {
    use ono_spatial_core::relation::{Awaiting, await_shape, endpoint_type, shape_attributes};
    // A package that has just been loaded may contribute the kind of place another package's
    // shape was waiting for (ADR-1008), whatever this package may contribute itself.
    ono_spatial_core::relation::forget_awaiting(id);
    ono_spatial_core::relation::settle();
    // The scope a package's kinds of place sit in is declared on disk beside their schemas
    // (ADR-1009); it needs no capability, because it says where the package's own places are.
    #[cfg(feature = "kuang")]
    crate::spatial::lazy::declare_scopes(id, directory);
    let Some(granted) = plugin.contract().grant(RELATION_WRITE) else {
        return;
    };
    // A grant bounded by `relations` (ADR-0600 §2) covers exactly the ids it names: a shape
    // outside it is not adopted, so the package contributes nothing under that name.
    let allowed: Option<Vec<String>> = granted
        .scope
        .as_ref()
        .and_then(|scope| scope.get("relations"))
        .map(|value| match value {
            serde_json::Value::Array(items) => items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect(),
            serde_json::Value::String(text) => vec![text.clone()],
            _ => Vec::new(),
        });
    let mut contributed = false;
    for shape in shapes {
        let Some((from, to)) = ono_spatial_core::relation::parse_shape(shape) else {
            continue;
        };
        let relation = ono_spatial_core::relation::contributed_id(id, from, to);
        if let Some(allowed) = &allowed
            && !allowed.iter().any(|name| name == &relation || name == "*")
        {
            continue;
        }
        // Checked at load by [`check_shapes`]; a shape that got this far declares them well.
        let attributes = shape_attributes(shape).unwrap_or_default();
        match (endpoint_type(from), endpoint_type(to)) {
            (Some(source), Some(target)) => {
                ono_spatial_core::relation::contribute(
                    ono_spatial_core::relation::shape_spec(
                        id,
                        from,
                        to,
                        source,
                        target,
                        &attributes,
                    ),
                    id,
                );
            }
            // An end in another package's namespace that nobody contributes yet: the relation
            // waits for that package rather than being refused, and `follow` says what it is
            // waiting for (ADR-1008).
            _ if [from, to].iter().all(|end| {
                endpoint_type(end).is_some()
                    || ono_spatial_core::relation::is_foreign_schema(end, id)
            }) =>
            {
                await_shape(Awaiting {
                    package: id.to_owned(),
                    from: from.to_owned(),
                    to: to.to_owned(),
                    attributes,
                });
            }
            _ => continue,
        }
        contributed = true;
    }
    ono_spatial_core::relation::settle();
    if contributed && let Ok(mut registry) = contributors().lock() {
        registry.insert(id.to_owned(), Arc::clone(plugin));
    }
}

/// The refusal for `follow <word>` where `word` names a contributed relation that waits for a
/// kind of place no loaded package contributes (ADR-1008, external-system-provider §14.5).
#[must_use]
pub fn awaiting_refusal(kind: SpatialType, word: &str) -> Option<ono_value::ErrorValue> {
    let shape = ono_spatial_core::relation::awaiting_from(kind, word)?;
    let missing = shape.missing().join("`, `");
    Some(
        ono_value::ErrorValue::new(
            ono_core::ErrorCode::SpatialUnsupported,
            format!(
                "`{word}` is a relation `{}` contributes, and it leads to `{missing}`, which no \
                 loaded package contributes",
                shape.package
            ),
        )
        .with_help(format!(
            "install the package that declares `{missing}` in its `contributions.targets`; the \
             relation is registered the moment that package loads (spec v0.4 §36.1, \
             external-system-provider §14.5)"
        )),
    )
}

/// Forgets a package's contributions — when it is unloaded, and before it is adopted again on a
/// reload: the package is no longer asked for edges, the edges it pushed are dropped (ADR-1014),
/// and the relations it registered and the shapes it left waiting are withdrawn, so a reload
/// registers what its manifest declares now.
pub fn forget(id: &str) {
    if let Ok(mut registry) = contributors().lock() {
        registry.remove(id);
    }
    if let Ok(mut store) = pushed().lock() {
        store.remove(id);
    }
    ono_spatial_core::relation::withdraw(id);
}

// --- edges a package pushes through `relations.contribute` (spec §31.26, ADR-1014) -----------

/// The most edges one package may hold in the session's store at once (ADR-1014).
pub const PUSHED_EDGES: usize = 1024;

/// The most bytes one pushed edge may take, written as the JSON the package sent (ADR-1031).
/// With [`PUSHED_EDGES`] it bounds what one package can make the shell hold, whatever the edges
/// carry in their keys and `metadata`.
pub const PUSHED_EDGE_BYTES: usize = 4096;

/// One edge a package asserted through `relations.contribute`, already validated by the host:
/// it runs along a relation the package itself contributed, between the places the canonical
/// providers find under `source_key` and `target_key` (§36.1, ADR-1014).
#[derive(Debug, Clone, PartialEq)]
pub struct Pushed {
    /// The relation, one the package declared in `contributions.relations`.
    pub relation: &'static ono_spatial_core::relation::RelationSpec,
    /// The source's key under the identity field its kind of place is found by.
    pub source_key: String,
    /// The target's key, the same way.
    pub target_key: String,
    /// What the package claimed, already held below `exact`: the host did not observe it.
    pub confidence: Confidence,
    /// Relationship-specific detail, as the package gave it.
    pub metadata: serde_json::Map<String, serde_json::Value>,
}

impl Pushed {
    fn same_edge(&self, other: &Self) -> bool {
        self.relation.id == other.relation.id
            && self.source_key == other.source_key
            && self.target_key == other.target_key
    }
}

/// The edges packages pushed this session, by package id. Session-scoped: a package that wants
/// its edges in the next session asserts them again when it is loaded there (ADR-1014).
fn pushed() -> &'static Mutex<BTreeMap<String, Vec<Pushed>>> {
    static PUSHED: OnceLock<Mutex<BTreeMap<String, Vec<Pushed>>>> = OnceLock::new();
    PUSHED.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// The relation `id` if `package` contributed it, and only then (§31.5, ADR-1014).
#[must_use]
pub fn relation_of(
    package: &str,
    id: &str,
) -> Option<&'static ono_spatial_core::relation::RelationSpec> {
    ono_spatial_core::relation::contributed_relations()
        .into_iter()
        .find(|entry| entry.origin == package && entry.spec.id == id)
        .map(|entry| entry.spec)
}

/// Keeps `edges` for `package`, all or none. An edge the package asserted before is replaced,
/// not counted twice; a call that would leave the package holding more than [`PUSHED_EDGES`]
/// keeps nothing and says how many the store would have held (ADR-1014).
///
/// # Errors
///
/// The number of edges the package would have held, when that is beyond the bound.
pub fn keep(package: &str, edges: Vec<Pushed>) -> Result<usize, usize> {
    let Ok(mut store) = pushed().lock() else {
        return Err(0);
    };
    let mut held = store.get(package).cloned().unwrap_or_default();
    let accepted = edges.len();
    for edge in edges {
        match held.iter_mut().find(|known| known.same_edge(&edge)) {
            Some(known) => *known = edge,
            None => held.push(edge),
        }
    }
    if held.len() > PUSHED_EDGES {
        return Err(held.len());
    }
    store.insert(package.to_owned(), held);
    Ok(accepted)
}

/// Every edge every package pushed, with the package that pushed it.
#[must_use]
pub fn pushed_edges() -> Vec<(String, Pushed)> {
    pushed().lock().map_or_else(
        |_| Vec::new(),
        |store| {
            store
                .iter()
                .flat_map(|(package, edges)| {
                    edges.iter().map(|edge| (package.clone(), edge.clone()))
                })
                .collect()
        },
    )
}

/// Asks the contributing packages for the edges of the relations in `wanted`, and records the
/// ones both of whose ends the canonical providers answer for (§35.5, §36.1, §2.16).
///
/// `wanted` is relation ids. v0.4 §32.1 lets a default `look` or `map` spend nothing on a relation
/// a package declared beyond `normal` unless the caller asked for it, so a package is asked only
/// for the relations of its own that are wanted — by name, in the `relations` argument — and a
/// package none of whose relations is wanted is not invoked at all (ADR-1020). What a package
/// answers beyond what it was asked for is already paid for, and is kept as any other edge.
///
/// The answer is the edges, with the places they run between already registered — a caller that
/// draws a map adds both to its horizon, and a caller that does not simply gains the edges in the
/// index, where `inspect relation` can explain them (§11.4).
pub async fn merge(
    providers: &ProviderRegistry,
    session: &mut SpatialSessionState,
    wanted: &[&str],
    now: Timestamp,
) -> Vec<(RelationshipEdge, SpatialId, SpatialId)> {
    let packages: Vec<(String, Arc<LoadedPlugin>)> = match contributors().lock() {
        Ok(registry) => registry
            .iter()
            .map(|(id, plugin)| (id.clone(), Arc::clone(plugin)))
            .collect(),
        Err(_) => return Vec::new(),
    };
    let contributed = ono_spatial_core::relation::contributed_relations();
    let mut merged = Vec::new();
    for (id, plugin) in packages {
        let asked: Vec<&str> = contributed
            .iter()
            .filter(|entry| entry.origin == id && wanted.contains(&entry.spec.id))
            .map(|entry| entry.spec.id)
            .collect();
        if asked.is_empty() {
            continue;
        }
        for record in asserted(&plugin, &asked).await {
            if let Some(edge) = resolve(providers, session, &id, &record, now).await {
                merged.push(edge);
            }
        }
    }
    // The edges packages pushed are merged the same way: resolved through the canonical
    // providers here, so one whose end nothing answers to is no edge at all (§36.2, ADR-1014).
    for (package, edge) in pushed_edges() {
        let ends = (
            (edge.relation.source, edge.source_key.as_str()),
            (edge.relation.target, edge.target_key.as_str()),
        );
        if let Some(edge) = edge_between(
            providers,
            session,
            &package,
            edge.relation,
            ends,
            edge.confidence,
            now,
        )
        .await
        {
            merged.push(edge);
        }
    }
    merged
}

/// What one package answers when asked for the edges of the relations `asked` names.
async fn asserted(plugin: &Arc<LoadedPlugin>, asked: &[&str]) -> Vec<RecordValue> {
    let commands: Vec<String> = plugin
        .commands()
        .iter()
        .filter(|registered| registered.contribution.target == RELATION_TARGET)
        .map(|registered| registered.contribution.id.clone())
        .collect();
    let mut arguments = serde_json::Map::new();
    arguments.insert(
        "relations".to_owned(),
        serde_json::Value::Array(
            asked
                .iter()
                .map(|id| serde_json::Value::String((*id).to_owned()))
                .collect(),
        ),
    );
    let mut records = Vec::new();
    for command in commands {
        let Ok(invocation) = plugin.invoke(&command, arguments.clone()).await else {
            continue;
        };
        let (events, _) = invocation.collect().await;
        for event in events {
            if let ono_kuang_supervisor::StreamEvent::Value(Value::Record(record)) = event {
                records.push(RecordValue::clone(&record));
            }
        }
    }
    records
}

/// One asserted edge, with both ends resolved through the canonical provider (§37.1's rule for
/// adapters read from the other side: a contributor names an object, the provider identifies it).
async fn resolve(
    providers: &ProviderRegistry,
    session: &mut SpatialSessionState,
    package: &str,
    record: &RecordValue,
    now: Timestamp,
) -> Option<(RelationshipEdge, SpatialId, SpatialId)> {
    let (source_name, target_name) = (text(record, "source_type")?, text(record, "target_type")?);
    let source_type = type_of(&source_name)?;
    let target_type = type_of(&target_name)?;
    let relation = ono_spatial_core::relation::spec(&ono_spatial_core::relation::contributed_id(
        package,
        &source_name,
        &target_name,
    ))?;
    let claimed = text(record, "confidence")
        .and_then(|name| Confidence::from_name(&name))
        .unwrap_or(Confidence::Unknown);
    let (mut edge, source, target) = edge_between(
        providers,
        session,
        package,
        relation,
        (
            (source_type, &text(record, "source_key")?),
            (target_type, &text(record, "target_key")?),
        ),
        claimed,
        now,
    )
    .await?;
    if let Some(word) = text(record, "relation") {
        edge = edge.with_attribute("provider_relation", Value::string(&word));
    }
    Some((edge, source, target))
}

/// The edge `package` asserts along `relation` between the places the canonical providers find
/// under the two keys, attributed to the package and never exact (§36.2, §31.64, §53).
async fn edge_between(
    providers: &ProviderRegistry,
    session: &mut SpatialSessionState,
    package: &str,
    relation: &'static ono_spatial_core::relation::RelationSpec,
    ((source_type, source_key), (target_type, target_key)): (
        (SpatialType, &str),
        (SpatialType, &str),
    ),
    claimed: Confidence,
    now: Timestamp,
) -> Option<(RelationshipEdge, SpatialId, SpatialId)> {
    let source = place_of(providers, session, source_type, source_key, now).await?;
    let target = place_of(providers, session, target_type, target_key, now).await?;
    if source == target {
        return None;
    }
    // §36.2: an AI- or plugin-derived relationship must never "appear exact without provenance".
    // The host did not observe this edge, so `exact` is not a claim it can pass on however sure
    // the package is; everything weaker travels as the package stated it (§11.5).
    let confidence = held_below_exact(claimed);
    let edge = RelationshipEdge::new(
        source.clone(),
        target.clone(),
        relation.relation_type(),
        confidence,
        // §31.64 and §53: the package that contributed the edge is on the edge. A reader who
        // cannot see where a relationship came from has been handed untraceable truth.
        Provenance::local(package, SchemaId::new("ono.spatial-relation", 1)).observed_at(now),
        now,
    )
    .with_attribute("origin", Value::string(package));
    Some((edge, source, target))
}

/// What a package's claim becomes on an edge the host did not observe: everything but `exact` as
/// stated, and `exact` one step below it (§36.2, §11.5).
#[must_use]
pub fn held_below_exact(claimed: Confidence) -> Confidence {
    match claimed {
        Confidence::Exact => Confidence::Strong,
        other => other,
    }
}

/// The place the canonical provider answers with for the key a package named.
async fn place_of(
    providers: &ProviderRegistry,
    session: &mut SpatialSessionState,
    object_type: SpatialType,
    key: &str,
    now: Timestamp,
) -> Option<SpatialId> {
    let (target, field) = crate::spatial::relations::target_of(object_type)?;
    let value = key
        .parse::<i64>()
        .map_or_else(|_| Value::string(key), |number| Value::Int(number.into()));
    let query = Query::target(target).with(Selector::field(field, value.clone()));
    let collected = providers.snapshot(&query).ok()?.collect().await;
    let records: Vec<RecordValue> = collected
        .into_values()
        .into_iter()
        .filter_map(|value| value.as_record().ok().cloned())
        .filter(|record| record.get(field).is_some_and(|found| *found == value))
        .collect();
    session.absorb(&records, now);
    records
        .iter()
        .find_map(|record| session.projection_of(record).ok())
}

/// The kind of place a contributor named, in either of the two spellings a shape may use.
///
/// v0.2 §31.7 writes a contribution shape as `process->process` and §3.3's own vocabulary spells
/// the type `Process`; a package that declares one and answers with the other means the same
/// thing both times. A schema id names a kind of place the package contributed itself (§36.1),
/// which is resolvable here because [`adopt`] runs after the handshake that registered it.
fn type_of(name: &str) -> Option<SpatialType> {
    ono_spatial_core::relation::endpoint_type(name)
}

/// Whether any relation a package contributed touches this kind of place.
///
/// What it guards is a *cost*: merging asks every contributing package for its edges, which is an
/// invocation per package. A place no contributed relation could reach never pays for one, so an
/// ordinary `look` at a process is exactly as expensive as it was before any package was loaded
/// (§32.1).
#[must_use]
pub fn relates(object_type: SpatialType) -> bool {
    ono_spatial_core::relation::contributed_relations()
        .iter()
        .any(|entry| object_type.is_a(entry.spec.source) || object_type.is_a(entry.spec.target))
}

/// Refuses a package whose relation shapes name a kind of place nobody contributes (§31.7, §36.1).
///
/// This is the whole of the ordering problem ADR-0584 left open, settled in the one place where
/// both halves are readable without running anything: the shapes come from the manifest, and
/// `package_schemas` are the schema ids the package's own `contributions.targets` documents
/// declare — which §31.68 already reads from disk to build registry placeholders. So a shape
/// naming a kind of place that will never exist is `package.invalid` at load, the way every other
/// registration check in `docs/contracts/kuang/contributions.v1.yaml` is, rather than a relation
/// that quietly never appears and a `follow` that says the word means nothing.
///
/// # Errors
///
/// `package.invalid` naming the shape and the endpoint that could not be resolved.
pub fn check_shapes(
    package: &str,
    shapes: &[String],
    package_schemas: &[String],
) -> Result<(), ono_value::ErrorValue> {
    use ono_spatial_core::relation::{parse_shape, shape_attributes, shape_endpoint_is_known};
    for shape in shapes {
        if let Err(problem) = shape_attributes(shape) {
            return Err(ono_value::ErrorValue::new(
                ono_core::ErrorCode::KuangPackageInvalid,
                format!("`{package}` declares the relation shape `{shape}`, and {problem}"),
            )
            .with_help(
                "a shape is `<from>-><to>`, optionally followed by `; name=<word>` and \
                 `; cost=<cheap|normal|expensive|privileged|remote>` (spec §31.26, v0.4 §32.1)",
            ));
        }
        let Some((from, to)) = parse_shape(shape) else {
            return Err(ono_value::ErrorValue::new(
                ono_core::ErrorCode::KuangPackageInvalid,
                format!(
                    "`{package}` declares the relation shape `{shape}`, which is not a \
                     `<from>-><to>` pair (spec §31.7)"
                ),
            )
            .with_help(
                "a shape names the two kinds of place the relation runs between, as \
                 `process->process`",
            ));
        };
        for endpoint in [from, to] {
            // A schema in another package's namespace is that package's to declare, and it may
            // not be installed: the shape is accepted and waits for it (ADR-1008).
            if shape_endpoint_is_known(endpoint, package_schemas)
                || ono_spatial_core::relation::is_foreign_schema(endpoint, package)
            {
                continue;
            }
            return Err(ono_value::ErrorValue::new(
                ono_core::ErrorCode::KuangPackageInvalid,
                format!(
                    "`{package}` declares the relation shape `{shape}`, and `{endpoint}` names no \
                     kind of place: it is not one of the types spec v0.4 §3.3 declares, and no \
                     target this package declares answers with a schema of that id"
                ),
            )
            .with_help(
                "an endpoint is a declared type of `docs/contracts/spatial/spatial.yaml` — \
                 `Process`, `Service`, `File` — or the id of a schema one of this package's \
                 `contributions.targets` declares, such as `dev.example.echo.place/1` \
                 (spec v0.4 §3.3, §36.1)",
            ));
        }
    }
    Ok(())
}

/// Refuses a package whose target declares a spatial parent it cannot reach (ADR-0597).
///
/// `parents` is `(target name, target schema, parent schema)` for every on-disk target that
/// declares one. The parent must be a schema one of the package's own targets declares, and the
/// manifest must declare the shape `<schema>-><parent>` so that an edge can carry the containment.
/// Both halves are on disk, so both are settled before the runtime is spawned, exactly as
/// [`check_shapes`] settles a shape.
///
/// # Errors
///
/// `package.invalid` naming the target and what is missing.
pub fn check_parents(
    package: &str,
    parents: &[(String, String, String)],
    shapes: &[String],
    package_schemas: &[String],
) -> Result<(), ono_value::ErrorValue> {
    for (target, schema, parent) in parents {
        if parent == schema {
            return Err(ono_value::ErrorValue::new(
                ono_core::ErrorCode::KuangPackageInvalid,
                format!(
                    "`{package}` declares the target `{target}` with its own schema `{schema}` as \
                     its parent"
                ),
            )
            .with_help("a kind of place cannot contain itself (spec v0.4 §11.3; ADR-0597)"));
        }
        if !package_schemas.contains(parent) {
            return Err(ono_value::ErrorValue::new(
                ono_core::ErrorCode::KuangPackageInvalid,
                format!(
                    "`{package}` declares the target `{target}` with the parent `{parent}`, and no \
                     target this package declares answers with a schema of that id"
                ),
            )
            .with_help(
                "a parent is the schema id of a kind of place this package contributes itself, so \
                 that `up` lands on one (spec v0.4 §11.3, §36.4; ADR-0597)",
            ));
        }
        let wanted = format!("{schema}->{parent}");
        let declared = shapes.iter().any(|shape| {
            ono_spatial_core::relation::parse_shape(shape)
                .is_some_and(|(from, to)| from == schema && to == parent)
        });
        if !declared {
            return Err(ono_value::ErrorValue::new(
                ono_core::ErrorCode::KuangPackageInvalid,
                format!(
                    "`{package}` declares the target `{target}` with the parent `{parent}`, and \
                     no relation shape `{wanted}` to carry the edge"
                ),
            )
            .with_help(
                "`up` reaches a contributed parent along a contributed relation, so the manifest \
                 declares the shape in `contributions.relations` beside the parent \
                 (spec v0.4 §36.1; ADR-0585, ADR-0597)",
            ));
        }
    }
    Ok(())
}

/// A record's string field, where it has one.
fn text(record: &RecordValue, field: &str) -> Option<String> {
    record
        .get(field)
        .and_then(|value| value.as_str().ok())
        .map(str::to_owned)
}
