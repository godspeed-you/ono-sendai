//! The projection of a provider value into a spatial object (spec v0.4 §3.1, §45.1).
//!
//! §2.16 is the rule this module keeps: "Providers own facts. Ono's spatial layer composes
//! provider data; it MUST NOT become an undocumented source of system truth." Nothing here
//! invents a field. A record that a provider produced becomes a `SpatialObject` by being read —
//! its identity from the fields its schema declares as identity, its provenance carried through
//! unchanged — or it does not become one at all.

use std::collections::BTreeSet;

use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_provider_api::ObjectRef;
use ono_value::{ErrorValue, Provenance, RecordValue};

use crate::{BootIdentity, IdentityTier, SpatialId, SpatialIdentity, SpatialScope, SpatialType};

/// What a spatial object can be asked to do (§3.1's `capabilities: Set<SpatialCapability>`).
///
/// These are spatial capabilities, not provider capabilities: they say what the *navigation*
/// layer may offer for the object, and they are what keeps `enter` from accepting something that
/// is not a place (`spatial.not_enterable`, §40) and keeps a tombstone from accepting an action
/// that needs a live object (§10.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SpatialCapability {
    /// `enter` accepts it: it is a place a user can stand in (§6.3).
    Enter,
    /// It has relations to traverse with `follow` (§6.4).
    Follow,
    /// Its state changes can be watched, so a live map may show it moving (§25.1).
    Watch,
    /// A mutation may target it — which the index revalidates against the provider first (§33.2).
    Act,
}

impl SpatialCapability {
    /// Every capability.
    pub const ALL: &'static [SpatialCapability] = &[
        SpatialCapability::Enter,
        SpatialCapability::Follow,
        SpatialCapability::Watch,
        SpatialCapability::Act,
    ];

    /// The name a place view or `inspect` shows.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            SpatialCapability::Enter => "enter",
            SpatialCapability::Follow => "follow",
            SpatialCapability::Watch => "watch",
            SpatialCapability::Act => "act",
        }
    }
}

/// What is known about an object's lifetime (§3.1's `lifetime: LifetimeDescriptor`).
///
/// The tier is the honest ceiling on how long the identity can be trusted (§10.1); `ended` is
/// what turns the object into a tombstone (§10.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifetimeDescriptor {
    tier: IdentityTier,
    first_observed: Timestamp,
    last_observed: Timestamp,
    ended: Option<Timestamp>,
}

impl LifetimeDescriptor {
    /// A lifetime that began being observed at `at`.
    #[must_use]
    pub fn observed(tier: IdentityTier, at: Timestamp) -> Self {
        Self {
            tier,
            first_observed: at,
            last_observed: at,
            ended: None,
        }
    }

    /// The same lifetime, seen again at `at`.
    #[must_use]
    pub fn seen_again(mut self, at: Timestamp) -> Self {
        self.last_observed = self.last_observed.max(at);
        self
    }

    /// The same lifetime, known to have ended at `at`.
    #[must_use]
    pub fn ended(mut self, at: Timestamp) -> Self {
        self.ended = Some(at);
        self
    }

    /// How far the identity can be trusted (§10.1).
    #[must_use]
    pub fn tier(&self) -> IdentityTier {
        self.tier
    }

    /// When the object was first observed.
    #[must_use]
    pub fn first_observed(&self) -> Timestamp {
        self.first_observed
    }

    /// When it was last observed.
    #[must_use]
    pub fn last_observed(&self) -> Timestamp {
        self.last_observed
    }

    /// When it ended, where that is known.
    #[must_use]
    pub fn end(&self) -> Option<Timestamp> {
        self.ended
    }

    /// Whether the object is still there as far as anyone has seen.
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.ended.is_none()
    }
}

/// An Ono value that can take part in spatial navigation (§3.1).
#[derive(Debug, Clone, PartialEq)]
pub struct SpatialObject {
    spatial_id: SpatialId,
    identity: SpatialIdentity,
    object_type: SpatialType,
    canonical_ref: ObjectRef,
    display_name: String,
    scope: SpatialScope,
    lifetime: LifetimeDescriptor,
    provenance: Provenance,
    capabilities: BTreeSet<SpatialCapability>,
}

impl SpatialObject {
    /// The object's opaque identity (§3.1).
    #[must_use]
    pub fn spatial_id(&self) -> &SpatialId {
        &self.spatial_id
    }

    /// What went into that identity, for an explanation or a conflict diagnostic (§40).
    #[must_use]
    pub fn identity(&self) -> &SpatialIdentity {
        &self.identity
    }

    /// The object's spatial type.
    #[must_use]
    pub fn object_type(&self) -> SpatialType {
        self.object_type
    }

    /// The provider's own reference to the object, which every action resolves through (§33.2).
    #[must_use]
    pub fn canonical_ref(&self) -> &ObjectRef {
        &self.canonical_ref
    }

    /// What a person calls it. "The display name is not identity" (§3.1).
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// The boundary the object belongs to (§3.2).
    #[must_use]
    pub fn scope(&self) -> &SpatialScope {
        &self.scope
    }

    /// What is known about its lifetime (§10).
    #[must_use]
    pub fn lifetime(&self) -> &LifetimeDescriptor {
        &self.lifetime
    }

    /// Where the observation came from (spec v0.2 §26).
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// What the navigation layer may offer for it.
    #[must_use]
    pub fn capabilities(&self) -> &BTreeSet<SpatialCapability> {
        &self.capabilities
    }

    /// Whether `enter` accepts it (§6.3, §40's `spatial.not_enterable`).
    #[must_use]
    pub fn is_enterable(&self) -> bool {
        self.capabilities.contains(&SpatialCapability::Enter)
    }

    /// The same object, observed again at `at`.
    #[must_use]
    pub fn seen_again(mut self, at: Timestamp) -> Self {
        self.lifetime = self.lifetime.seen_again(at);
        self
    }

    /// The same object, known to have ended at `at` (§10.3).
    #[must_use]
    pub fn ended(mut self, at: Timestamp) -> Self {
        self.lifetime = self.lifetime.ended(at);
        self.capabilities.remove(&SpatialCapability::Act);
        self.capabilities.remove(&SpatialCapability::Enter);
        self
    }
}

/// How a provider record becomes a spatial object.
///
/// The context carries what the record itself cannot: which host and boot the observation
/// belongs to, and which scope the object sits in. §10.2 needs the boot identity for a process,
/// and no `ono.process/1` record carries one.
#[derive(Debug, Clone)]
pub struct Projection {
    scope: SpatialScope,
    at: Timestamp,
}

impl Projection {
    /// A projection into `scope`, for observations made at `at`.
    #[must_use]
    pub fn new(scope: SpatialScope, at: Timestamp) -> Self {
        Self { scope, at }
    }

    /// The scope objects are projected into.
    #[must_use]
    pub fn scope(&self) -> &SpatialScope {
        &self.scope
    }

    /// The boot identity of the host the scope belongs to, or an unknown-boot identity where
    /// the caller could not read one (§10.2, §2.17).
    #[must_use]
    pub fn boot(&self) -> BootIdentity {
        self.scope
            .boot()
            .cloned()
            .unwrap_or_else(|| BootIdentity::unknown_boot(self.scope.host_scope().id()))
    }

    /// Projects `record` into a spatial object of `object_type` (§3.1).
    ///
    /// The type is the caller's to state, because a schema does not determine it: `ono.socket/1`
    /// is a [`SpatialType::Listener`] or a [`SpatialType::Connection`] depending on the socket's
    /// state, and `ono.file/1` is a [`SpatialType::Directory`] or a [`SpatialType::File`]
    /// depending on the entry. Deciding that from the record is the provider bridge's job (§45.2);
    /// inventing it here would make the spatial layer a source of truth about the object, which
    /// §2.16 forbids.
    ///
    /// # Errors
    ///
    /// - `spatial.identity_conflict` when the record's schema declares no identity: an object
    ///   whose identity cannot be read cannot be known to be the same object twice (§42.1).
    pub fn project_as(
        &self,
        record: &RecordValue,
        object_type: SpatialType,
    ) -> Result<SpatialObject, ErrorValue> {
        let canonical_ref = ObjectRef::of(record).ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::SpatialIdentityConflict,
                format!(
                    "`{}` declares no identity, so two observations of it cannot be known to be \
                     the same object",
                    record.schema().id()
                ),
            )
        })?;

        let identity = self.identity_of(object_type, record, &canonical_ref);
        let display_name = display_name(object_type, record, &canonical_ref);
        let mut capabilities = BTreeSet::new();
        capabilities.insert(SpatialCapability::Enter);
        capabilities.insert(SpatialCapability::Act);
        if crate::relation::exits_from(object_type).next().is_some() {
            capabilities.insert(SpatialCapability::Follow);
        }
        if crate::space::collection_for(object_type).is_some_and(|space| space.schema.is_some()) {
            capabilities.insert(SpatialCapability::Watch);
        }

        Ok(SpatialObject {
            spatial_id: identity.spatial_id(),
            object_type,
            canonical_ref,
            display_name,
            scope: self.scope.clone(),
            lifetime: LifetimeDescriptor::observed(identity.tier(), self.at),
            provenance: record.provenance().clone(),
            capabilities,
            identity,
        })
    }

    /// Projects a place a provider *named* inside another object's record but does not serve as
    /// a record of its own (§42.3, §16.2, §16.3, §14.4).
    ///
    /// The far end of a connection, the control group a process reports, the pid namespace it
    /// runs in: each is a fact a provider stated, and each is a place a user can stand in. None
    /// of them arrives as a record, so none of them can go through [`Projection::project_as`].
    ///
    /// `schema` and `field` say what the place would be if a provider did serve it, and `key` is
    /// the value of that field — so a derived place and a served one reduce to the **same**
    /// identity when both name the same object, which is what lets a cgroup composed from
    /// `/proc/<pid>/cgroup` reconcile with an `ono.cgroup/1` record a future provider emits
    /// (§42.1).
    ///
    /// `provenance` is the provenance of the record that named the place. The spatial layer
    /// composes; it does not observe (§2.16).
    #[must_use]
    pub fn derive(
        &self,
        object_type: SpatialType,
        schema: ono_value::SchemaId,
        field: &str,
        key: &str,
        provenance: Provenance,
    ) -> SpatialObject {
        let identity = SpatialIdentity::new(
            object_type.identity_tier(),
            object_type,
            [
                ("scope".to_owned(), self.scope_chain()),
                (field.to_owned(), key.to_owned()),
            ],
        );
        let canonical_ref = ObjectRef::derived(
            ono_provider_api::ObjectId::new(schema, [ono_value::Value::string(key)]),
            key,
            provenance.clone(),
        );
        let mut capabilities = BTreeSet::new();
        capabilities.insert(SpatialCapability::Enter);
        if crate::relation::exits_from(object_type).next().is_some() {
            capabilities.insert(SpatialCapability::Follow);
        }
        SpatialObject {
            spatial_id: identity.spatial_id(),
            object_type,
            canonical_ref,
            display_name: key.to_owned(),
            scope: self.scope.clone(),
            lifetime: LifetimeDescriptor::observed(identity.tier(), self.at),
            provenance,
            capabilities,
            identity,
        }
    }

    /// The scope chain, as the identity of every object in it spells it.
    fn scope_chain(&self) -> String {
        self.scope
            .chain()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("/")
    }

    /// Projects `record` into a spatial object, taking its type from the geography.
    ///
    /// # Errors
    ///
    /// - `spatial.unsupported` when no canonical space holds objects of the record's schema, or
    ///   when more than one does — then the type is genuinely undetermined and the caller must
    ///   say which it meant with [`Projection::project_as`].
    /// - whatever [`Projection::project_as`] returns.
    pub fn project(&self, record: &RecordValue) -> Result<SpatialObject, ErrorValue> {
        let schema = record.schema().id().to_string();
        let candidates = spatial_types_of(&schema);
        match candidates.as_slice() {
            [object_type] => self.project_as(record, *object_type),
            [] => Err(ErrorValue::new(
                ErrorCode::SpatialUnsupported,
                format!(
                    "`{schema}` is not a schema the spatial layer places; nothing in the \
                     canonical geography holds objects of that type"
                ),
            )),
            several => Err(ErrorValue::new(
                ErrorCode::SpatialUnsupported,
                format!(
                    "`{schema}` is placed as {} depending on the object, so the type cannot be \
                     read from the schema alone",
                    several
                        .iter()
                        .map(|kind| kind.as_str())
                        .collect::<Vec<_>>()
                        .join(" or ")
                ),
            )),
        }
    }

    /// The identity of one record, at the strongest tier its type honestly allows (§10.1).
    ///
    /// A process is the case §10.2 legislates: boot identity, pid, start time and pid namespace,
    /// never the pid alone. Everything else takes the fields its schema declares as identity,
    /// scoped by the boundary it was observed in — the same uid in two containers is two users.
    fn identity_of(
        &self,
        object_type: SpatialType,
        record: &RecordValue,
        canonical_ref: &ObjectRef,
    ) -> SpatialIdentity {
        if object_type == SpatialType::Process
            && let Some(Ok(pid)) = record.get("pid").map(ono_value::Value::as_int)
            && let Some(started) = record.get("started")
        {
            // §10.2: "A local Linux process identity SHOULD include host boot identity, pid,
            // process start time, pid namespace identity." The start time is carried as the
            // provider's own text for it: what matters is that it is the same for the same
            // process and different for one that merely reused the pid (§42.2).
            let start_time = ono_value::canonical_text(started).unwrap_or_default();
            let namespace = record
                .get("pid_namespace")
                .map(ono_value::Value::as_int)
                .and_then(Result::ok)
                .map_or_else(|| "unknown".to_owned(), |value: i128| value.to_string());
            return SpatialIdentity::new(
                IdentityTier::Lifetime,
                SpatialType::Process,
                [
                    ("boot".to_owned(), self.boot().as_str().to_owned()),
                    ("pid".to_owned(), pid.to_string()),
                    ("start_time".to_owned(), start_time),
                    ("pid_namespace".to_owned(), namespace),
                ],
            );
        }

        let tier = object_type.identity_tier();
        let mut components: Vec<(String, String)> = vec![("scope".to_owned(), self.scope_chain())];
        for (field, value) in record
            .schema()
            .identity()
            .iter()
            .zip(canonical_ref.id().values())
        {
            components.push((
                field.to_string(),
                ono_value::canonical_text(value).unwrap_or_else(|_| format!("{value:?}")),
            ));
        }
        SpatialIdentity::new(tier, object_type, components)
    }
}

/// What a person calls the object §3.1 keeps out of its identity.
///
/// A schema's own name for the thing comes first — `name` for a process or a service, `path` for
/// a device, `target` for a mount, `address` for a neighbour — because that is the word §12 and
/// §13 print in their own examples (`PROCESS / nginx / 1842`, `SERVICE / nginx.service`). Where a
/// schema has none of those, the provider reference's label is what is left, and it is honest:
/// spec v0.2 fixes it as the first default-view column outside the identity.
fn display_name(
    object_type: SpatialType,
    record: &RecordValue,
    canonical_ref: &ObjectRef,
) -> String {
    // §15.4 and §15.5 make a filesystem place its path, and §27.2's own picker prints
    // `/etc/nginx`: a base name cannot tell two files of the same name apart. Everywhere else the
    // schema's own name field is what a person says — a device is `sda`, not `/dev/sda`.
    let fields: &[&str] = match object_type {
        SpatialType::File | SpatialType::Directory => {
            &["path", "name", "target", "address", "destination", "id"]
        }
        _ => &["name", "path", "target", "address", "destination", "id"],
    };
    for field in fields {
        if let Some(value) = record.get(field)
            && !value.is_null()
            && let Ok(text) = ono_value::canonical_text(value)
            && !text.is_empty()
        {
            return text;
        }
    }
    // §14.3 and §14.4: a socket is called by its endpoint. Its schema has none of the fields
    // above, and the provider reference's label would be the protocol — `tcp` names every socket
    // on the host and none of them in particular.
    if let Some(endpoint) = endpoint_name(record, "local") {
        return match endpoint_name(record, "remote") {
            Some(peer) => format!("{endpoint} -> {peer}"),
            None => endpoint,
        };
    }
    canonical_ref.label().to_owned()
}

/// How an endpoint sub-record reads as a name: `127.0.0.1:443`, or the socket path.
fn endpoint_name(record: &RecordValue, field: &str) -> Option<String> {
    let endpoint = record.get(field)?.as_record().ok()?.clone();
    let text = |field: &str| {
        endpoint
            .get(field)
            .filter(|value| !value.is_null())
            .and_then(|value| ono_value::canonical_text(value).ok())
            .filter(|text| !text.is_empty())
    };
    if let Some(path) = text("path") {
        return Some(path);
    }
    let address = text("address")?;
    match text("port") {
        Some(port) => Some(format!("{address}:{port}")),
        None => Some(address),
    }
}

/// Every name the object answers to, lowercased: what a person would call it, and every value the
/// schema declares as its identity.
///
/// Discovery must not require knowing an identity (§2.1, §9), but a user who *does* know one
/// should be able to type it: `nginx.service` is both the display name of a service and the value
/// of its `name` identity field, and `1842` is a pid nobody would guess but everybody can read.
#[must_use]
pub fn aliases_of(object: &SpatialObject) -> BTreeSet<String> {
    let mut aliases = BTreeSet::new();
    aliases.insert(object.display_name().to_ascii_lowercase());
    aliases.insert(object.canonical_ref().label().to_ascii_lowercase());
    for (field, value) in object.identity().components() {
        // The scope is a boundary, not a name: a user does not navigate to `host:testbox` by
        // typing it at a place selector, and indexing it would make every object on the host
        // answer to one word.
        if field != "scope" && field != "boot" && !value.is_empty() && value != "unknown" {
            aliases.insert(value.to_ascii_lowercase());
        }
    }
    aliases.remove("");
    aliases
}

/// The spatial types a provider schema can project to, in the order the geography declares them.
///
/// Usually one; `ono.socket/1` and `ono.file/1` are the two schemas that carry more than one kind
/// of place, and for those the provider bridge says which (§14.3, §14.4, §15.4, §15.5).
#[must_use]
pub fn spatial_types_of(schema: &str) -> Vec<SpatialType> {
    let mut types: Vec<SpatialType> = crate::space::spaces()
        .iter()
        .filter(|space| space.is_served() && space.schema == Some(schema))
        .filter_map(|space| space.member_type)
        .collect();
    types.dedup();
    types
}
