//! The relationship edges of an object place (spec v0.4 §2.16, §3.5, §11.2, §12–§18, §31.3).
//!
//! §2.16 fixes where a spatial edge comes from: "the spatial layer composes provider data and
//! MUST NOT become an undocumented second source of truth", and §31.3 says which data — `map`
//! and `trace` share the underlying graph. So the edges of a place are the ones the v0.2
//! relationship providers of `ono-graph` assert about the very same object, read once per place
//! and translated into the spatial vocabulary of `docs/spec/spatial/relations.yaml`.
//!
//! Translation is all that happens here. The relation a provider names (`reads`), the provider
//! that named it (`linux.open-files`) and the confidence it claimed travel onto the spatial edge
//! unchanged; what this module adds is the declared relation the label belongs to, so that
//! `follow file` and `follow opener` are the two ends of the edge a user can type (§6.4).
//!
//! A provider that could not read what it needs is not a provider that found nothing (§35.2,
//! §42.4). Its failure becomes the withheld state of exactly the groups it feeds, so a `look` at
//! pid 1 says `files  permission denied` rather than `files  0`.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use jiff::Timestamp;
use ono_graph::{Node, ProcessUsers, RelationshipProvider, kernel_relationships};
use ono_provider_api::{ProviderRegistry, Query, Selector};
use ono_spatial_core::{
    Confidence, PermissionState, RelationSpec, RelationshipEdge, SpatialId, SpatialType, relation,
};
use ono_value::{ErrorValue, Provenance, RecordValue, SchemaId, Value};

use crate::spatial::session::SpatialSessionState;

/// What a caller is looking at, so a broad scan is paid for only when it was asked for (§32.1).
#[derive(Debug, Clone, Default)]
pub struct Interest {
    complete: bool,
    label: Option<String>,
    object_type: Option<SpatialType>,
}

impl Interest {
    /// The default look: every relation whose answer is about this object alone.
    #[must_use]
    pub fn here() -> Self {
        Self::default()
    }

    /// `--all`: every relation, however it has to be answered (§6.2).
    #[must_use]
    pub fn complete(mut self, complete: bool) -> Self {
        self.complete = complete;
        self
    }

    /// One relation the caller named, by its `follow` label or the word `look` prints.
    #[must_use]
    pub fn along(mut self, label: Option<String>) -> Self {
        self.label = label;
        self
    }

    /// One kind of neighbour the caller named (`near --type process`).
    #[must_use]
    pub fn of_type(mut self, object_type: Option<SpatialType>) -> Self {
        self.object_type = object_type;
        self
    }

    /// Whether a provider that has to enumerate a whole target to answer is worth asking.
    ///
    /// §32.1: "Default `look` and `map` MUST avoid expensive relationships unless cached or
    /// already available." The cost is not the relation's alone but the *end* it is answered
    /// from: reading one process's descriptors is one directory, and finding every process that
    /// holds one file is every process on the host (ADR-0149).
    fn wants(&self, labels: &[&'static str], reaches: SpatialType) -> bool {
        if self.complete {
            return true;
        }
        if let Some(label) = &self.label
            && labels.contains(&label.as_str())
        {
            return true;
        }
        self.object_type.is_some_and(|wanted| reaches.is_a(wanted))
    }
}

/// The v0.2 relationship providers that answer about one object by enumerating a whole target,
/// with the exits they fill and the kind of place they reach (§32.1, §32.2).
fn broad(provider: &str) -> Option<SpatialType> {
    match provider {
        "linux.file-holders"
        | "linux.user-processes"
        | "linux.mount-users"
        | "linux.socket-owners" => Some(SpatialType::Process),
        _ => None,
    }
}

/// How one relation word of one v0.2 relationship provider reads in the spatial vocabulary.
struct Translation {
    /// The declared relation of `docs/spec/spatial/relations.yaml` the edge belongs to.
    relation: &'static str,
    /// Whether the object being expanded is the relation's `source` (rather than its `target`).
    subject_is_source: bool,
}

/// The v0.2 relationship vocabulary, in the spatial relations it is the same fact as.
///
/// The key is the pair a provider asserts — its own id and its own word for the relation — so a
/// word that means two things for two providers stays two things (`owner` from
/// `linux.socket-owners` is a process; `owner` on a file record is a user).
///
/// A pair that is not here is a relation v0.4 does not declare — a container's image (§7 places
/// no image), a DNS answer, the sockets bound to an interface. Those are real facts of the v0.2
/// graph and stay reachable through `trace`; inventing a spatial relation for them here is
/// exactly the second source of truth §2.16 forbids.
fn translate(provider: &str, relation: &str) -> Option<Translation> {
    let (spatial, subject_is_source) = match (provider, relation) {
        ("linux.process-tree", "child") => ("process.parent_of", true),
        ("linux.process-tree", "parent") => ("process.parent_of", false),
        ("linux.open-files", "reads" | "writes" | "opens") => ("process.opened_file", true),
        ("linux.file-holders", "holder") => ("process.opened_file", false),
        ("linux.process-sockets", "listens" | "connects") => ("process.owns_socket", true),
        ("linux.socket-owners", "owner") => ("process.owns_socket", false),
        ("linux.service-processes", "owns" | "contains") => ("service.controls_process", true),
        ("linux.service-dependencies", "depends-on") => ("service.depends_on", true),
        ("linux.user-processes", "runs") => ("process.run_by_user", false),
        ("linux.process-users", "runs-as") => ("process.run_by_user", true),
        ("linux.user-groups", "primary-group" | "member-of") => ("user.member_of_group", true),
        ("linux.route-interfaces", "via" | "gateway") => ("route.via_interface", true),
        ("linux.interface-routes", "route") => ("route.via_interface", false),
        ("linux.mount-filesystems", "filesystem") => ("filesystem.mounted_at", false),
        ("ono.host-links", "link") => ("host.linked_to", true),
        _ => return None,
    };
    Some(Translation {
        relation: spatial,
        subject_is_source,
    })
}

/// Which groups of a place stop being answerable when one v0.2 provider cannot read (§35.2).
///
/// The labels are the ones `relation_summary` builds its groups from, so a refusal lands on the
/// exit the user is looking at rather than on a relation id nobody typed.
fn labels_of(provider: &str) -> &'static [&'static str] {
    match provider {
        "linux.process-tree" => &["parent", "children"],
        "linux.open-files" => &["files"],
        "linux.file-holders" => &["openers"],
        "linux.process-sockets" => &["sockets"],
        "linux.socket-owners" => &["process"],
        "linux.service-processes" => &["processes"],
        "linux.service-dependencies" => &["dependencies", "dependents"],
        "linux.user-processes" => &["processes"],
        "linux.process-users" => &["user"],
        "linux.user-groups" => &["groups"],
        _ => &[],
    }
}

/// The exits a record of `object_type` can fill from its own fields, through the provider bridge
/// (§45.2). The words are the ones `look` prints, so they line up with a neighborhood group.
fn composed_labels(object_type: SpatialType) -> &'static [&'static str] {
    use SpatialType as T;
    match object_type {
        T::Process => &[
            "parent",
            "service",
            "user",
            "container",
            "namespaces",
            "cgroup",
            "files",
        ],
        T::Socket | T::Listener | T::Connection => &["process", "peer", "connections", "listener"],
        // §15.4 lists the mount boundary among a directory place's neighbours, and the mount
        // table is the shell's to compose from `get mount` (§2.16, ADR-0187).
        T::File | T::Directory => &["owner", "mount"],
        T::Filesystem => &["mounts", "device"],
        T::Mount => &["directory"],
        T::Address | T::Route => &["interface"],
        T::User => &["groups"],
        _ => &[],
    }
}

/// Provider targets a place of this type needs answered before its own declared relations can
/// settle (§32.2's discoverable exits, loaded when the place is looked at).
///
/// A listener's connections are other socket records; a mount's filesystem is a filesystem
/// record. Neither is a relation the v0.2 relationship graph serves, and both are facts the
/// providers already state — they only have to be asked for.
pub(crate) fn adjacent_targets(object_type: SpatialType) -> &'static [&'static str] {
    use SpatialType as T;
    match object_type {
        T::Socket | T::Listener | T::Connection => &["socket"],
        // §15.3, §15.4: a path place sits on a mount, and the mount table says which.
        T::File | T::Directory => &["mount"],
        T::Mount => &["filesystem"],
        T::Filesystem => &["mount"],
        T::Interface => &["route"],
        T::User => &["group"],
        _ => &[],
    }
}

/// The provider target that serves objects of `object_type`, and the field another record names
/// one by — what it takes to ask the provider about this one object again (§33.2).
pub fn target_of(object_type: SpatialType) -> Option<(&'static str, &'static str)> {
    use SpatialType as T;
    Some(match object_type {
        T::Process => ("process", "pid"),
        T::Service => ("service", "name"),
        T::Job => ("job", "id"),
        T::Container => ("container", "id"),
        T::Socket | T::Listener | T::Connection => ("socket", "inode"),
        T::Interface => ("interface", "name"),
        T::Route => ("route", "destination"),
        T::Neighbor => ("neighbor", "address"),
        T::Filesystem => ("filesystem", "source"),
        T::Mount => ("mount", "target"),
        T::BlockDevice | T::Device => ("device", "path"),
        T::File | T::Directory => ("file", "path"),
        T::User => ("user", "uid"),
        T::Group => ("group", "gid"),
        T::Session => ("session", "id"),
        T::Host => ("host", "name"),
        _ => return None,
    })
}

/// The kinds of place a v0.2 provider target serves — the inverse of [`target_of`].
pub(crate) fn types_of_target(target: &str) -> Vec<SpatialType> {
    SpatialType::ALL
        .iter()
        .copied()
        .filter(|object_type| target_of(*object_type).is_some_and(|(name, _)| name == target))
        .collect()
}

/// Asks the provider that serves `id` about it again, and registers what it answered (§33.2).
///
/// A process is asked for its detail view: §12 lists its cgroup, its namespaces, its open files
/// and its sockets among the exits of a process place, and `ono.process-detail/1` is where the
/// v0.2 provider states them. Anything else is asked for plainly.
async fn refresh(
    providers: &ProviderRegistry,
    session: &mut SpatialSessionState,
    id: &SpatialId,
    now: Timestamp,
) -> Option<Arc<RecordValue>> {
    let object_type = session.index().get(id)?.object().object_type();
    let (target, field) = target_of(object_type)?;
    let key = reference_value(session, id, field)?;

    if providers.for_target(target).is_empty() {
        return session.record_of(id).cloned();
    }
    let plain = Query::target(target).with(Selector::field(field, key.clone()));
    // §12 lists a process's cgroup, namespaces, files and sockets among its exits, and
    // `ono.process-detail/1` is where the v0.2 provider states them (§33.1). The detail view is
    // absorbed for those facts and the plain record second, so the place keeps the object type a
    // pipeline recognises — `ono.process/1`, the schema §37.1's identity merge compares.
    if object_type == SpatialType::Process {
        let detail = Query::target(target)
            .with(Selector::field(field, key))
            .option("detail", Value::Bool(true));
        let (records, _) = answered(providers, &detail).await;
        session.absorb(&records, now);
    }
    let (records, refused) = answered(providers, &plain).await;
    if refused {
        // §35.2 and §42.4: a provider that could not read is not a provider that found nothing.
        // Refusing to read a place is never evidence that the place has gone.
        return session.record_of(id).cloned();
    }
    // §33.2: "The index is a cache. Providers remain authoritative." The provider was asked
    // about this one object and did not answer for it, so it is not there any more — and the
    // last live answer must not be handed on as if it still were (§10.3, ADR-0179).
    if !records
        .iter()
        .any(|record| session.projection_of(record).is_ok_and(|seen| &seen == id))
    {
        session.record_removed(id, now);
        return session.record_of(id).cloned();
    }
    session.absorb(&records, now);
    session.record_of(id).cloned()
}

/// The records a query answered with, and whether the provider refused to answer at all.
///
/// The two are different facts and §35.2 keeps them apart: an empty answer from a provider that
/// read the system means the object is not there, and an empty answer from one that could not
/// read means nothing at all (§42.4). Only the first may end a place's lifetime (§10.3).
async fn answered(providers: &ProviderRegistry, query: &Query) -> (Vec<RecordValue>, bool) {
    let Ok(stream) = providers.snapshot(query) else {
        return (Vec::new(), true);
    };
    let collected = stream.collect().await;
    let refused = collected.errors().iter().any(could_not_read);
    let records = collected
        .into_values()
        .into_iter()
        .filter_map(|value| match value {
            Value::Record(record) => Some(RecordValue::clone(&record)),
            _ => None,
        })
        .collect();
    (records, refused)
}

/// Whether an error means the provider could not read, rather than that the object is not there.
///
/// §35.2 and §42.4 make the two different answers, and §10.3 only lets the second one end a
/// place's lifetime. `io.not_found` from a query that named one object is the provider saying the
/// object is gone — `/proc/1842/stat: No such file or directory` is exactly what a process exiting
/// looks like from outside. Everything else is a reading failure, and a reading failure is never
/// evidence of absence.
fn could_not_read(error: &ErrorValue) -> bool {
    !matches!(
        error.code(),
        ono_core::ErrorCode::IoNotFound | ono_core::ErrorCode::ResolveTargetNotFound
    )
}

/// The value another record would name this place by — its provider reference key.
fn reference_value(session: &SpatialSessionState, id: &SpatialId, field: &str) -> Option<Value> {
    // The record the provider last answered with is the first place to look: a file is named by
    // its path and identified by its device and inode, so the reference is not always part of
    // the identity (§3.1's "the display name is not identity" from the other side).
    if let Some(record) = session.record_of(id)
        && let Some(value) = record.get(field)
        && !value.is_null()
    {
        return Some(value.clone());
    }
    let entry = session.index().get(id)?;
    let reference = entry.canonical_ref().id();
    let schema = ono_value::builtin_schemas().get(reference.schema())?;
    let position = schema
        .identity()
        .iter()
        .position(|name| name.as_ref() == field)?;
    reference.values().get(position).cloned()
}

/// Reads the relationship edges of the object at `id` and registers them (§11.2, §12–§18).
///
/// Idempotent by construction: every edge carries a stable identity, so asking twice in one
/// session adds nothing the first answer did not (§33.1, §42.1).
pub async fn observe(
    providers: &ProviderRegistry,
    session: &mut SpatialSessionState,
    id: &SpatialId,
    interest: &Interest,
    now: Timestamp,
) -> Result<(), ErrorValue> {
    let Some(record) = refresh(providers, session, id, now).await else {
        return Ok(());
    };
    let Some(node) = Node::of(&record) else {
        return Ok(());
    };
    let schema = record.schema().id().to_string();
    // A detail view is the same object seen more closely: `ono.process-detail/1` carries every
    // field `ono.process/1` does, and the relationship providers that expand a process read the
    // pid out of either. Refusing them the richer record would cost the exits §12 requires.
    let plain = schema.replace("-detail", "");
    let expands = |subjects: &[&str]| {
        subjects
            .iter()
            .any(|subject| *subject == schema || *subject == plain)
    };
    let object_type = match session.index().get(id) {
        Some(entry) => entry.object().object_type(),
        None => return Ok(()),
    };

    let adjacent: BTreeSet<&'static str> = adjacent_targets(object_type).iter().copied().collect();
    if !adjacent.is_empty() {
        crate::spatial::view::observe_targets(providers, session, &adjacent, now).await;
    }
    // §15.3, §15.4: the mount a path place sits on is a fact of the mount table, composed here
    // so that looking at a directory shows the same boundary entering it did (ADR-0187).
    if matches!(object_type, SpatialType::Directory | SpatialType::File) {
        crate::spatial::storage::link_mount_of(session, id, now);
    }
    // §15.4: "A directory place MUST support normal path navigation" — so standing in one shows
    // what is in it. The listing is bounded by the view, never by the read (ADR-0188).
    if object_type == SpatialType::Directory
        && let Some(path) = crate::spatial::storage::path_of(session, id)
    {
        crate::spatial::storage::observe_children(providers, session, id, &path, now).await;
    }

    let mut answered: BTreeSet<&'static str> = BTreeSet::new();
    // The relations a provider serves that §32.1 kept this view from spending its budget on.
    let mut declined: BTreeSet<&'static str> = BTreeSet::new();
    let providers = Arc::new(providers.clone());
    // §12 lists `user` among the exits of a process place, so the people behind the processes
    // are part of the neighborhood rather than an option of `trace` (v0.2 §22.3).
    let mut sources: Vec<Arc<dyn RelationshipProvider>> =
        kernel_relationships(Arc::clone(&providers));
    sources.push(Arc::new(ProcessUsers::new(providers)));
    for provider in sources {
        if !expands(provider.subjects()) {
            continue;
        }
        // §32.2: an exit nobody asked about stays a discoverable but unloaded one. §35.2 has a
        // word for that, and it is not `unsupported`: the provider is there and was not asked
        // because §32.1 forbids a default `look` from spending a whole-target enumeration.
        if let Some(reaches) = broad(provider.id())
            && !interest.wants(labels_of(provider.id()), reaches)
        {
            declined.extend(labels_of(provider.id()).iter().copied());
            continue;
        }
        if !matches!(
            provider.availability(),
            ono_provider_api::Availability::Available
        ) {
            continue;
        }
        let found = provider.relationships(&node).await;
        let (relationships, failures) = (found.found().to_vec(), found.failures().to_vec());

        for relationship in &relationships {
            let Some(translation) = translate(provider.id(), relationship.edge().relation()) else {
                continue;
            };
            let Some(spec) = relation::spec(translation.relation) else {
                continue;
            };
            let Some(target) = relationship.target().record() else {
                continue;
            };
            let target = RecordValue::clone(target);
            let absorbed = session.absorb(std::slice::from_ref(&target), now);
            let _ = absorbed;
            let Ok(other) = session.projection_of(&target) else {
                continue;
            };
            if &other == id {
                continue;
            }
            let (source, target_id) = if translation.subject_is_source {
                (id.clone(), other)
            } else {
                (other, id.clone())
            };
            let confidence = admissible(spec, relationship.edge().confidence());
            // §19.4: an edge the far side observed says so. Which side observed it is not a
            // detail of the rendering — a one-sided remote observation and a local one are
            // different evidence, and §11.4 makes `inspect relation` answer with the difference.
            let schema = SchemaId::new("ono.spatial-relation", 1);
            let host = session.current_scope();
            let provenance = if host.is_remote() {
                Provenance::remote(provider.id(), host.host_scope().id(), schema)
            } else {
                Provenance::local(provider.id(), schema)
            }
            .observed_at(now);
            let mut edge = RelationshipEdge::new(
                source,
                target_id,
                spec.relation_type(),
                confidence,
                provenance,
                now,
            )
            .with_attribute(
                "provider_relation",
                Value::string(relationship.edge().relation()),
            );
            for (key, value) in relationship.edge().metadata() {
                edge = edge.with_attribute(key, value.clone());
            }
            let (index, _) = session.absorb_with();
            index.record_edge(edge);
        }

        for label in labels_of(provider.id()) {
            if !relation::resolve_label(object_type, label).is_empty() {
                answered.insert(*label);
            }
        }
        // §35.2: a refusal replaces a count, so it may only replace one nobody could take. A
        // provider that answered about some ends and was refused others has answered — the
        // neighborhood carries the incompleteness, and the group carries what was found.
        for failure in failures.iter().filter(|_| relationships.is_empty()) {
            let state = PermissionState::of_refusal(failure);
            let (index, _) = session.absorb_with();
            for label in labels_of(provider.id()) {
                if relation::resolve_label(object_type, label).is_empty() {
                    continue;
                }
                index.record_withheld(id, label, state, failure.message());
            }
        }
    }

    // §35.2 and §2.17: an exit nothing in this build can fill is `unsupported`, which is a
    // different answer from `empty`. Saying "no sockets" about a relation nobody serves would be
    // a count taken from nowhere.
    //
    // A composed exit answers for itself only where the record actually stated it. A reference
    // field the provider left null states nothing, and where a relationship provider serves the
    // same exit and this view declined to spend its budget on it (§32.1), the answer belongs to
    // that provider and has not been asked for: "the owner of this socket" is a scan of every
    // `/proc/<pid>/fd` on the host. Claiming the composed source answered turned that into
    // `process 0`, which is §35.2's own counter-example and the false-empty rendering §42.4
    // forbids (ADR-0209).
    let stated = stated_labels(session, id);
    answered.extend(
        composed_labels(object_type)
            .iter()
            .copied()
            .filter(|label| !declined.contains(label) || stated.contains(label)),
    );
    let unanswered: Vec<&'static str> = relation::exits_from(object_type)
        .map(|(label, _)| label)
        .filter(|label| !answered.contains(label))
        .collect();
    let (index, _) = session.absorb_with();
    for label in unanswered {
        // A relation a provider serves and this view did not spend the budget on is `unknown`,
        // and says how to have it: `near --type <kind>` or `near --all` asks for it by name
        // (§32.1, §32.2, §35.2). Only a relation nothing in this build fills is `unsupported`.
        if declined.contains(label) && relation::resolve_label(object_type, label).is_empty() {
            continue;
        }
        let (state, detail) = if declined.contains(label) {
            (PermissionState::Unknown, "available on request".to_owned())
        } else {
            (
                PermissionState::Unsupported,
                format!("no provider answers for the `{label}` of a {object_type}"),
            )
        };
        index.record_withheld(id, label, state, &detail);
    }
    Ok(())
}

/// The exits of the place at `id` that something has actually said an answer for: an edge under
/// that exit, or a recorded state saying why there is none (§35.2).
///
/// An exit that is in neither is one nobody has answered yet, which is not the same as one whose
/// answer is nothing. The vocabulary is the *group* — the word `look` prints and
/// `record_withheld` is keyed by — and not the `follow` label, because an edge's two ends have
/// two labels (`socket` and `owner`) and one group each (`sockets` and `process`).
fn stated_labels(session: &SpatialSessionState, id: &SpatialId) -> BTreeSet<&'static str> {
    let index = session.index();
    let Some(entry) = index.get(id) else {
        return BTreeSet::new();
    };
    let mut stated: BTreeSet<&'static str> = entry
        .edges()
        .iter()
        .filter_map(|edge| edge.group_from(id))
        .collect();
    let object_type = entry.object().object_type();
    for (label, _, _) in index.withheld(id) {
        if let Some(declared) = relation::exits_from(object_type)
            .map(|(declared, _)| declared)
            .find(|declared| *declared == label)
        {
            stated.insert(declared);
        }
    }
    stated
}

/// The confidence an edge may carry: the provider's own claim, never above what the relation
/// declares (§11.5, §41.2).
fn admissible(spec: &'static RelationSpec, confidence: ono_render::Confidence) -> Confidence {
    let claimed = Confidence::from_graph(confidence);
    if spec.confidence.admits(claimed) {
        return claimed;
    }
    // The declaration is the ceiling, not a promotion: an edge the provider called inferred stays
    // inferred even where the relation is declared exact, and one it called exact is reported at
    // the strongest the relation admits.
    match claimed {
        Confidence::Exact => Confidence::Strong,
        other => other,
    }
}

/// Every relation label the place at `id` could be followed along, with the edges behind it.
#[must_use]
pub fn edges_by_label(
    session: &SpatialSessionState,
    id: &SpatialId,
) -> BTreeMap<&'static str, Vec<RelationshipEdge>> {
    let mut found: BTreeMap<&'static str, Vec<RelationshipEdge>> = BTreeMap::new();
    let Some(entry) = session.index().get(id) else {
        return found;
    };
    let object_type = entry.object().object_type();
    for edge in entry.edges() {
        let Some(label) = edge.label_from(id) else {
            continue;
        };
        if relation::resolve_label(object_type, label).is_empty() {
            continue;
        }
        found.entry(label).or_default().push(edge.clone());
    }
    found
}

/// Every label any relation declares, for the diagnostics of §40.
#[must_use]
pub fn known_labels() -> BTreeSet<&'static str> {
    relation::labels().into_iter().collect()
}
