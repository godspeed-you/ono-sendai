//! Building a place view: what a place is, what surrounds it, and what could be learned about it
//! (spec v0.4 §3.1, §3.6, §6.1, §6.2, §7, §24, §35.2).
//!
//! This is the shell's half of `look` and `near`. It asks the providers — which nothing else may
//! do (§2.16) — hands what they answered to the provider bridge, and lets `ono-spatial-query`
//! decide what of it is shown and in which order (§45.3, §45.6). Everything it does itself is
//! bookkeeping the two library crates cannot do: which target belongs to which canonical
//! collection, and what a refusal from one of them means for the group that would have shown it.
//!
//! The honesty rule of §35.2 runs through all of it. A group is `available` or `empty` only when
//! a provider actually answered; a target nobody serves is `unsupported`; a refusal keeps the
//! provider's own diagnostic and the state its error code maps to; and an expensive enumeration
//! nobody asked for is `unknown`, not zero (§2.17, §33.3, §42.4).

use std::collections::{BTreeMap, BTreeSet};

use jiff::Timestamp;
use ono_command::Invocation;
use ono_core::ErrorCode;
use ono_pipeline::ValueStream;
use ono_provider_api::{ProviderRegistry, Query};
use ono_spatial_core::{
    CanonicalSpace, CostClass, Freshness, Landmark, Neighborhood, NeighborhoodGroup,
    PermissionState, SpatialId, SpatialType, space,
};
use ono_spatial_index::SpatialIndex;
use ono_spatial_query::{Exit, NeighborhoodRequest, declared_children, source_of_space};
use ono_value::{ErrorValue, Provenance, RecordValue, SchemaId, Value, builtin_schemas};

use crate::spatial::session::SpatialSessionState;

/// The provider id the spatial layer signs its own composition with (ADR-0140).
const COMPOSER: &str = "ono.spatial";

/// What one canonical space's exits look like right now, and what the shell learned on the way.
pub struct Surroundings {
    exits: Vec<Exit>,
    /// What the place itself could be told about its own contents (§35.2) — `available` where it
    /// only holds other places, because declared geography is always there (§4).
    permission: PermissionState,
}

impl Surroundings {
    /// The exits, in the geography's declaration order.
    #[must_use]
    pub fn exits(self) -> Vec<Exit> {
        self.exits
    }

    /// What this user could be told about what the place holds (§35.2).
    #[must_use]
    pub fn permission(&self) -> PermissionState {
        self.permission
    }
}

/// Observes what lies behind every exit of `here`, asking each provider target once (§34).
///
/// The exits of a canonical space are its served child spaces, each labelled with the child's
/// own label and holding the places *inside* that child, plus — where the space itself holds
/// objects rather than other places — one exit of its own contents. §24.2 is why: a group label
/// is the word `enter` takes, so it names the place it leads into (ADR-0143).
pub async fn observe_space(
    ctx: &Invocation<'_>,
    session: &mut SpatialSessionState,
    here: &'static CanonicalSpace,
    complete: bool,
    now: Timestamp,
) -> Result<Surroundings, ErrorValue> {
    let children: Vec<&'static CanonicalSpace> = space::children(here.id)
        .filter(|child| child.is_served())
        .collect();
    // The space's own contents come last: a user standing in COMPUTE meets its collections
    // first, and one standing in CONTAINERS meets the containers because there is nothing else.
    let mut sourced: Vec<&'static CanonicalSpace> = children.clone();
    if here.member_type.is_some() {
        sourced.push(here);
    }

    let targets: BTreeSet<&'static str> = sourced
        .iter()
        .filter_map(|space| source_of_space(space.id))
        .filter(|source| affordable(source.cost, complete))
        .flat_map(|source| source.targets.iter().copied())
        .collect();
    let observed = observe(ctx.providers(), session, &targets, now).await;

    let mut exits = Vec::with_capacity(sourced.len());
    for space in children {
        exits.push(exit_of(&observed, space, space.label, complete));
    }
    let permission = if here.member_type.is_some() {
        let own = exit_of(&observed, here, here.label, complete);
        let state = own.state();
        exits.push(own);
        state
    } else {
        // A place that holds only other places holds them by declaration: they are there
        // whatever a provider says, which is exactly what §4 requires of an unavailable domain.
        PermissionState::Available
    };
    Ok(Surroundings { exits, permission })
}

/// The exit that leads into `space`, labelled `label`.
fn exit_of(
    observed: &Observed,
    space: &'static CanonicalSpace,
    label: &str,
    complete: bool,
) -> Exit {
    let Some(source) = source_of_space(space.id) else {
        // Geography: the places inside it are declared, not observed, so listing them cannot fail.
        return Exit::open(label, declared_children(space.id));
    };
    if source.targets.is_empty() {
        return Exit::withheld(
            label,
            PermissionState::Unsupported,
            format!("no provider answers for {label}"),
        );
    }
    if !affordable(source.cost, complete) {
        // §33.3 makes the filesystem query-driven and §32.1 makes it expensive; a `look` that
        // walked it would spend the whole §34 budget before saying where the user is standing.
        return Exit::withheld(label, PermissionState::Unknown, "available on request");
    }
    match observed.state_of(source.targets) {
        Some((state, detail)) => Exit::withheld(label, state, detail),
        None => Exit::open(label, observed.of_types(source.accepts)),
    }
}

/// Whether a source of this cost is enumerated by an orientation command (§32.1, §33.3).
fn affordable(cost: CostClass, _complete: bool) -> bool {
    cost != CostClass::Expensive
}

/// What the providers answered for a set of targets, and what they refused.
#[derive(Default)]
struct Observed {
    by_type: BTreeMap<SpatialType, Vec<SpatialId>>,
    refused: BTreeMap<&'static str, (PermissionState, String)>,
    served: BTreeSet<&'static str>,
}

impl Observed {
    /// The places of these exact types, in the order the providers answered.
    ///
    /// The match is exact rather than by [`SpatialType::is_a`]: `ono.socket/1` answers for both
    /// `network.listeners` and `network.connections`, and a listener is not a connection (§14.3).
    fn of_types(&self, accepts: &[SpatialType]) -> Vec<SpatialId> {
        accepts
            .iter()
            .filter_map(|kind| self.by_type.get(kind))
            .flat_map(|ids| ids.iter().cloned())
            .collect()
    }

    /// Why these targets could not answer, when none of them did (§35.2).
    fn state_of(&self, targets: &[&'static str]) -> Option<(PermissionState, String)> {
        if targets.iter().any(|target| self.served.contains(target)) {
            return None;
        }
        for target in targets {
            if let Some((state, detail)) = self.refused.get(target) {
                return Some((*state, detail.clone()));
            }
        }
        Some((
            PermissionState::Unsupported,
            format!(
                "no provider answers for `{}`",
                targets.first().copied().unwrap_or_default()
            ),
        ))
    }
}

/// Observes the filesystem object at an absolute path, and registers it (§15.1, §33.3).
///
/// §33.3 makes the filesystem query-driven: nothing enumerates it, so a path only becomes a place
/// when somebody names one. That is what a selector spelled as a path does — `storage:/data`,
/// `/etc/nginx` — and asking for exactly that path is the whole query (§34).
pub async fn observe_path(
    ctx: &Invocation<'_>,
    session: &mut SpatialSessionState,
    path: &std::path::Path,
    now: Timestamp,
) {
    if ctx.providers().for_target("file").is_empty() {
        return;
    }
    let query = Query::target("file").with(ono_provider_api::Selector::field(
        "path",
        Value::Path(std::sync::Arc::from(path)),
    ));
    let Ok(stream) = ctx.providers().snapshot(&query) else {
        return;
    };
    let records: Vec<RecordValue> = stream
        .collect()
        .await
        .into_values()
        .into_iter()
        .filter_map(|value| match value {
            Value::Record(record) => Some(RecordValue::clone(&record)),
            _ => None,
        })
        .collect();
    session.absorb(&records, now);
}

/// Asks a set of provider targets once and registers everything they answered (§33.1, §34).
///
/// The plan of which targets to ask belongs to `ono-spatial-query` (§45.3); asking is the
/// shell's, because nothing but the shell may reach a provider (§2.16).
pub async fn observe_targets(
    ctx: &Invocation<'_>,
    session: &mut SpatialSessionState,
    targets: &BTreeSet<&'static str>,
    now: Timestamp,
) {
    let _ = observe(ctx.providers(), session, targets, now).await;
}

/// Asks every target once and registers what came back (§33.1, §42.1).
async fn observe(
    providers: &ProviderRegistry,
    session: &mut SpatialSessionState,
    targets: &BTreeSet<&'static str>,
    now: Timestamp,
) -> Observed {
    let mut observed = Observed::default();
    for target in targets {
        if providers.for_target(target).is_empty() {
            continue;
        }
        let stream = match providers.snapshot(&Query::target(*target)) {
            Ok(stream) => stream,
            Err(error) => {
                observed.refused.insert(
                    target,
                    (
                        PermissionState::of_refusal(&error),
                        error.message().to_owned(),
                    ),
                );
                continue;
            }
        };
        let collected = stream.collect().await;
        if let Some(error) = collected.errors().first() {
            observed.refused.insert(
                target,
                (
                    PermissionState::of_refusal(error),
                    error.message().to_owned(),
                ),
            );
        }
        let records: Vec<RecordValue> = collected
            .into_values()
            .into_iter()
            .filter_map(|value| match value {
                Value::Record(record) => Some(RecordValue::clone(&record)),
                _ => None,
            })
            .collect();
        if records.is_empty() && observed.refused.contains_key(target) {
            continue;
        }
        observed.served.insert(target);

        for record in &records {
            if let Ok(object) = session.projection_of_object(record) {
                observed
                    .by_type
                    .entry(object.object_type())
                    .or_default()
                    .push(object.spatial_id().clone());
            }
        }
        session.absorb(&records, now);
    }
    observed
}

// --- the records the commands emit -----------------------------------------------------------

/// The `ono.spatial-place/1` record of one place (§3.1, ADR-0140).
///
/// `scope` is the session's, and it is used only for a canonical space: an observed object
/// belongs to the scope it was observed in, which is part of its identity (§3.2, §10.2).
pub fn place_record(
    index: &SpatialIndex,
    id: &SpatialId,
    scope: &ono_spatial_core::SpatialScope,
    permission: PermissionState,
    pinned: bool,
    now: Timestamp,
) -> Result<RecordValue, ErrorValue> {
    place_record_of(index, id, scope, permission, pinned, None, now)
}

/// The same record, with what the provider last said about the object beside it.
///
/// §24.1 budgets a place view: the *summary* the provider's own default view names — a process's
/// state, a mount's filesystem and source, a socket's endpoints — is what §12 and §13 print, and
/// the exhaustive property list stays `inspect`'s. Nothing here is read from the system; it is
/// the record a provider already answered with (§2.16).
pub fn place_record_of(
    index: &SpatialIndex,
    id: &SpatialId,
    scope: &ono_spatial_core::SpatialScope,
    permission: PermissionState,
    pinned: bool,
    observed: Option<&RecordValue>,
    now: Timestamp,
) -> Result<RecordValue, ErrorValue> {
    let schema = schema("ono.spatial-place", 1)?;
    let place_path = ono_spatial_query::place_path(index, id);

    let entry = index.get(id);
    let space = ono_spatial_query::resolve::space_of(id);
    let (name, object_type, spatial_type, freshness, observed_at, provenance, own_scope) =
        match (space, entry) {
            (Some(space), _) => {
                // §3.2: the geography is per host, so a canonical space belongs to the scope of
                // the host whose geography it is — never to whichever host the session happens
                // to be standing on (§19.2, §43.7).
                let host = ono_spatial_query::resolve::scope_of_space(id);
                (
                    space.label_in(host.as_ref()),
                    space.place_schema().to_owned(),
                    space.object_type,
                    // Declared geography is as current as the build: nothing observed it, and
                    // nothing can make it stale (§4.1).
                    Freshness::Live,
                    Value::Null,
                    declared_provenance(host.as_ref()),
                    host.unwrap_or_else(|| scope.clone()),
                )
            }
            (None, Some(entry)) => (
                entry.object().display_name().to_owned(),
                entry.canonical_ref().id().schema().to_string(),
                entry.object().object_type(),
                index.freshness(id, now),
                Value::Timestamp(entry.observed_at()),
                entry.object().provenance().clone(),
                entry.object().scope().clone(),
            ),
            (None, None) => {
                return Err(ErrorValue::new(
                    ErrorCode::SpatialNotFound,
                    "that place is no longer in the spatial index",
                ));
            }
        };
    let capability_values: Vec<Value> = entry.map_or_else(
        || vec![Value::string("look"), Value::string("near")],
        |entry| {
            entry
                .object()
                .capabilities()
                .iter()
                .map(|capability| Value::string(capability.as_str()))
                .collect()
        },
    );
    let identity = entry.map_or(Ok(Value::Null), |entry| {
        identity_record(entry.object().identity())
    })?;
    let tier = id
        .tier()
        .map_or(Value::Null, |tier| Value::string(tier.as_str()));

    let canonical_ref = entry.map_or(Value::Null, |entry| reference_record(entry.canonical_ref()));
    let canonical_parent = ono_spatial_query::resolve::parent_of(index, id)
        .map_or(Value::Null, |parent| Value::string(&parent.to_string()));
    let state = observed
        .and_then(|record| record.get("state"))
        .filter(|value| !value.is_null())
        .cloned()
        .unwrap_or(Value::Null);
    let summary = observed.map_or(Value::Null, summary_record);

    let record = RecordValue::builder(schema, Provenance::clone(&provenance))
        .set("spatial_id", Value::string(&id.to_string()))?
        .set("type", Value::string(&kind_chain(spatial_type)))?
        .set("canonical_ref", canonical_ref)?
        .set("canonical_parent", canonical_parent)?
        .set("state", state)?
        .set("summary", summary)?
        .set("name", Value::string(&name))?
        .set("display_name", Value::string(&name))?
        .set("object_type", Value::string(&object_type))?
        .set("spatial_type", Value::string(spatial_type.as_str()))?
        .set("place_path", Value::string(&place_path))?
        .set("scope", Value::string(&own_scope.to_string()))?
        .set("lifetime", lifetime_record(entry))?
        .set("freshness", Value::string(freshness.as_str()))?
        .set("observed_at", observed_at)?
        .set("identity_tier", tier)?
        .set("capabilities", Value::list(capability_values))?
        .set("identity", identity)?
        .set("permission", Value::string(permission.as_str()))?
        .set("pinned", Value::Bool(pinned))?
        .set("provenance", ono_command::provenance_value(&provenance))?;
    // The fields the provider identifies the object by travel at the top level as well, because
    // that is where a reader looks for them: `look --json | from json | where pid == 1842` is an
    // ordinary v0.2 pipeline, and a place that hid its pid inside a nested reference would need a
    // second vocabulary to be filtered by (§28, §29.4).
    let mut record = record;
    if let Some(entry) = entry {
        let reference = entry.canonical_ref().id();
        if let Some(object_schema) = builtin_schemas().get(reference.schema()) {
            for (field, value) in object_schema.identity().iter().zip(reference.values()) {
                // A name the place contract already declares keeps its own meaning: `name` is
                // what a person calls the place, whatever the object's schema calls its identity.
                if schema_declares(field) {
                    continue;
                }
                record = record.set_extra(field, value.clone());
            }
        }
    }
    Ok(record.build())
}

/// The §19.1 link map: every link this session holds, with the state of each.
///
/// It is built from the session's own link table rather than from an enumeration of the network:
/// a link is a connection this shell holds, and §35.4 forbids treating a name that resembles one
/// as a place. A link that is not connected stays in the list with the state that says so (§35.2,
/// §53), because a link map that hides what it cannot reach is the `empty` answer for a host that
/// is merely unreachable.
pub fn link_records() -> Result<Vec<Value>, ErrorValue> {
    let schema = schema("ono.link-place", 1)?;
    let mut rows = Vec::new();
    for link in crate::spatial::links::all() {
        let scope = ono_spatial_core::SpatialScope::remote_host(
            &link.name,
            ono_spatial_core::BootIdentity::unknown_boot(&link.name),
        );
        // §4 gives every host the canonical geography, and §19.1 already lists the link as
        // somewhere to go, so the far root is a place as soon as the link is one. Learning it
        // names it; it does not stand on it (§35.4).
        ono_spatial_core::space::learn(&scope);
        let root = Value::string(&space::root().spatial_id_in(Some(&scope)).to_string());
        rows.push(Value::Record(std::sync::Arc::new(
            RecordValue::builder(
                std::sync::Arc::clone(&schema),
                Provenance::local(COMPOSER, SchemaId::new("ono.link-place", 1)),
            )
            .set("name", Value::string(&link.name))?
            .set("display_name", Value::string(&link.name))?
            .set("host", Value::string(&link.host))?
            .set("transport", Value::string(&link.transport))?
            .set("state", Value::string(link.state()))?
            .set("reachable", Value::Bool(link.reachable()))?
            .set("scope", Value::string(&scope.to_string()))?
            .set("spatial_id", root)?
            .build(),
        )));
    }
    Ok(rows)
}

/// The provenance of declared geography: the spatial layer composed it, and it says which host's
/// geography it is (§4.1, §19.4, §25.2).
///
/// §19.4 requires an observation from the far side to say so, and the geography of a linked host
/// is a statement about that host. A reader who cannot tell a remote place from a local one
/// cannot check anything the far side said.
fn declared_provenance(host: Option<&ono_spatial_core::SpatialScope>) -> Provenance {
    let schema = SchemaId::new("ono.spatial-place", 1);
    match host {
        Some(scope) => Provenance::remote(COMPOSER, scope.host_scope().id(), schema),
        None => Provenance::local(COMPOSER, schema),
    }
}

/// Whether `ono.spatial-place/1` already declares a field of this name.
fn schema_declares(field: &str) -> bool {
    builtin_schemas()
        .get(&SchemaId::new("ono.spatial-place", 1))
        .is_some_and(|schema| schema.position_of(field).is_some())
}

/// What kind of place this is, in §3.3's vocabulary and the families it belongs to.
///
/// §3.3 lists `Socket` among the place kinds, and §14.3 and §14.4 then split it into a listener
/// and a connection. Both readings are true of the same place, and a reader that asks "is this a
/// socket?" and one that asks "is this a connection?" must both get an answer — so the chain is
/// written out, most specific first: `connection socket`.
fn kind_chain(object_type: SpatialType) -> String {
    let mut chain = vec![object_type.as_str().to_ascii_lowercase()];
    let mut current = object_type;
    while let Some(general) = current.generalises_to() {
        chain.push(general.as_str().to_ascii_lowercase());
        current = general;
    }
    chain.join(" ")
}

/// The provider's own reference to the object — the schema it serves it under, and the values
/// of the fields that schema calls its identity (§3.1's `canonical_ref`, §37.1).
///
/// It is what makes "every visible node corresponds to inspectable data" checkable (§23.1), so
/// the map builds its `object_ref` from the very same function a place view does.
pub fn reference_record(reference: &ono_provider_api::ObjectRef) -> Value {
    let id = reference.id();
    let Some(schema) = builtin_schemas().get(id.schema()) else {
        return Value::Null;
    };
    let mut map = ono_value::MapValue::new();
    map.insert("schema".into(), Value::string(&id.schema().to_string()));
    for (field, value) in schema.identity().iter().zip(id.values()) {
        map.insert(field.as_ref().into(), value.clone());
    }
    Value::Map(std::sync::Arc::new(map))
}

/// The provider's own summary of the object: the columns its schema puts in a default view.
///
/// §12's `state running / user www-data / uptime 17d` and §13's `since` are exactly these, and
/// they are the provider's words, not the spatial layer's (§2.16, §24.1).
fn summary_record(record: &RecordValue) -> Value {
    let mut map = ono_value::MapValue::new();
    for column in record.schema().default_view() {
        if let Some(value) = record.get(column) {
            map.insert(column.as_ref().into(), value.clone());
        }
    }
    if map.is_empty() {
        return Value::Null;
    }
    Value::Map(std::sync::Arc::new(map))
}

/// What is known about the object's lifetime (§3.1's `lifetime`, §10.1, §10.2).
///
/// §10.2 makes a pid without its start time not an identity at all, so the descriptor names when
/// the object began and when it was last seen, beside the tier that says how far either can be
/// trusted. A canonical space has no lifetime: it is declared, not born.
fn lifetime_record(entry: Option<&ono_spatial_index::IndexEntry>) -> Value {
    let Some(entry) = entry else {
        return Value::Null;
    };
    let lifetime = entry.object().lifetime();
    let mut map = ono_value::MapValue::new();
    map.insert("tier".into(), Value::string(lifetime.tier().as_str()));
    map.insert(
        "first_observed".into(),
        Value::Timestamp(lifetime.first_observed()),
    );
    map.insert(
        "last_observed".into(),
        Value::Timestamp(lifetime.last_observed()),
    );
    map.insert(
        "ended".into(),
        lifetime.end().map_or(Value::Null, Value::Timestamp),
    );
    if let Some(started) = entry
        .object()
        .identity()
        .components()
        .iter()
        .find(|(name, _)| name == "start_time")
        .map(|(_, value)| value.clone())
    {
        map.insert("start_time".into(), Value::string(&started));
    }
    Value::Map(std::sync::Arc::new(map))
}

/// The identity components of §3.1, as an open record a reader can recognise the place by.
///
/// A pid without its boot is not an identity (§10.2), so all of them travel together; and none of
/// them is a *property* of the object, which is what §24.1 keeps out of a place view.
fn identity_record(identity: &ono_spatial_core::SpatialIdentity) -> Result<Value, ErrorValue> {
    let mut map = ono_value::MapValue::new();
    for (name, value) in identity.components() {
        // The scope is a boundary, not a name (§3.2): the place carries it in its own `scope`.
        if name == "scope" {
            continue;
        }
        map.insert(name.as_str().into(), Value::string(value));
    }
    if map.is_empty() {
        return Ok(Value::Null);
    }
    Ok(Value::Map(std::sync::Arc::new(map)))
}

/// One neighborhood group as `ono.neighborhood-group/1` (§3.6, §24.2, §35.2).
pub fn group_record(
    index: &SpatialIndex,
    here: &SpatialId,
    group: &NeighborhoodGroup,
    scope: &ono_spatial_core::SpatialScope,
    with_members: bool,
    now: Timestamp,
) -> Result<RecordValue, ErrorValue> {
    let schema = schema("ono.neighborhood-group", 1)?;
    let members = if with_members {
        let mut records = Vec::with_capacity(group.members().len());
        for member in group.members() {
            records.push(Value::Record(std::sync::Arc::new(place_record(
                index,
                member,
                scope,
                PermissionState::Available,
                false,
                now,
            )?)));
        }
        Value::list(records)
    } else {
        Value::Null
    };
    // §2.17 and §42.4: a count nobody could take is not zero. A withheld group carries its state
    // and no number, so `files  permission denied` can never be read as `files  0`.
    let count = group.total().map_or(Value::Null, |total| {
        Value::Int(i128::try_from(total).unwrap_or(i128::MAX))
    });
    Ok(RecordValue::builder(
        schema,
        Provenance::local(COMPOSER, SchemaId::new("ono.neighborhood-group", 1)),
    )
    .set("label", Value::string(group.label()))?
    .set("name", Value::string(group.label()))?
    .set("display_name", Value::string(group.label()))?
    .set(
        "relation",
        group
            .relation()
            .map_or(Value::Null, |relation| Value::string(relation.as_str())),
    )?
    .set("count", count)?
    .set("state", Value::string(group.state().as_str()))?
    .set("detail", group.detail().map_or(Value::Null, Value::string))?
    .set("navigable", Value::Bool(navigable(here, group.label())))?
    .set("freshness", Value::string(group.freshness().as_str()))?
    .set("members", members)?
    .build())
}

/// Whether `enter <label>` from `here` is a move (§24.2).
///
/// A group is an exit when its label names a place other than the one the user is standing in.
/// The collection a user is already standing in is not an exit out of itself, and §24.2 forbids
/// rendering it as one.
fn navigable(here: &SpatialId, label: &str) -> bool {
    ono_spatial_query::resolve::space_of(here).is_none_or(|space| {
        space::children(space.id).any(|child| child.is_served() && child.label == label)
    })
}

/// One landmark as `ono.landmark/1` (§3.7).
pub fn landmark_record(
    index: &SpatialIndex,
    landmark: &Landmark,
) -> Result<RecordValue, ErrorValue> {
    let schema = schema("ono.landmark", 1)?;
    let name = index.get(landmark.subject()).map_or_else(
        || landmark.subject().to_string(),
        |entry| entry.object().display_name().to_owned(),
    );
    Ok(RecordValue::builder(
        schema,
        Provenance::local(COMPOSER, SchemaId::new("ono.landmark", 1)),
    )
    .set("subject", Value::string(&landmark.subject().to_string()))?
    .set("name", Value::string(&name))?
    .set("reason", Value::string(landmark.reason().as_str()))?
    .set("evidence", Value::string(landmark.evidence()))?
    .set("source", Value::string(&source_of(landmark.source())))?
    .build())
}

/// The `ono.neighborhood/1` record of a projection (§3.6).
pub fn neighborhood_record(
    neighborhood: &Neighborhood,
    groups: Vec<Value>,
    landmarks: Vec<Value>,
) -> Result<RecordValue, ErrorValue> {
    let schema = schema("ono.neighborhood", 1)?;
    let hidden = i128::try_from(neighborhood.hidden_count()).unwrap_or(i128::MAX);
    Ok(RecordValue::builder(
        schema,
        Provenance::local(COMPOSER, SchemaId::new("ono.neighborhood", 1)),
    )
    .set("center", Value::string(&neighborhood.center().to_string()))?
    .set("groups", Value::list(groups))?
    .set("landmarks", Value::list(landmarks))?
    .set("hidden_count", Value::Int(hidden))?
    .set(
        "completeness",
        Value::string(neighborhood.completeness().as_str()),
    )?
    .set(
        "generated_at",
        Value::Timestamp(neighborhood.generated_at()),
    )?
    .build())
}

/// One neighbour as `ono.spatial-neighbor/1` (§6.2).
pub fn neighbor_record(
    index: &SpatialIndex,
    here: &SpatialId,
    relation: &str,
    state: PermissionState,
    member: &SpatialId,
    scope: &ono_spatial_core::SpatialScope,
    pinned: bool,
    now: Timestamp,
) -> Result<RecordValue, ErrorValue> {
    let schema = schema("ono.spatial-neighbor", 1)?;
    let place = place_record(
        index,
        member,
        scope,
        PermissionState::Available,
        pinned,
        now,
    )?;
    let field = |name: &str| place.get(name).cloned().unwrap_or(Value::Null);
    // §11.4: a displayed relationship is inspectable — the relation, both ends, the direction,
    // the provider, the provenance, the confidence and when it was observed. A neighbour *is* a
    // displayed relationship, so it carries them rather than pointing at somewhere they live.
    let edge = index.get(here).and_then(|entry| {
        entry
            .edges()
            .iter()
            .find(|edge| edge.other_end(here) == Some(member))
            .cloned()
    });
    let label = edge
        .as_ref()
        .and_then(|edge| edge.label_from(here))
        .unwrap_or(relation);
    let (source, target, direction, confidence, observed_at, provenance) = match &edge {
        Some(edge) => (
            Value::string(&edge.source().to_string()),
            Value::string(&edge.target().to_string()),
            Value::string(edge.direction().as_str()),
            Value::string(edge.confidence().as_str()),
            Value::Timestamp(edge.observed_at()),
            edge.provenance().clone(),
        ),
        // A canonical collection reaches its members by hierarchy, not by an edge (§3.4, §2.6):
        // there is no relationship to explain, and inventing `source`/`target`/`confidence` for
        // one would be exactly the confusion §2.6 forbids. The neighbour is still a place, and
        // the group it appeared under still says how it was reached.
        None => (
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            index.get(member).map_or_else(
                || Provenance::local(COMPOSER, SchemaId::new("ono.spatial-neighbor", 1)),
                |entry| entry.object().provenance().clone(),
            ),
        ),
    };
    Ok(RecordValue::builder(
        schema,
        Provenance::local(COMPOSER, SchemaId::new("ono.spatial-neighbor", 1)),
    )
    .set("relation", Value::string(label))?
    .set("group", Value::string(relation))?
    .set("type", field("type"))?
    .set("canonical_ref", field("canonical_ref"))?
    .set("identity", field("identity"))?
    .set("source", source)?
    .set("target", target)?
    .set("direction", direction)?
    .set("confidence", confidence)?
    .set("observed_at", observed_at)?
    .set(
        "provider",
        edge.as_ref()
            .map_or(Value::Null, |_| Value::string(provenance.provider())),
    )?
    .set("provenance", ono_command::provenance_value(&provenance))?
    .set(
        "provider_relation",
        edge.as_ref()
            .and_then(|edge| edge.attributes().get("provider_relation").cloned())
            .unwrap_or(Value::Null),
    )?
    .set("spatial_id", field("spatial_id"))?
    .set("name", field("name"))?
    .set("display_name", field("display_name"))?
    .set("object_type", field("object_type"))?
    .set("spatial_type", field("spatial_type"))?
    .set("state", Value::string(state.as_str()))?
    .set("place_path", field("place_path"))?
    .set("scope", field("scope"))?
    .set("freshness", field("freshness"))?
    .set("pinned", Value::Bool(pinned))?
    .build())
}

/// The `ono.system/1` record of §7.1, for the root place of this host.
///
/// §7.1's conceptual schema names `os`, `kernel` and `uptime` beside the host's identity. No
/// installed provider answers for them — there is no `get system` — so they are null rather than
/// read behind the providers' backs (§2.16) or invented (§2.17, §35.3). The phase that adds a
/// system producer fills them; until then the field says "not known", which is a real answer.
pub fn system_record(
    scope: &ono_spatial_core::SpatialScope,
    domains: Vec<Value>,
    landmarks: Vec<Value>,
    now: Timestamp,
) -> Result<RecordValue, ErrorValue> {
    let schema = schema("ono.system", 1)?;
    Ok(RecordValue::builder(
        schema,
        Provenance::local(COMPOSER, SchemaId::new("ono.system", 1)),
    )
    .set("host", Value::string(&scope.host_scope().to_string()))?
    .set("hostname", Value::string(scope.host_scope().id()))?
    .set("os", Value::Null)?
    .set("kernel", Value::Null)?
    .set("uptime", Value::Null)?
    .set("domains", Value::list(domains))?
    .set("landmarks", Value::list(landmarks))?
    .set("links", Value::Null)?
    .set("generated_at", Value::Timestamp(now))?
    .build())
}

/// The bounded projection around wherever the session is standing (§3.6).
///
/// A canonical space is declared geography with no edges of its own, so its neighborhood is built
/// from the exits the shell observed; an object the index holds has edges, and the ranking of
/// §45.3 answers for it. Neither path ranks anything here (ADR-0143).
pub async fn neighborhood_here(
    ctx: &Invocation<'_>,
    session: &mut SpatialSessionState,
    request: &NeighborhoodRequest,
    now: Timestamp,
) -> Result<(Neighborhood, PermissionState), ErrorValue> {
    let here = session.current_place().clone();
    // §35.2/§43.7: standing on a host whose link is gone, what is behind the exits is `stale` —
    // not `empty`, and above all not the local objects the same providers would answer with if
    // they were asked here instead (§35.4). Nothing is asked at all.
    if let Some(detail) = unreachable_host(session) {
        let pins = session.pins().clone();
        let exits = stale_exits(&here, &detail);
        let neighborhood = ono_spatial_query::space_neighborhood(
            session.index(),
            &here,
            exits,
            request,
            &pins,
            now,
        );
        return Ok((neighborhood, PermissionState::Stale));
    }
    if let Some(space) = ono_spatial_query::resolve::space_of(&here) {
        let surroundings = observe_space(ctx, session, space, request.is_complete(), now).await?;
        let permission = surroundings.permission();
        let pins = session.pins().clone();
        let neighborhood = ono_spatial_query::space_neighborhood(
            session.index(),
            &here,
            surroundings.exits(),
            request,
            &pins,
            now,
        );
        return Ok((neighborhood, permission));
    }
    // An object's exits are its relationship edges, and those are the v0.2 relationship graph's
    // (§2.16, §31.3). They are read here, once, before anything is ranked.
    let interest = crate::spatial::relations::Interest::here()
        .complete(request.is_complete())
        .along(request.named_relation().map(str::to_owned))
        .of_type(request.named_type());
    crate::spatial::relations::observe(ctx, session, &here, &interest, now).await?;
    let pins = session.pins().clone();
    let neighborhood =
        ono_spatial_query::neighborhood_of(session.index(), &here, request, &pins, now);
    Ok((neighborhood, PermissionState::Available))
}

/// Why the host the session is standing on cannot be reached, where it cannot be (§35.4, §19.1).
///
/// A link that was detached, torn down or never negotiated is the same answer here: this session
/// is not following the far side, so what it knows about it is what it knew when it stopped.
fn unreachable_host(session: &SpatialSessionState) -> Option<String> {
    let host = session.host()?;
    if crate::spatial::links::reachable(&host.link) {
        return None;
    }
    Some(match crate::spatial::links::facts(&host.link) {
        Some(facts) => format!(
            "the link `{}` is {}; what is behind it was last read before that",
            host.link,
            facts.state()
        ),
        None => format!(
            "this session no longer holds a link called `{}`; what is behind it was last read \
             while it did",
            host.link
        ),
    })
}

/// The exits of a place whose host cannot be reached: every one of them known by name, none of
/// them counted (§35.2, §2.17).
fn stale_exits(here: &SpatialId, detail: &str) -> Vec<Exit> {
    let Some(space) = ono_spatial_query::resolve::space_of(here) else {
        // An observed object's exits are its relationship edges, and none of them can be read
        // from here. The place keeps its name and says so; it does not grow an invented exit.
        return Vec::new();
    };
    let mut exits: Vec<Exit> = space::children(space.id)
        .filter(|child| child.is_served())
        .map(|child| Exit::withheld(child.label, PermissionState::Stale, detail))
        .collect();
    if space.member_type.is_some() {
        exits.push(Exit::withheld(space.label, PermissionState::Stale, detail));
    }
    exits
}

/// A stream of already-collected values.
pub fn stream(values: Vec<Value>) -> ValueStream {
    ValueStream::from_values(values)
}

/// How a landmark names who claimed it (§26.5, §35.5).
fn source_of(source: &ono_spatial_core::LandmarkSource) -> String {
    match source {
        ono_spatial_core::LandmarkSource::BuiltIn => "built-in".to_owned(),
        ono_spatial_core::LandmarkSource::Package(id) => id.to_string(),
    }
}

fn schema(name: &str, version: u32) -> Result<std::sync::Arc<ono_value::Schema>, ErrorValue> {
    let id = SchemaId::new(name, version);
    builtin_schemas().get(&id).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::ProviderSchemaViolation,
            format!("the `{id}` contract is not in this build"),
        )
    })
}
