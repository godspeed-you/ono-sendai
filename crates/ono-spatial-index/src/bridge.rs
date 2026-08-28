//! The provider bridge: which place a provider record *is* (spec v0.4 §42, §45.2, §50 Phase S2).
//!
//! §45.2 puts identity reconciliation in this crate, and §2.16 fixes what it may do while doing
//! it: "Providers own facts. Ono's spatial layer composes provider data; it MUST NOT become an
//! undocumented source of system truth." So this module reads records and decides nothing that
//! the record does not say.
//!
//! It exists because a schema does not determine a place. `ono.socket/1` is a
//! [`SpatialType::Listener`] or a [`SpatialType::Connection`] depending on the socket's state
//! (§14.3, §14.4); `ono.file/1` is a [`SpatialType::Directory`] or a [`SpatialType::File`]
//! depending on the entry (§15.4, §15.5); `ono.device/1` is a block device in STORAGE or a
//! character device in DEVICES depending on its kind (§7.4, §7.7). [`spatial_type_of`] is the
//! one table that decides, and it decides from the record's own fields.
//!
//! The second thing it does is *reconcile*. Two providers can answer for one object —
//! `get process` and `inspect process` produce `ono.process/1` and `ono.process-detail/1`,
//! `linux.sysfs` and the `lsblk` adapter both describe `/dev/sda2` — and §50's gate for this
//! phase is that they arrive as one place, not two. They do, because identity is built from the
//! facts that make the object that object and never from the schema that carried them.

use std::collections::BTreeMap;

use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_spatial_core::{
    Confidence, Projection, RelationSpec, RelationshipEdge, SpatialId, SpatialObject, SpatialType,
    relation,
};
use ono_value::{ErrorValue, Provenance, RecordValue, SchemaId, Value};

use crate::{Registration, SpatialIndex};

/// The spatial type a provider record projects to, or `None` where §7 gives it no place.
///
/// The decision is the record's, not the schema's, wherever a schema carries more than one kind
/// of place:
///
/// - a socket that is listening, or that has no peer to be connected to, is a
///   [`SpatialType::Listener`]; one with a peer is a [`SpatialType::Connection`] (§14.3, §14.4);
/// - a directory entry is a [`SpatialType::Directory`], every other kind of file a
///   [`SpatialType::File`] (§15.4, §15.5);
/// - a block device node belongs to STORAGE as a [`SpatialType::BlockDevice`], a character
///   device to DEVICES as a [`SpatialType::Device`] (§7.4, §7.7, §18).
///
/// `None` is the honest answer for a package, an environment variable, a log record or a DNS
/// answer: they are values in the typed shell, and §7 places none of them.
#[must_use]
pub fn spatial_type_of(record: &RecordValue) -> Option<SpatialType> {
    use SpatialType as T;
    let schema = record.schema().id().to_string();
    Some(match schema.as_str() {
        "ono.process/1" | "ono.process-detail/1" => T::Process,
        "ono.service/1" => T::Service,
        "ono.job/1" => T::Job,
        "ono.container/1" => T::Container,
        "ono.socket/1" => {
            if is_connected(record) {
                T::Connection
            } else {
                T::Listener
            }
        }
        "ono.interface/1" => T::Interface,
        "ono.interface-address/1" => T::Address,
        "ono.route/1" => T::Route,
        "ono.neighbor/1" => T::Neighbor,
        "ono.namespace/1" => T::Namespace,
        "ono.filesystem/1" => T::Filesystem,
        "ono.mount/1" => T::Mount,
        "ono.block-device/1" => T::BlockDevice,
        "ono.device/1" => {
            if text(record.get("kind")).as_deref() == Some("block") {
                T::BlockDevice
            } else {
                T::Device
            }
        }
        "ono.file/1" => {
            if text(record.get("kind")).as_deref() == Some("dir") {
                T::Directory
            } else {
                T::File
            }
        }
        "ono.user/1" => T::User,
        "ono.group/1" => T::Group,
        "ono.session/1" => T::Session,
        "ono.host/1" => T::Host,
        "ono.cgroup/1" => T::Cgroup,
        _ => return None,
    })
}

/// Whether a socket record describes a connection rather than a listener (§14.3, §14.4).
///
/// A socket in `listen` is a listener whatever else it carries. Otherwise the peer decides: a
/// socket with an endpoint at the far end is one end of a connection, and one without — a bound
/// UDP socket, a Unix socket nobody has connected to — is a place traffic arrives at, which is
/// what §14.3 calls a listener.
fn is_connected(record: &RecordValue) -> bool {
    if text(record.get("state")).as_deref() == Some("listen") {
        return false;
    }
    record
        .get("remote")
        .and_then(|value| value.as_record().ok())
        .is_some_and(|peer| {
            ["address", "path"]
                .iter()
                .any(|field| peer.get(field).is_some_and(|value| !value.is_null()))
        })
}

/// The field of an object of `object_type` that *another* record names it by.
///
/// This is not the object's identity: a process's identity is four parts (§10.2) while a socket
/// record names its owner by pid alone, and an interface's identity is its kernel index while a
/// route names it by name. The bridge needs both — the identity to know which place this is, and
/// this key to know which place a reference points at.
#[must_use]
fn reference_field(object_type: SpatialType) -> Option<&'static str> {
    use SpatialType as T;
    Some(match object_type {
        T::Process => "pid",
        T::Service | T::Host | T::Interface => "name",
        T::Job | T::Container | T::Namespace | T::Session => "id",
        T::Socket | T::Listener | T::Connection => "inode",
        T::Address | T::Neighbor => "address",
        T::Filesystem => "source",
        T::Mount => "target",
        T::BlockDevice | T::Device | T::Directory | T::File | T::Cgroup => "path",
        T::User => "uid",
        T::Group => "gid",
        _ => return None,
    })
}

/// Every key an object answers to when another record refers to it.
///
/// Usually one. An interface answers to its name and its index because `ono.route/1` carries
/// whichever the kernel could resolve, and a container answers to its full id and to the twelve
/// characters a cgroup path and a `docker ps` line show.
fn reference_keys(object_type: SpatialType, record: &RecordValue) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(field) = reference_field(object_type)
        && let Some(key) = text(record.get(field))
    {
        keys.push(key);
    }
    match object_type {
        SpatialType::Interface => keys.extend(text(record.get("index"))),
        SpatialType::Container => {
            if let Some(id) = keys.first().cloned()
                && id.len() > 12
            {
                keys.push(id[..12].to_owned());
            }
        }
        _ => {}
    }
    keys.retain(|key| !key.is_empty());
    keys
}

/// The key a reference *value* names, whether it is a whole record or the bare scalar a provider
/// could resolve.
///
/// `ono.process/1`'s `user` is a record carrying `uid` and `name`; its `service` is the unit name
/// as a string; `ono.route/1`'s `interface` is the interface's name, or its index where the name
/// could not be read. All three are references, and all three resolve here.
#[must_use]
pub fn reference_key(value: &Value, object_type: SpatialType) -> Option<String> {
    match value {
        Value::Null => None,
        Value::Record(record) => {
            let field = reference_field(object_type)?;
            text(record.get(field))
        }
        other => text(Some(other)),
    }
}

/// The canonical text of a value, where it has one and is not null.
fn text(value: Option<&Value>) -> Option<String> {
    match value {
        None | Some(Value::Null) | Some(Value::Error(_)) => None,
        Some(value) => ono_value::canonical_text(value)
            .ok()
            .filter(|text| !text.is_empty()),
    }
}

// --- the relations a record asserts ------------------------------------------------------------

/// Where the far end of an asserted relation is.
#[derive(Debug, Clone)]
enum FarEnd {
    /// A place some provider serves; the bridge looks it up by the key another record names it by.
    Known {
        object_type: SpatialType,
        key: String,
    },
    /// A place a provider named inside this record but serves no record for — the far end of a
    /// connection, a control group, a namespace (§42.3, §16.2, §16.3).
    Composed {
        object_type: SpatialType,
        schema: &'static str,
        field: &'static str,
        key: String,
    },
}

/// One relation a record asserted, from the object the record described to somewhere else.
#[derive(Debug, Clone)]
struct Fact {
    subject: SpatialId,
    /// Whether the record's own object is the relation's declared source or its target.
    subject_is_source: bool,
    relation: &'static RelationSpec,
    far: FarEnd,
    confidence: Confidence,
    provenance: Provenance,
    attributes: Vec<(&'static str, Value)>,
}

/// The directory the Unix path tree puts an object inside, waiting for that directory to be seen.
#[derive(Debug, Clone)]
struct PathParent {
    child: SpatialId,
    parent_path: String,
}

/// What absorbing a batch of provider records did (§42.1).
#[derive(Debug, Clone, Default)]
pub struct Absorbed {
    added: Vec<SpatialId>,
    reconciled: Vec<SpatialId>,
    unplaced: Vec<String>,
    refused: Vec<ErrorValue>,
    edges: usize,
}

impl Absorbed {
    /// The objects the index did not hold before.
    #[must_use]
    pub fn added(&self) -> &[SpatialId] {
        &self.added
    }

    /// The objects that were already there and were recognised as the same place — §42.1's
    /// identity test holding across two observations, or across two providers.
    #[must_use]
    pub fn reconciled(&self) -> &[SpatialId] {
        &self.reconciled
    }

    /// The schemas in the batch that name no place, each counted once (§7).
    ///
    /// Not an error: a package and a log record are values, and a batch that contains them is a
    /// pipeline the user asked for, not a fault.
    #[must_use]
    pub fn unplaced(&self) -> &[String] {
        &self.unplaced
    }

    /// The records that name a place but could not become one — an identity the schema does not
    /// declare, or an identity conflict (§40).
    #[must_use]
    pub fn refused(&self) -> &[ErrorValue] {
        &self.refused
    }

    /// How many objects the batch put into the index, new or reconciled.
    #[must_use]
    pub fn registered(&self) -> usize {
        self.added.len() + self.reconciled.len()
    }

    /// How many relationship edges the batch settled — including facts an earlier batch asserted
    /// whose far end only arrived now.
    #[must_use]
    pub fn edges(&self) -> usize {
        self.edges
    }
}

/// The bridge from one provider scope's records into the spatial index (§45.2, §50 Phase S2).
///
/// It holds the [`Projection`] for the scope it reads and the reference table that lets one
/// record's mention of another become an edge between two places. It holds no facts of its own:
/// every entry in that table came from a record a provider produced.
#[derive(Debug)]
pub struct ProviderBridge {
    projection: Projection,
    keys: BTreeMap<(SpatialType, String), SpatialId>,
    /// Relations asserted by a record whose far end nobody had observed yet. Discovery is not
    /// ordered — sockets can be listed before processes — and §42.3 forbids an edge to an
    /// unknown id, so the assertion waits here rather than becoming a dangling edge or being
    /// lost.
    pending: Vec<Fact>,
    /// The same, for the enclosing directory of the Unix path tree (§15.1).
    pending_paths: Vec<PathParent>,
}

impl ProviderBridge {
    /// A bridge that projects into `projection`'s scope.
    #[must_use]
    pub fn new(projection: Projection) -> Self {
        Self {
            projection,
            keys: BTreeMap::new(),
            pending: Vec::new(),
            pending_paths: Vec::new(),
        }
    }

    /// The projection objects are read into.
    #[must_use]
    pub fn projection(&self) -> &Projection {
        &self.projection
    }

    /// Projects one record into the place it is (§3.1).
    ///
    /// # Errors
    ///
    /// - `spatial.unsupported` when no canonical domain holds objects of the record's schema;
    /// - whatever [`Projection::project_as`] returns for a record whose identity cannot be read.
    pub fn project(&self, record: &RecordValue) -> Result<SpatialObject, ErrorValue> {
        let object_type = spatial_type_of(record).ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::SpatialUnsupported,
                format!(
                    "`{}` is not a schema the spatial layer places; the six domains of spec v0.4 \
                     §7 hold no objects of that kind",
                    record.schema().id()
                ),
            )
        })?;
        self.projection.project_as(record, object_type)
    }

    /// Registers every record of `records` that names a place, then records the relations those
    /// records assert between places the index holds (§42.1, §42.3).
    ///
    /// Registration comes first and relations second, so that two records of one batch that name
    /// each other both land. An assertion whose far end nobody has observed is kept rather than
    /// dropped: discovery is not ordered, and the edge is made the moment the far end arrives.
    ///
    /// Records that name no place are counted and skipped; records that name one and cannot
    /// become one are refused with the provider's own diagnostic rather than dropped, because a
    /// place that silently failed to exist is indistinguishable from one that is not there.
    pub fn absorb(
        &mut self,
        index: &mut SpatialIndex,
        records: &[RecordValue],
        at: Timestamp,
    ) -> Absorbed {
        let mut outcome = Absorbed::default();
        let mut facts = std::mem::take(&mut self.pending);
        let mut paths = std::mem::take(&mut self.pending_paths);

        for record in records {
            let Some(object_type) = spatial_type_of(record) else {
                let schema = record.schema().id().to_string();
                if !outcome.unplaced.contains(&schema) {
                    outcome.unplaced.push(schema);
                }
                continue;
            };
            let object = match self.projection.project_as(record, object_type) {
                Ok(object) => object,
                Err(error) => {
                    outcome.refused.push(error);
                    continue;
                }
            };
            let id = object.spatial_id().clone();
            match index.register(object, at) {
                Ok(Registration::Added) => outcome.added.push(id.clone()),
                Ok(Registration::Reconciled) => outcome.reconciled.push(id.clone()),
                Err(error) => {
                    outcome.refused.push(error);
                    continue;
                }
            }
            for key in reference_keys(object_type, record) {
                self.keys.insert((object_type, key), id.clone());
            }
            facts.extend(facts_of(&id, object_type, record));
            if let Some(parent) = enclosing_directory(object_type, record) {
                paths.push(PathParent {
                    child: id,
                    parent_path: parent,
                });
            }
        }

        // Composed places first: an edge to one of them can always be settled, and a place the
        // index does not hold cannot be an edge's end (§42.3).
        for fact in &facts {
            if let FarEnd::Composed {
                object_type,
                schema,
                field,
                key,
            } = &fact.far
                && self.resolve(*object_type, key).is_none()
            {
                let Ok(schema) = schema.parse::<SchemaId>() else {
                    continue;
                };
                let place = self.projection.derive(
                    *object_type,
                    schema,
                    field,
                    key,
                    fact.provenance.clone(),
                );
                let id = place.spatial_id().clone();
                match index.register(place, at) {
                    Ok(Registration::Added) => outcome.added.push(id.clone()),
                    Ok(Registration::Reconciled) => outcome.reconciled.push(id.clone()),
                    Err(error) => {
                        outcome.refused.push(error);
                        continue;
                    }
                }
                self.keys.insert((*object_type, key.clone()), id);
            }
        }

        for fact in facts {
            match self.settle(&fact, at) {
                Some(edge) => {
                    index.record_edge(edge);
                    outcome.edges += 1;
                }
                // The far end is still unobserved, and the subject is still a place: keep the
                // assertion for the batch that brings the other half.
                None if index.contains(&fact.subject) => self.pending.push(fact),
                None => {}
            }
        }
        for path in paths {
            let resolved = self
                .resolve(SpatialType::Directory, &path.parent_path)
                .cloned();
            match resolved {
                Some(parent) if index.set_path_parent(&path.child, &parent) => {}
                _ if index.contains(&path.child) => self.pending_paths.push(path),
                _ => {}
            }
        }
        outcome
    }

    /// The edge a fact asserts, once both its ends are places the index holds.
    fn settle(&self, fact: &Fact, at: Timestamp) -> Option<RelationshipEdge> {
        let far = match &fact.far {
            FarEnd::Known { object_type, key }
            | FarEnd::Composed {
                object_type, key, ..
            } => self.resolve(*object_type, key)?,
        };
        let (source, target) = if fact.subject_is_source {
            (fact.subject.clone(), far.clone())
        } else {
            (far.clone(), fact.subject.clone())
        };
        let mut edge = RelationshipEdge::new(
            source,
            target,
            fact.relation.relation_type(),
            fact.confidence,
            fact.provenance.clone(),
            at,
        );
        for (key, value) in &fact.attributes {
            edge = edge.with_attribute(key, value.clone());
        }
        Some(edge)
    }

    /// The place of `object_type` that another record's reference to `key` names, where the
    /// index already holds it.
    ///
    /// `None` is the answer for a reference to something nobody has observed yet — a socket whose
    /// owning process was not in this batch. §42.3 makes that the right answer: an edge to an
    /// unknown id is a dangling edge, and the bridge would rather have no edge than one.
    #[must_use]
    pub fn resolve(&self, object_type: SpatialType, key: &str) -> Option<&SpatialId> {
        self.keys
            .get(&(object_type, key.to_owned()))
            .or_else(|| {
                object_type
                    .generalises_to()
                    .and_then(|general| self.keys.get(&(general, key.to_owned())))
            })
            .or_else(|| {
                // The reference names the general type — `process.owns_socket` runs to a
                // `Socket` — and the place is the specialised one it actually is (§14.3).
                SpatialType::ALL
                    .iter()
                    .filter(|kind| kind.is_a(object_type) && **kind != object_type)
                    .find_map(|kind| self.keys.get(&(*kind, key.to_owned())))
            })
    }
}

/// The relation `id` must be declared, and every fact below names one that is.
///
/// A `RelationSpec` exists only for a relation `docs/spec/spatial/relations.yaml` declares, so a
/// fact that could not find one is a fact the bridge does not assert (§2.5: every edge is
/// explainable, starting with being declared).
fn declared(id: &'static str) -> Option<&'static RelationSpec> {
    relation::spec(id)
}

/// Everything the record `record` says about how its object relates to other places.
///
/// Every entry is a fact the *provider* stated: a parent pid, a unit name, a uid, an open
/// descriptor, a cgroup path, a peer endpoint. §2.16 allows the spatial layer to compose those
/// into a graph and nothing else — so the confidence is `exact` wherever the record states the
/// relation outright, and weaker only where the join is evidence rather than an observation.
fn facts_of(id: &SpatialId, object_type: SpatialType, record: &RecordValue) -> Vec<Fact> {
    let mut facts = Vec::new();
    let provenance = record.provenance().clone();
    let mut push = |relation: &'static str,
                    subject_is_source: bool,
                    far: FarEnd,
                    confidence: Confidence,
                    attributes: Vec<(&'static str, Value)>| {
        if let Some(spec) = declared(relation) {
            facts.push(Fact {
                subject: id.clone(),
                subject_is_source,
                relation: spec,
                far,
                confidence,
                provenance: provenance.clone(),
                attributes,
            });
        }
    };
    let known = |object_type: SpatialType, key: String| FarEnd::Known { object_type, key };

    match object_type {
        SpatialType::Process => {
            if let Some(key) = record.get("ppid").and_then(|value| text(Some(value))) {
                // The record's object is the *child*, and `process.parent_of` runs from parent
                // to child, so the subject is the relation's target.
                push(
                    "process.parent_of",
                    false,
                    known(SpatialType::Process, key),
                    Confidence::Exact,
                    Vec::new(),
                );
            }
            if let Some(key) = reference_of(record, "service", SpatialType::Service) {
                push(
                    "service.controls_process",
                    false,
                    known(SpatialType::Service, key),
                    Confidence::Exact,
                    Vec::new(),
                );
            }
            if let Some(key) = reference_of(record, "user", SpatialType::User) {
                push(
                    "user.owns_process",
                    false,
                    known(SpatialType::User, key),
                    Confidence::Exact,
                    Vec::new(),
                );
            }
            if let Some(key) = reference_of(record, "container", SpatialType::Container) {
                // The provider stated it outright, so this half is an observation.
                push(
                    "container.contains_process",
                    false,
                    known(SpatialType::Container, key),
                    Confidence::Exact,
                    Vec::new(),
                );
            }
            if let Some(key) = record
                .get("pid_namespace")
                .and_then(|value| text(Some(value)))
            {
                push(
                    "process.in_namespace",
                    true,
                    FarEnd::Composed {
                        object_type: SpatialType::Namespace,
                        schema: "ono.namespace/1",
                        field: "id",
                        key,
                    },
                    Confidence::Exact,
                    Vec::new(),
                );
            }
            if let Some(path) = record.get("cgroup").and_then(|value| text(Some(value))) {
                push(
                    "process.member_of_cgroup",
                    true,
                    FarEnd::Composed {
                        object_type: SpatialType::Cgroup,
                        schema: "ono.cgroup/1",
                        field: "path",
                        key: path.clone(),
                    },
                    Confidence::Exact,
                    Vec::new(),
                );
                if let Some(container) = container_id_in(&path) {
                    // §11.5: the kernel does not report container membership. The runtime id in
                    // the cgroup path leaves no serious alternative, which is `strong`, and the
                    // path travels with the edge as the evidence §11.4 requires.
                    push(
                        "container.contains_process",
                        false,
                        known(SpatialType::Container, container),
                        Confidence::Strong,
                        vec![("evidence", Value::string(&path))],
                    );
                }
            }
            for path in list_of(record, "open_files") {
                push(
                    "process.opened_file",
                    true,
                    known(SpatialType::File, path),
                    Confidence::Exact,
                    Vec::new(),
                );
            }
        }
        SpatialType::Socket | SpatialType::Listener | SpatialType::Connection => {
            if let Some(key) = reference_of(record, "process", SpatialType::Process) {
                push(
                    "process.owns_socket",
                    false,
                    known(SpatialType::Process, key),
                    Confidence::Exact,
                    Vec::new(),
                );
            }
            if let Some(peer) = endpoint_name(record, "remote") {
                push(
                    "socket.connected_to",
                    true,
                    FarEnd::Composed {
                        object_type: SpatialType::Endpoint,
                        schema: "ono.endpoint/1",
                        field: "endpoint",
                        key: peer,
                    },
                    Confidence::Exact,
                    Vec::new(),
                );
            }
        }
        SpatialType::File | SpatialType::Directory => {
            if let Some(key) = reference_of(record, "owner", SpatialType::User) {
                push(
                    "user.owns_file",
                    false,
                    known(SpatialType::User, key),
                    Confidence::Exact,
                    Vec::new(),
                );
            }
        }
        SpatialType::Filesystem => {
            if let Some(target) = record.get("target").and_then(|value| text(Some(value))) {
                push(
                    "filesystem.mounted_at",
                    true,
                    known(SpatialType::Mount, target),
                    Confidence::Exact,
                    Vec::new(),
                );
            }
            if let Some(source) = record.get("source").and_then(|value| text(Some(value))) {
                // The filesystem's source and the device node are the same string because the
                // kernel spells them the same way; that is a join, not an observation (§11.5).
                push(
                    "device.backs_filesystem",
                    false,
                    known(SpatialType::BlockDevice, source.clone()),
                    Confidence::Strong,
                    vec![("evidence", Value::string(&source))],
                );
            }
        }
        SpatialType::Mount => {
            if let Some(target) = record.get("target").and_then(|value| text(Some(value))) {
                push(
                    "mount.backs_directory",
                    true,
                    known(SpatialType::Directory, target),
                    Confidence::Exact,
                    Vec::new(),
                );
            }
        }
        SpatialType::Address => {
            if let Some(key) = reference_of(record, "interface", SpatialType::Interface) {
                push(
                    "interface.has_address",
                    false,
                    known(SpatialType::Interface, key),
                    Confidence::Exact,
                    Vec::new(),
                );
            }
        }
        SpatialType::Route => {
            if let Some(key) = reference_of(record, "interface", SpatialType::Interface) {
                push(
                    "route.via_interface",
                    true,
                    known(SpatialType::Interface, key),
                    Confidence::Exact,
                    Vec::new(),
                );
            }
        }
        SpatialType::User => {
            if let Some(key) = reference_of(record, "primary_group", SpatialType::Group) {
                push(
                    "user.member_of_group",
                    true,
                    known(SpatialType::Group, key),
                    Confidence::Exact,
                    Vec::new(),
                );
            }
        }
        _ => {}
    }
    facts
}

/// The value of a reference field, as the key the far end answers to.
fn reference_of(record: &RecordValue, field: &str, object_type: SpatialType) -> Option<String> {
    reference_key(record.get(field)?, object_type)
}

/// The elements of a list field, as text.
fn list_of(record: &RecordValue, field: &str) -> Vec<String> {
    match record.get(field) {
        Some(Value::List(items)) => items.iter().filter_map(|item| text(Some(item))).collect(),
        _ => Vec::new(),
    }
}

/// How an endpoint sub-record names the place at the far end of a connection (§14.4, §42.3).
///
/// `10.0.0.5:5432` for an internet peer, the socket path for a Unix one. The far end is a place
/// even where Ono cannot say which host it is, which is what §42.3 calls an explicit unresolved
/// endpoint object.
fn endpoint_name(record: &RecordValue, field: &str) -> Option<String> {
    let endpoint = record.get(field)?.as_record().ok()?;
    if let Some(path) = text(endpoint.get("path")) {
        return Some(path);
    }
    let address = text(endpoint.get("address"))?;
    Some(match text(endpoint.get("port")) {
        Some(port) => format!("{address}:{port}"),
        None => address,
    })
}

/// The container runtime id a control-group path names, where it names one (§16.1).
///
/// Docker writes `/docker/<id>` or `…/docker-<id>.scope`, Podman `…/libpod-<id>.scope`,
/// Kubernetes `…/cri-containerd-<id>.scope` and `…/crio-<id>.scope`. All of them end in the
/// runtime's own 64-character container id, which is the identity `ono.container/1` carries — so
/// the join is to the engine's own id and never to a name a user can change.
fn container_id_in(cgroup: &str) -> Option<String> {
    cgroup.rsplit('/').find_map(|segment| {
        let segment = segment.strip_suffix(".scope").unwrap_or(segment);
        let candidate = segment.rsplit('-').next().unwrap_or(segment);
        (candidate.len() == 64 && candidate.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| candidate.to_owned())
    })
}

/// The path of the directory the Unix path tree puts a path object inside (§15.1).
///
/// `None` for anything that is not a path object, and for `/`, which has no enclosing directory.
fn enclosing_directory(object_type: SpatialType, record: &RecordValue) -> Option<String> {
    if !matches!(object_type, SpatialType::File | SpatialType::Directory) {
        return None;
    }
    let path = text(record.get("path"))?;
    let parent = std::path::Path::new(&path).parent()?;
    let parent = parent.to_str()?;
    (parent != path && !parent.is_empty()).then(|| parent.to_owned())
}
