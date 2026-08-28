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
use ono_spatial_core::{Projection, SpatialId, SpatialObject, SpatialType};
use ono_value::{ErrorValue, RecordValue, Value};

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

/// What absorbing a batch of provider records did (§42.1).
#[derive(Debug, Clone, Default)]
pub struct Absorbed {
    added: Vec<SpatialId>,
    reconciled: Vec<SpatialId>,
    unplaced: Vec<String>,
    refused: Vec<ErrorValue>,
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
}

impl ProviderBridge {
    /// A bridge that projects into `projection`'s scope.
    #[must_use]
    pub fn new(projection: Projection) -> Self {
        Self {
            projection,
            keys: BTreeMap::new(),
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

    /// Registers every record of `records` that names a place, at `at`.
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
        }
        outcome
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
