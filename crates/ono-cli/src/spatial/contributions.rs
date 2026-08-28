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
use ono_spatial_core::{
    Confidence, ConfidenceClaim, CostClass, Direction, RelationSpec, RelationshipEdge, SpatialId,
    SpatialType,
};
use ono_value::{Provenance, RecordValue, SchemaId, Value};

use crate::spatial::session::SpatialSessionState;

/// The capability a package must hold before it may contribute relationship edges.
///
/// `docs/spec/capabilities.yaml` already declares it: "Contribute relationship edges to the
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
///
/// The shapes come from the manifest — `contributions.relations: ["process->process"]` — and the
/// grant from the negotiated contract. A package without the grant is left alone: it is degraded,
/// `load plugin` says which capability it lacks (§31.17), and no relation of its bears its name.
pub fn adopt(id: &str, plugin: &Arc<LoadedPlugin>, shapes: &[String]) {
    if plugin.contract().grant(RELATION_WRITE).is_none() {
        return;
    }
    let mut contributed = false;
    for shape in shapes {
        let Some((from, to)) = shape.split_once("->") else {
            continue;
        };
        let (Some(source), Some(target)) = (type_of(from.trim()), type_of(to.trim())) else {
            continue;
        };
        let relation = relation_id(id, source, target);
        let leak = |text: String| -> &'static str { Box::leak(text.into_boxed_str()) };
        ono_spatial_core::relation::contribute(
            RelationSpec {
                id: leak(relation.clone()),
                source,
                target,
                direction: Direction::Outbound,
                canonical_label: leak(relation.clone()),
                inverse_label: leak(format!("{relation}.inverse")),
                canonical_group: leak(relation.clone()),
                inverse_group: leak(format!("{relation}.inverse")),
                // §36.2: a contributed edge may say how sure the package is, and the host never
                // raises it — the host did not observe the edge.
                confidence: ConfidenceClaim::ProviderDeclared,
                cost_class: CostClass::Normal,
            },
            id,
        );
        contributed = true;
    }
    if contributed && let Ok(mut registry) = contributors().lock() {
        registry.insert(id.to_owned(), Arc::clone(plugin));
    }
}

/// Forgets a package's contributions when it is unloaded.
pub fn forget(id: &str) {
    if let Ok(mut registry) = contributors().lock() {
        registry.remove(id);
    }
}

/// Asks every contributing package for its edges and records the ones both of whose ends the
/// canonical providers answer for (§35.5, §36.1, §2.16).
///
/// The answer is the edges, with the places they run between already registered — a caller that
/// draws a map adds both to its horizon, and a caller that does not simply gains the edges in the
/// index, where `inspect relation` can explain them (§11.4).
pub async fn merge(
    providers: &ProviderRegistry,
    session: &mut SpatialSessionState,
    now: Timestamp,
) -> Vec<(RelationshipEdge, SpatialId, SpatialId)> {
    let packages: Vec<(String, Arc<LoadedPlugin>)> = match contributors().lock() {
        Ok(registry) => registry
            .iter()
            .map(|(id, plugin)| (id.clone(), Arc::clone(plugin)))
            .collect(),
        Err(_) => return Vec::new(),
    };
    let mut merged = Vec::new();
    for (id, plugin) in packages {
        for record in asserted(&plugin).await {
            if let Some(edge) = resolve(providers, session, &id, &record, now).await {
                merged.push(edge);
            }
        }
    }
    merged
}

/// What one package answers when asked for the edges it contributes.
async fn asserted(plugin: &Arc<LoadedPlugin>) -> Vec<RecordValue> {
    let commands: Vec<String> = plugin
        .commands()
        .iter()
        .filter(|registered| registered.contribution.target == RELATION_TARGET)
        .map(|registered| registered.contribution.id.clone())
        .collect();
    let mut records = Vec::new();
    for command in commands {
        let Ok(invocation) = plugin.invoke(&command, serde_json::Map::new()).await else {
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
    let source_type = type_of(&text(record, "source_type")?)?;
    let target_type = type_of(&text(record, "target_type")?)?;
    let relation =
        ono_spatial_core::relation::spec(&relation_id(package, source_type, target_type))?;
    let source = place_of(
        providers,
        session,
        source_type,
        &text(record, "source_key")?,
        now,
    )
    .await?;
    let target = place_of(
        providers,
        session,
        target_type,
        &text(record, "target_key")?,
        now,
    )
    .await?;
    if source == target {
        return None;
    }
    // §36.2: an AI- or plugin-derived relationship must never "appear exact without provenance".
    // The host did not observe this edge, so `exact` is not a claim it can pass on however sure
    // the package is; everything weaker travels as the package stated it (§11.5).
    let claimed = text(record, "confidence")
        .and_then(|name| Confidence::from_name(&name))
        .unwrap_or(Confidence::Unknown);
    let confidence = match claimed {
        Confidence::Exact => Confidence::Strong,
        other => other,
    };
    let mut edge = RelationshipEdge::new(
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
    if let Some(word) = text(record, "relation") {
        edge = edge.with_attribute("provider_relation", Value::string(&word));
    }
    Some((edge, source, target))
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

/// The §3.3 kind a contributor named, however it spelled the case.
///
/// v0.2 §31.7 writes a contribution shape as `process->process` and §3.3's own vocabulary spells
/// the type `Process`; a package that declares one and answers with the other means the same
/// thing both times.
fn type_of(name: &str) -> Option<SpatialType> {
    SpatialType::ALL
        .iter()
        .copied()
        .find(|kind| kind.as_str().eq_ignore_ascii_case(name.trim()))
}

/// The id a package's relation between two kinds of place is registered under.
///
/// §31.5 reserves `<publisher>.<package>` to its owner, so a contributed relation lives inside
/// the package's own namespace and can never collide with a core relation or another package's.
fn relation_id(package: &str, source: SpatialType, target: SpatialType) -> String {
    format!(
        "{package}.{}_to_{}",
        source.as_str().to_ascii_lowercase(),
        target.as_str().to_ascii_lowercase()
    )
}

/// A record's string field, where it has one.
fn text(record: &RecordValue, field: &str) -> Option<String> {
    record
        .get(field)
        .and_then(|value| value.as_str().ok())
        .map(str::to_owned)
}
