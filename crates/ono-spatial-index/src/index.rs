//! The spatial index (spec v0.4 §33, §45.2).
//!
//! §33.2 is the whole design in two sentences: "The index is a cache. Providers remain
//! authoritative. Actions MUST resolve/revalidate live objects before mutation." Everything here
//! is derived from what a provider said; nothing here is a fact of its own, and the one operation
//! that could make it look like one — resolving a target for a mutation — refuses rather than
//! answering from a stale entry.
//!
//! §33.1 lists what the index holds, and [`IndexEntry`] holds exactly that: the id, the canonical
//! object reference, display names and aliases, scope, object type, canonical parent, the known
//! relationship summary, freshness and landmark state.

use std::collections::{BTreeMap, BTreeSet};

use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_provider_api::ObjectRef;
use ono_spatial_core::{
    CostClass, Freshness, HierarchicalEdge, Landmark, NeighborhoodGroup, PermissionState,
    RelationshipEdge, SpatialId, SpatialObject, SpatialType, canonical_parent_with, relation,
};
use ono_value::ErrorValue;

use crate::FreshnessPolicy;

/// One object the index knows about (§33.1).
#[derive(Debug, Clone)]
pub struct IndexEntry {
    object: SpatialObject,
    aliases: BTreeSet<String>,
    canonical_parent: Option<HierarchicalEdge>,
    path_parent: Option<SpatialId>,
    edges: Vec<RelationshipEdge>,
    withheld: BTreeMap<String, (PermissionState, String)>,
    landmarks: Vec<Landmark>,
    sources: BTreeSet<String>,
    /// The provider whose record `object` currently holds, so an adapted observation can be
    /// told from a canonical one without re-reading the provenance (§37.1).
    sources_of_record: String,
    observed_at: Timestamp,
    subscribed: bool,
}

impl IndexEntry {
    /// The object as the provider described it.
    #[must_use]
    pub fn object(&self) -> &SpatialObject {
        &self.object
    }

    /// The provider's own reference, which every action resolves through (§33.2).
    #[must_use]
    pub fn canonical_ref(&self) -> &ObjectRef {
        self.object.canonical_ref()
    }

    /// Every name this object answers to, lowercased (§33.1).
    #[must_use]
    pub fn aliases(&self) -> &BTreeSet<String> {
        &self.aliases
    }

    /// Where `up` goes from here (§11.3).
    #[must_use]
    pub fn canonical_parent(&self) -> Option<&HierarchicalEdge> {
        self.canonical_parent.as_ref()
    }

    /// Every relationship edge known about the object.
    #[must_use]
    pub fn edges(&self) -> &[RelationshipEdge] {
        &self.edges
    }

    /// What deserves attention about it (§26).
    #[must_use]
    pub fn landmarks(&self) -> &[Landmark] {
        &self.landmarks
    }

    /// Every provider that has observed this object, canonical and adapted alike (§37.1).
    ///
    /// §37.1 reconciles an adapted object with its canonical twin into one place and keeps
    /// "both objects … with provenance": the place is one, and what saw it is a set. A reader
    /// who wants to know that `ip link` and the netlink provider agreed about `lo` finds both
    /// names here.
    #[must_use]
    pub fn sources(&self) -> &BTreeSet<String> {
        &self.sources
    }

    /// When the provider last saw it.
    #[must_use]
    pub fn observed_at(&self) -> Timestamp {
        self.observed_at
    }

    /// Whether a subscription is delivering its changes (§33.3).
    #[must_use]
    pub fn is_subscribed(&self) -> bool {
        self.subscribed
    }
}

/// Whether a provider name is a v0.3 external command adapter rather than a canonical provider.
///
/// v0.3 §1.47 spells an adapted object's provenance `adapter:<adapter id>`, and that prefix is
/// the whole test: everything else is a provider that owns its facts (§2.16).
#[must_use]
fn is_adapted(provider: &str) -> bool {
    provider.starts_with("adapter:")
}

/// What registering an observation did (§42.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Registration {
    /// The object was not in the index and now is.
    Added,
    /// The object was already in the index under the same identity, and the entry was refreshed.
    /// This is §42.1's identity test holding: two observations of one live object are one place.
    Reconciled,
}

/// The discovery index of one session (§33.1, §45.2).
#[derive(Debug)]
pub struct SpatialIndex {
    entries: BTreeMap<SpatialId, IndexEntry>,
    aliases: BTreeMap<String, BTreeSet<SpatialId>>,
    /// The identity each provider reference resolved to, per scope. §42.1 requires repeated
    /// observations of one object to resolve to one id; this is where that is enforced rather
    /// than assumed.
    identities: BTreeMap<(String, String), SpatialId>,
    freshness: FreshnessPolicy,
}

impl SpatialIndex {
    /// An empty index with `freshness` as its TTL policy.
    #[must_use]
    pub fn new(freshness: FreshnessPolicy) -> Self {
        Self {
            entries: BTreeMap::new(),
            aliases: BTreeMap::new(),
            identities: BTreeMap::new(),
            freshness,
        }
    }

    /// The TTL policy (§33.3).
    #[must_use]
    pub fn freshness_policy(&self) -> &FreshnessPolicy {
        &self.freshness
    }

    /// How many objects the index holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether it holds none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Registers an observation (§45.2's "object registration/reconciliation").
    ///
    /// # Errors
    ///
    /// `spatial.identity_conflict` when a second observation of the same provider object in the
    /// same scope carries a different [`SpatialId`]. §42.1 makes that a provider conformance
    /// failure, and the index refuses it rather than quietly holding one object as two places —
    /// which would make `back`, pins and every map edge point at a coin flip.
    pub fn register(
        &mut self,
        object: SpatialObject,
        observed_at: Timestamp,
    ) -> Result<Registration, ErrorValue> {
        let key = Self::provider_key(&object);
        let id = object.spatial_id().clone();
        if let Some(known) = self.identities.get(&key)
            && known != &id
        {
            return Err(ErrorValue::new(
                ErrorCode::SpatialIdentityConflict,
                format!(
                    "`{}` was observed as two different objects in the same scope: the index \
                     holds `{known}` and this observation claims `{id}`. Two observations of one \
                     live object must resolve to the same identity (spec v0.4 §42.1).",
                    object.display_name()
                ),
            ));
        }
        self.identities.insert(key, id.clone());

        let aliases = ono_spatial_core::aliases_of(&object);

        let source = object.provenance().provider().to_owned();
        match self.entries.get_mut(&id) {
            Some(entry) => {
                // §2.16 and §37.1: an adapter observes an object, it does not own it. Where a
                // canonical provider has already described this place, the adapted observation
                // refreshes it and adds itself to the sources; it does not replace the record a
                // provider gave with one decoded from a command's output.
                entry.object = if is_adapted(&source) && !is_adapted(&entry.sources_of_record) {
                    entry.object.clone().seen_again(observed_at)
                } else {
                    entry.sources_of_record = source.clone();
                    object.seen_again(observed_at)
                };
                entry.sources.insert(source);
                entry.observed_at = observed_at;
                for alias in &aliases {
                    entry.aliases.insert(alias.clone());
                }
                for alias in aliases {
                    self.aliases.entry(alias).or_default().insert(id.clone());
                }
                Ok(Registration::Reconciled)
            }
            None => {
                for alias in &aliases {
                    self.aliases
                        .entry(alias.clone())
                        .or_default()
                        .insert(id.clone());
                }
                let object_type = object.object_type();
                self.entries.insert(
                    id.clone(),
                    IndexEntry {
                        object,
                        aliases,
                        canonical_parent: canonical_parent_with(&id, object_type, &[], None),
                        path_parent: None,
                        edges: Vec::new(),
                        withheld: BTreeMap::new(),
                        landmarks: Vec::new(),
                        sources: [source.clone()].into_iter().collect(),
                        sources_of_record: source,
                        observed_at,
                        subscribed: false,
                    },
                );
                Ok(Registration::Added)
            }
        }
    }

    /// Adds a name the object answers to besides its display name (§33.1's "display names and
    /// aliases", §27.3's fuzzy matching).
    pub fn add_alias(&mut self, id: &SpatialId, alias: &str) -> bool {
        let alias = alias.to_ascii_lowercase();
        match self.entries.get_mut(id) {
            Some(entry) => {
                entry.aliases.insert(alias.clone());
                self.aliases.entry(alias).or_default().insert(id.clone());
                true
            }
            None => false,
        }
    }

    /// Records a relationship edge and recomputes the canonical parent of both its ends.
    ///
    /// The recomputation is what keeps `up` honest as discovery proceeds: a process registered
    /// before its service was known falls back to `compute.processes`, and arrives under the
    /// service the moment the edge is known (§11.3).
    pub fn record_edge(&mut self, edge: RelationshipEdge) {
        for end in [edge.source().clone(), edge.target().clone()] {
            let Some(entry) = self.entries.get_mut(&end) else {
                continue;
            };
            // §33.2 makes the providers authoritative and the index a cache, so a later
            // observation of the same edge replaces the earlier one rather than being dropped:
            // the assertion is the same, and its provenance and observation time are newer.
            match entry
                .edges
                .iter()
                .position(|known| known.edge_id() == edge.edge_id())
            {
                Some(position) => entry.edges[position] = edge.clone(),
                None => entry.edges.push(edge.clone()),
            }
            let object_type = entry.object.object_type();
            entry.canonical_parent =
                canonical_parent_with(&end, object_type, &entry.edges, entry.path_parent.as_ref());
        }
    }

    /// Records the directory the Unix path tree puts `child` inside (§15.1, §3.4).
    ///
    /// This is hierarchy, not a relationship: no edge carries it, and `up` consults it at exactly
    /// the position [`ono_spatial_core::PATH_PARENT`] holds in the type's rule chain — after the
    /// mount that provides a mount point (§15.3), before the collection space.
    ///
    /// Returns whether the index holds both ends. It refuses a cycle, because a directory that is
    /// its own ancestor would make `up` loop forever.
    pub fn set_path_parent(&mut self, child: &SpatialId, parent: &SpatialId) -> bool {
        if child == parent || !self.entries.contains_key(parent) {
            return false;
        }
        let Some(entry) = self.entries.get_mut(child) else {
            return false;
        };
        entry.path_parent = Some(parent.clone());
        let object_type = entry.object.object_type();
        entry.canonical_parent =
            canonical_parent_with(child, object_type, &entry.edges, entry.path_parent.as_ref());
        true
    }

    /// The places the Unix path tree puts directly inside `id` (§15.1, §15.4, §3.4).
    ///
    /// The reverse of [`SpatialIndex::set_path_parent`], and hierarchy rather than relationship:
    /// §3.4 lists "Directory -> child Directory" among the hierarchical edges, so a directory's
    /// children are not an edge anybody follows but the tree the filesystem already is. Only the
    /// entries this session has actually observed appear — §33.3 makes the filesystem
    /// query-driven, and the index never invents a child nobody read.
    #[must_use]
    pub fn path_children(&self, id: &SpatialId) -> Vec<SpatialId> {
        let mut children: Vec<SpatialId> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.path_parent.as_ref() == Some(id))
            .map(|(child, _)| child.clone())
            .collect();
        children.sort();
        children
    }

    /// Records that one exit of `id` could not be read, and what the user must be told instead
    /// (§35.2, §42.4).
    ///
    /// §42.4: "Denied information must produce `permission_denied` or `unknown`, never false
    /// empty collections." This is where that survives: [`SpatialIndex::relation_summary`] shows
    /// the state and the detail in place of a count, so "files — permission denied for 14 process
    /// FDs" cannot become "files — 0" on the way to a place view.
    pub fn record_withheld(
        &mut self,
        id: &SpatialId,
        label: &str,
        state: PermissionState,
        detail: &str,
    ) -> bool {
        match self.entries.get_mut(id) {
            Some(entry) => {
                entry
                    .withheld
                    .insert(label.to_owned(), (state, detail.to_owned()));
                true
            }
            None => false,
        }
    }

    /// What the user was told about each exit that could not be read (§35.2).
    #[must_use]
    pub fn withheld(&self, id: &SpatialId) -> Vec<(&str, PermissionState, &str)> {
        self.entries
            .get(id)
            .into_iter()
            .flat_map(|entry| {
                entry
                    .withheld
                    .iter()
                    .map(|(label, (state, detail))| (label.as_str(), *state, detail.as_str()))
            })
            .collect()
    }

    /// Records what deserves attention about an object (§26, §33.1's "landmark state").
    pub fn set_landmarks(&mut self, id: &SpatialId, landmarks: Vec<Landmark>) -> bool {
        match self.entries.get_mut(id) {
            Some(entry) => {
                entry.landmarks = landmarks;
                true
            }
            None => false,
        }
    }

    /// Records that a provider subscription is delivering the object's changes (§33.3).
    pub fn set_subscribed(&mut self, id: &SpatialId, subscribed: bool) -> bool {
        match self.entries.get_mut(id) {
            Some(entry) => {
                entry.subscribed = subscribed;
                true
            }
            None => false,
        }
    }

    /// The entry for `id`.
    #[must_use]
    pub fn get(&self, id: &SpatialId) -> Option<&IndexEntry> {
        self.entries.get(id)
    }

    /// Every entry, by identity.
    pub fn entries(&self) -> impl Iterator<Item = &IndexEntry> {
        self.entries.values()
    }

    /// Whether the index still holds `id`.
    #[must_use]
    pub fn contains(&self, id: &SpatialId) -> bool {
        self.entries.contains_key(id)
    }

    /// Records that the object behind `id` has ended, keeping the entry itself (§10.3, §33.2).
    ///
    /// §10.3 keeps a removed object reachable as a tombstone, and §20.3 makes `back` arrive at
    /// one — so the entry stays, with its identity and its place in the hierarchy intact, and
    /// only its lifetime closes. Forgetting it outright is [`SpatialIndex::remove`], which is a
    /// different answer: it makes the place one nobody ever saw.
    ///
    /// Returns whether the index held the object.
    pub fn mark_ended(&mut self, id: &SpatialId, at: Timestamp) -> bool {
        let Some(entry) = self.entries.get_mut(id) else {
            return false;
        };
        entry.object = entry.object.clone().ended(at);
        true
    }

    /// Drops every relationship edge that touches `id`, from both of its ends (§33.2).
    ///
    /// The index is a cache of what the providers asserted, and an edge nobody asserts any more
    /// is not a relationship that merely went unmentioned — it is one that is not there. A live
    /// view has to be able to say so, and it can only do that if the previous answer is dropped
    /// before the current one is read.
    ///
    /// Returns how many edges were dropped.
    pub fn forget_edges(&mut self, id: &SpatialId) -> usize {
        let ends: Vec<SpatialId> = self.entries.get(id).map_or_else(Vec::new, |entry| {
            entry
                .edges
                .iter()
                .filter_map(|edge| edge.other_end(id).cloned())
                .chain(std::iter::once(id.clone()))
                .collect()
        });
        let mut dropped = 0;
        for end in ends {
            let Some(entry) = self.entries.get_mut(&end) else {
                continue;
            };
            let before = entry.edges.len();
            entry
                .edges
                .retain(|edge| edge.source() != id && edge.target() != id);
            dropped += before - entry.edges.len();
            let object_type = entry.object.object_type();
            entry.canonical_parent =
                canonical_parent_with(&end, object_type, &entry.edges, entry.path_parent.as_ref());
        }
        dropped
    }

    /// Forgets an object that has gone away, and every alias that named only it.
    pub fn remove(&mut self, id: &SpatialId) -> Option<IndexEntry> {
        let entry = self.entries.remove(id)?;
        for alias in &entry.aliases {
            if let Some(ids) = self.aliases.get_mut(alias) {
                ids.remove(id);
                if ids.is_empty() {
                    self.aliases.remove(alias);
                }
            }
        }
        self.identities.retain(|_, known| known != id);
        Some(entry)
    }

    /// Every object that answers to `text` exactly, in identity order (§27.1).
    ///
    /// Matching is case-insensitive because names are typed by people; it is not fuzzy, because
    /// deciding *how* fuzzy is the query layer's job (§27.3) and the index must not make one
    /// candidate disappear before the ambiguity rules of §27.2 have seen it.
    #[must_use]
    pub fn by_alias(&self, text: &str) -> Vec<&IndexEntry> {
        self.aliases
            .get(&text.to_ascii_lowercase())
            .into_iter()
            .flatten()
            .filter_map(|id| self.entries.get(id))
            .collect()
    }

    /// Every object whose name contains `text`, in identity order (§9.4, §27.3).
    ///
    /// Ordering is by identity rather than by relevance for the same reason: ranking is the query
    /// layer's, and a deterministic order here is what makes §29.3's "deterministic ambiguity"
    /// possible at all.
    #[must_use]
    pub fn search(&self, text: &str) -> Vec<&IndexEntry> {
        let needle = text.to_ascii_lowercase();
        self.entries
            .values()
            .filter(|entry| entry.aliases.iter().any(|alias| alias.contains(&needle)))
            .collect()
    }

    /// Every object of one type, in identity order.
    #[must_use]
    pub fn of_type(&self, object_type: SpatialType) -> Vec<&IndexEntry> {
        self.entries
            .values()
            .filter(|entry| entry.object.object_type() == object_type)
            .collect()
    }

    /// Where `up` goes from `id` (§45.2's "canonical parent lookup").
    #[must_use]
    pub fn canonical_parent(&self, id: &SpatialId) -> Option<&HierarchicalEdge> {
        self.entries.get(id)?.canonical_parent()
    }

    /// How current the index's answer about `id` is, as of `now` (§33.4).
    #[must_use]
    pub fn freshness(&self, id: &SpatialId, now: Timestamp) -> Freshness {
        let Some(entry) = self.entries.get(id) else {
            return Freshness::Unknown;
        };
        self.freshness.freshness(
            entry.object.object_type(),
            Some(entry.observed_at),
            entry.subscribed,
            now,
        )
    }

    /// The object to act on, or a refusal (§33.2's "Actions MUST resolve/revalidate live objects
    /// before mutation").
    ///
    /// The index does not call providers — it is a cache, and §45.2 says it must treat them as
    /// truth — so it does the one thing a cache honestly can: it refuses to hand a stale entry to
    /// a mutation, and names what the caller must do about it.
    ///
    /// # Errors
    ///
    /// - `spatial.not_found` when the index does not hold the object.
    /// - `spatial.stale` when it does, but the observation is older than its class's TTL.
    pub fn resolve_for_action(
        &self,
        id: &SpatialId,
        now: Timestamp,
    ) -> Result<&IndexEntry, ErrorValue> {
        let entry = self.entries.get(id).ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::SpatialNotFound,
                format!("no object in the spatial index answers to `{id}`"),
            )
        })?;
        if self.freshness(id, now).is_current() {
            return Ok(entry);
        }
        Err(ErrorValue::new(
            ErrorCode::SpatialStale,
            format!(
                "`{}` was last observed at {} and the index is a cache, not the truth; the \
                 provider must be asked again before this object is acted on (spec v0.4 §33.2)",
                entry.object.display_name(),
                entry.observed_at
            ),
        ))
    }

    /// The exits of `id`, bounded (§45.2's "bounded relation summaries", §3.6, §32.2).
    ///
    /// Each declared exit of the object's type becomes a group, so an exit with no neighbour is
    /// visible as empty rather than missing (§2.17). `budget` caps how many members a group lists;
    /// the rest are counted, never dropped silently (§3.6's `hidden_count`). An
    /// [`CostClass::Expensive`] relation is listed as a discoverable but unloaded exit unless its
    /// edges are already known, which is §32.2 exactly.
    #[must_use]
    pub fn relation_summary(
        &self,
        id: &SpatialId,
        budget: usize,
        now: Timestamp,
    ) -> Vec<NeighborhoodGroup> {
        let Some(entry) = self.entries.get(id) else {
            return Vec::new();
        };
        let object_type = entry.object.object_type();
        let freshness = self.freshness(id, now);
        let mut groups = Vec::new();

        for (label, spec) in relation::exits_from(object_type) {
            // §35.2, §42.4: a refusal outranks a count. An exit the provider could not read is
            // shown as refused with its reason, never as an empty collection.
            if let Some((state, detail)) = entry.withheld.get(label) {
                groups.push(
                    NeighborhoodGroup::withheld(label, *state, detail).along(spec.relation_type()),
                );
                continue;
            }
            // The label decides the direction, not only the relation: `process.parent_of` is one
            // relation with two exits, and a process's `children` are the edges it is the source
            // of while its `parent` is the one edge it is the target of (§12).
            let members: Vec<SpatialId> = entry
                .edges
                .iter()
                .filter(|edge| edge.relation().as_str() == spec.id)
                .filter(|edge| edge.group_from(id) == Some(label))
                .filter_map(|edge| edge.other_end(id).cloned())
                .filter(|end| self.entries.contains_key(end))
                .collect();

            if members.is_empty() && spec.cost_class == CostClass::Expensive {
                groups.push(
                    NeighborhoodGroup::withheld(
                        label,
                        PermissionState::Unknown,
                        "available on request",
                    )
                    .along(spec.relation_type()),
                );
                continue;
            }

            let total = members.len();
            let listed: Vec<SpatialId> = members.into_iter().take(budget).collect();
            groups.push(
                NeighborhoodGroup::available(label, listed)
                    .of_total(total)
                    .along(spec.relation_type())
                    .observed(freshness),
            );
        }
        groups
    }

    /// What makes two observations the same provider object: the reference, inside the scope it
    /// was observed in. The same uid in two containers is two objects (§16.2).
    fn provider_key(object: &SpatialObject) -> (String, String) {
        (
            object
                .scope()
                .chain()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("/"),
            object.canonical_ref().id().to_string(),
        )
    }
}
