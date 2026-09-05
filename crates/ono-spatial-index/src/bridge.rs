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
    Confidence, PermissionState, Projection, RelationSpec, RelationshipEdge, SpatialId,
    SpatialObject, SpatialType, relation,
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
            if is_released(record) {
                return None;
            }
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

/// Whether an adapted record carries the identity the canonical provider composes (§37.1, §10.2).
///
/// An adapter reads a tool's output, so it carries what the tool prints. Where that is the whole
/// identity — `ip link` prints an interface's index, `findmnt` a mount's target — the adapted
/// observation reduces to exactly the [`SpatialId`] the canonical provider's record reduces to,
/// and the two are one place with two sources. Where it is not, the adapted object would be a
/// second place for one object, which is the duplicate §37.1 forbids, and it stays a typed value
/// in the pipeline until something asks the canonical provider for it.
///
/// A process is the case that makes the rule: §10.2 composes its identity from the boot, the
/// pid, the start time *and* the pid namespace, and `ps` prints neither the namespace nor a
/// start time at the precision `/proc` reports.
#[must_use]
pub fn carries_full_identity(record: &RecordValue) -> bool {
    let complete = |fields: &[&str]| {
        fields
            .iter()
            .all(|field| record.get(field).is_some_and(|value| !value.is_null()))
    };
    match record.schema().id().to_string().as_str() {
        "ono.process/1" | "ono.process-detail/1" => complete(&["pid", "started", "pid_namespace"]),
        _ => record
            .schema()
            .identity()
            .iter()
            .all(|field| record.get(field).is_some_and(|value| !value.is_null())),
    }
}

/// Whether the kernel has already released the socket, so no connection stands there any more
/// (§14.4, §10.3, ADR-0192).
///
/// `time-wait` and `close` are the two states in which no application holds the connection: the
/// kernel keeps the 2MSL remnant so a late duplicate segment cannot be mistaken for new data,
/// and `ono-provider-netlink` already refuses to act on one because "a socket in time-wait has
/// already been released". Such a remnant has no owner, no inode and nothing a user can do at
/// it, so §7 gives it no place — it is the kernel's own tombstone of a connection that ended,
/// not a place the connection still occupies.
fn is_released(record: &RecordValue) -> bool {
    matches!(
        text(record.get("state")).as_deref(),
        Some("time-wait" | "close")
    )
}

/// Whether a socket record describes a connection rather than a listener (§14.3, §14.4).
///
/// The kernel's own account comes first: a socket in `listen` is a listener, and a socket in any
/// of the states a connection passes through is a connection, whatever else the record carries.
/// For the protocols that have no state — UDP, a Unix datagram socket — the peer decides: one
/// with an endpoint at the far end is one end of a connection, and one without is a place traffic
/// arrives at, which is what §14.3 calls a listener.
fn is_connected(record: &RecordValue) -> bool {
    match text(record.get("state")).as_deref() {
        Some("listen") => return false,
        Some(
            "established" | "syn-sent" | "syn-recv" | "fin-wait-1" | "fin-wait-2" | "time-wait"
            | "close" | "close-wait" | "last-ack" | "closing",
        ) => return true,
        _ => {}
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
        // §14.3 and §14.4: a connection names the listener it was accepted by through the local
        // endpoint they share, and nothing else in the kernel's socket table joins the two. The
        // key is namespaced so it can only ever be matched by that join.
        SpatialType::Listener => {
            if let Some(endpoint) = endpoint_name(record, "local") {
                keys.push(format!("listener@{endpoint}"));
                if let Some(port) = endpoint.rsplit(':').next()
                    && endpoint
                        .strip_suffix(&format!(":{port}"))
                        .is_some_and(is_wildcard)
                {
                    keys.push(format!("listener@*:{port}"));
                }
            }
        }
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
            for alias in aliases_from(object_type, record) {
                index.add_alias(&id, &alias);
            }
            let asserted = facts_of(&id, object_type, record);
            facts.extend(asserted.facts);
            for refusal in asserted.withheld {
                index.record_withheld(
                    &refusal.subject,
                    refusal.label,
                    refusal.state,
                    &refusal.detail,
                );
            }
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

/// One exit a provider could not read, and what the user must be told instead (§35.2, §42.4).
#[derive(Debug, Clone)]
struct Withheld {
    subject: SpatialId,
    label: &'static str,
    state: PermissionState,
    detail: String,
}

/// What one record said about its object's relations, and what it could not say.
///
/// The two halves are separate because they answer different questions: no fact means the object
/// has no such neighbour, while a refusal means nobody knows whether it has any (§35.2).
#[derive(Debug, Default)]
struct Assertions {
    facts: Vec<Fact>,
    withheld: Vec<Withheld>,
}

/// Reads one record's relations, keeping refusals apart from absences.
struct Reader<'a> {
    subject: &'a SpatialId,
    record: &'a RecordValue,
    provenance: Provenance,
    out: Assertions,
}

impl Reader<'_> {
    /// Whether `field` holds a refusal rather than a value, recording it against `label` if so.
    ///
    /// §35.2's states are what a provider's own error becomes here: a field carrying an
    /// `ono.error/1` is the provider saying "I could not read this", and turning that into an
    /// absent neighbour is exactly the false empty collection §42.4 forbids.
    fn refused(&mut self, field: &str, label: &'static str) -> bool {
        let Some(Value::Error(error)) = self.record.get(field) else {
            return false;
        };
        self.out.withheld.push(Withheld {
            subject: self.subject.clone(),
            label,
            state: PermissionState::of_refusal(error),
            detail: error.message().to_owned(),
        });
        true
    }

    /// Records a relation to a place some provider serves, named by `field`.
    fn relate(
        &mut self,
        field: &str,
        label: &'static str,
        far_type: SpatialType,
        relation: &'static str,
        subject_is_source: bool,
    ) {
        if self.refused(field, label) {
            return;
        }
        let Some(key) = self.record.get(field).and_then(|value| {
            if field == "ppid" {
                text(Some(value))
            } else {
                reference_key(value, far_type)
            }
        }) else {
            return;
        };
        self.push(
            relation,
            subject_is_source,
            FarEnd::Known {
                object_type: far_type,
                key,
            },
            Confidence::Exact,
            Vec::new(),
        );
    }

    /// Records a relation the bridge asserts, once its far end is named.
    fn push(
        &mut self,
        relation: &'static str,
        subject_is_source: bool,
        far: FarEnd,
        confidence: Confidence,
        attributes: Vec<(&'static str, Value)>,
    ) {
        // A `RelationSpec` exists only for a relation `docs/contracts/spatial/relations.yaml`
        // declares, so a relation nobody wrote down cannot become an edge (§2.5).
        if let Some(spec) = relation::spec(relation) {
            self.out.facts.push(Fact {
                subject: self.subject.clone(),
                subject_is_source,
                relation: spec,
                far,
                confidence,
                provenance: self.provenance.clone(),
                attributes,
            });
        }
    }
}

/// Everything the record `record` says about how its object relates to other places, and every
/// exit it could not read.
///
/// Every entry is a fact the *provider* stated: a parent pid, a unit name, a uid, an open
/// descriptor, a cgroup path, a peer endpoint. §2.16 allows the spatial layer to compose those
/// into a graph and nothing else — so the confidence is `exact` wherever the record states the
/// relation outright, and weaker only where the join is evidence rather than an observation.
fn facts_of(id: &SpatialId, object_type: SpatialType, record: &RecordValue) -> Assertions {
    let mut reader = Reader {
        subject: id,
        record,
        provenance: record.provenance().clone(),
        out: Assertions::default(),
    };

    match object_type {
        SpatialType::Process => {
            // The record's object is the *child*, and `process.parent_of` runs from parent to
            // child, so the subject is the relation's target.
            reader.relate(
                "ppid",
                "parent",
                SpatialType::Process,
                "process.parent_of",
                false,
            );
            reader.relate(
                "service",
                "service",
                SpatialType::Service,
                "service.controls_process",
                false,
            );
            reader.relate(
                "user",
                "user",
                SpatialType::User,
                "process.run_by_user",
                true,
            );
            // Where the provider states the container outright, this half is an observation.
            reader.relate(
                "container",
                "container",
                SpatialType::Container,
                "container.contains_process",
                false,
            );
            if !reader.refused("pid_namespace", "namespaces")
                && let Some(key) = text(record.get("pid_namespace"))
            {
                reader.push(
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
            if !reader.refused("cgroup", "cgroup")
                && let Some(path) = text(record.get("cgroup"))
            {
                reader.push(
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
                    reader.push(
                        "container.contains_process",
                        false,
                        FarEnd::Known {
                            object_type: SpatialType::Container,
                            key: container,
                        },
                        Confidence::Strong,
                        vec![("evidence", Value::string(&path))],
                    );
                }
            }
            if !reader.refused("open_files", "files") {
                for path in list_of(record, "open_files") {
                    reader.push(
                        "process.opened_file",
                        true,
                        FarEnd::Known {
                            object_type: SpatialType::File,
                            key: path,
                        },
                        Confidence::Exact,
                        Vec::new(),
                    );
                }
            }
        }
        SpatialType::Socket | SpatialType::Listener | SpatialType::Connection => {
            reader.relate(
                "process",
                "owner",
                SpatialType::Process,
                "process.owns_socket",
                false,
            );
            // §14.4: an established socket whose local endpoint is a listening socket's is the
            // connection that listener accepted. The kernel states both sockets and never states
            // the acceptance, so the edge is `strong` with the shared endpoint as its evidence
            // (§11.5) — never `exact`, which would claim an observation nobody made.
            if object_type == SpatialType::Connection
                && let Some(local) = endpoint_name(record, "local")
            {
                let port = local.rsplit(':').next().unwrap_or_default().to_owned();
                let mut keys = vec![format!("listener@{local}")];
                if !port.is_empty() {
                    keys.push(format!("listener@*:{port}"));
                }
                for key in keys {
                    reader.push(
                        "socket.accepts_connection",
                        false,
                        FarEnd::Known {
                            object_type: SpatialType::Listener,
                            key,
                        },
                        Confidence::Strong,
                        vec![("evidence", Value::string(&local))],
                    );
                }
            }
            if !reader.refused("remote", "peer")
                && let Some(peer) = endpoint_name(record, "remote")
            {
                reader.push(
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
            reader.relate("owner", "owner", SpatialType::User, "user.owns_file", false);
        }
        SpatialType::Filesystem => {
            if !reader.refused("target", "mounts")
                && let Some(target) = text(record.get("target"))
            {
                reader.push(
                    "filesystem.mounted_at",
                    true,
                    FarEnd::Known {
                        object_type: SpatialType::Mount,
                        key: target,
                    },
                    Confidence::Exact,
                    Vec::new(),
                );
            }
            if !reader.refused("source", "device")
                && let Some(source) = text(record.get("source"))
            {
                // The filesystem's source and the device node are the same string because the
                // kernel spells them the same way; that is a join, not an observation (§11.5).
                reader.push(
                    "device.backs_filesystem",
                    false,
                    FarEnd::Known {
                        object_type: SpatialType::BlockDevice,
                        key: source.clone(),
                    },
                    Confidence::Strong,
                    vec![("evidence", Value::string(&source))],
                );
            }
        }
        SpatialType::Mount => {
            if !reader.refused("target", "directory")
                && let Some(target) = text(record.get("target"))
            {
                reader.push(
                    "mount.backs_directory",
                    true,
                    FarEnd::Known {
                        object_type: SpatialType::Directory,
                        key: target,
                    },
                    Confidence::Exact,
                    Vec::new(),
                );
            }
        }
        SpatialType::Address => {
            reader.relate(
                "interface",
                "interface",
                SpatialType::Interface,
                "interface.has_address",
                false,
            );
        }
        SpatialType::Route => {
            reader.relate(
                "interface",
                "interface",
                SpatialType::Interface,
                "route.via_interface",
                true,
            );
        }
        SpatialType::User => {
            reader.relate(
                "primary_group",
                "groups",
                SpatialType::Group,
                "user.member_of_group",
                true,
            );
        }
        _ => {}
    }
    reader.out
}

/// The other names a record answers to, beyond the ones [`ono_spatial_core::aliases_of`] derives
/// from the object itself (§27.1's "exact", §33.1's "display names and aliases").
///
/// Every one of them is a name the provider already stated. The kernel truncates a process's
/// `comm` to fifteen characters, so `ono-spatial-twin` is `ono-spatial-twi` there and the name a
/// user types is in the command line instead; a socket is called by its endpoint or its bare
/// port; a file answers to its path and to its base name.
fn aliases_from(object_type: SpatialType, record: &RecordValue) -> Vec<String> {
    let mut aliases = Vec::new();
    match object_type {
        SpatialType::Process => {
            if let Some(Value::List(command)) = record.get("command")
                && let Some(program) = command.first().and_then(|value| text(Some(value)))
                // `argv[0]` is memory the process owns, and a process may write anything into
                // it: OpenSSH puts `sshd-session: william@pts/1` there, PostgreSQL puts
                // `postgres: writer process`. That is a status line rather than a path, and its
                // last slash-separated segment is not a program name — `pts/1` gave an ssh
                // session the alias `1`, beside pid 1, so `enter process/1` named two places and
                // refused on any host with a login. A path a kernel executed carries no
                // whitespace; anything that does is a line the process wrote about itself.
                && !program.chars().any(char::is_whitespace)
            {
                aliases.push(base_name(&program));
            }
            if let Some(executable) = text(record.get("executable")) {
                aliases.push(base_name(&executable));
            }
        }
        SpatialType::Socket | SpatialType::Listener | SpatialType::Connection => {
            if let Some(local) = endpoint_name(record, "local") {
                if let Some(port) = local.rsplit(':').next()
                    && port.chars().all(|character| character.is_ascii_digit())
                    && !port.is_empty()
                {
                    aliases.push(port.to_owned());
                    aliases.push(format!(":{port}"));
                }
                aliases.push(local);
            }
        }
        SpatialType::File | SpatialType::Directory => {
            if let Some(path) = text(record.get("path")) {
                aliases.push(base_name(&path));
                aliases.push(path);
            }
        }
        _ => {}
    }
    aliases.retain(|alias| !alias.is_empty());
    aliases
}

/// The last segment of a path, or the whole text where it has none.
fn base_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
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

/// Whether an address is the kernel's "any address" for its family (§14.3).
fn is_wildcard(address: &str) -> bool {
    matches!(address, "0.0.0.0" | "::" | "*" | "")
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
