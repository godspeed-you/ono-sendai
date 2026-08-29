//! Cost-aware discovery planning (spec v0.4 §9, §32.1, §33.3, §34, §45.3).
//!
//! §45.3 ends its list of responsibilities with "cost-aware lazy queries", and §34 says why: the
//! spatial layer "MUST not make Ono feel slower than a shell", with a search budget of 100 ms.
//! A search that asked every provider for every object it serves would spend that on a directory
//! walk before it looked at the first candidate.
//!
//! So a search plans first. It decides which provider targets can possibly hold an answer, and
//! asks only those:
//!
//! - a `--type` names one kind of place, and only the targets that serve it are asked;
//! - a predicate names fields, and only the targets whose schema declares every one of them can
//!   match it — `--where local.port == 8080` is a question about sockets and about nothing else;
//! - files and directories are `query-driven` (§33.3) and [`CostClass::Expensive`] (§32.1), so a
//!   search reaches them only when it was asked to: by type, or by an anchor that is one.
//!
//! What was *not* asked is part of the answer, not a silent omission (§2.17): [`TargetPlan`]
//! keeps the skipped targets and the reason each was skipped, so a diagnostic or `explain` can
//! say why a search did not look somewhere.

use std::collections::BTreeSet;

use ono_spatial_core::{CostClass, SpatialType, types_of_target};

/// Every v0.2 provider target whose objects §7 gives a place, with what asking it costs (§32.1).
///
/// The cost class is the *search* cost — what it takes to enumerate the target — not the cost of
/// one relation. `dir` and `file` are expensive because §33.3 makes them query-driven: there is
/// no bounded enumeration of a filesystem, only a walk.
const SPATIAL_TARGETS: &[(&str, CostClass)] = &[
    ("host", CostClass::Cheap),
    ("process", CostClass::Cheap),
    ("interface", CostClass::Cheap),
    ("mount", CostClass::Cheap),
    ("service", CostClass::Normal),
    ("socket", CostClass::Normal),
    ("connection", CostClass::Normal),
    ("route", CostClass::Normal),
    ("neighbor", CostClass::Normal),
    ("filesystem", CostClass::Normal),
    ("device", CostClass::Normal),
    ("container", CostClass::Normal),
    ("user", CostClass::Normal),
    ("group", CostClass::Normal),
    ("session", CostClass::Normal),
    ("dir", CostClass::Expensive),
    ("file", CostClass::Expensive),
];

/// Why a target was left out of a search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skipped {
    /// It serves no object of the requested type.
    WrongType(SpatialType),
    /// Its schema does not declare a field the predicate reads, so no record it serves can match.
    MissingField(String),
    /// Enumerating it is expensive and nothing in the request asked for it (§32.1, §33.3).
    TooExpensive(CostClass),
    /// No registered provider answers for it.
    NoProvider,
}

impl Skipped {
    /// The reason, in the words a diagnostic uses.
    #[must_use]
    pub fn reason(&self) -> String {
        match self {
            Skipped::WrongType(wanted) => {
                format!("serves no `{}`", wanted.as_str())
            }
            Skipped::MissingField(field) => {
                format!("its records carry no `{field}`, so none can match the predicate")
            }
            Skipped::TooExpensive(cost) => format!(
                "enumerating it is `{}`, and nothing in the search asked for it (spec v0.4 §32.1)",
                cost.as_str()
            ),
            Skipped::NoProvider => "no provider answers for it".to_owned(),
        }
    }
}

/// Which targets a search asks, and which it deliberately does not (§34, §45.3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetPlan {
    asked: Vec<&'static str>,
    skipped: Vec<(&'static str, Skipped)>,
    candidates: Vec<&'static str>,
    unknown_fields: Vec<String>,
}

impl TargetPlan {
    /// The targets to ask, cheapest first.
    #[must_use]
    pub fn asked(&self) -> &[&'static str] {
        &self.asked
    }

    /// The targets not asked, and why.
    #[must_use]
    pub fn skipped(&self) -> &[(&'static str, Skipped)] {
        &self.skipped
    }

    /// The targets a search of this kind could hold an answer in at all: served by a provider,
    /// and serving the kind of place the search asked for.
    ///
    /// Cost and the predicate's fields narrow this further; what is *not* in it is a target the
    /// question could never have been about.
    #[must_use]
    pub fn candidates(&self) -> &[&'static str] {
        &self.candidates
    }

    /// The predicate's fields that no candidate target declares.
    ///
    /// A field some candidates declare narrows the search — a cross-type search is what
    /// `find place` is for, and a mount having no `cpu` is not an error. A field *none* of them
    /// declares is a word about nothing, and v0.2 §11.3 refuses it before anything is
    /// enumerated rather than answering an empty stream (ADR-0210).
    #[must_use]
    pub fn unknown_fields(&self) -> &[String] {
        &self.unknown_fields
    }

    /// Whether the plan asks nothing at all.
    ///
    /// That is a real answer where the fields exist and the candidates were narrowed away — a
    /// search for a container on a host with no container runtime finds nothing, and says so at
    /// no cost. It is *not* a real answer where the predicate names a field nothing declares:
    /// see [`TargetPlan::unknown_fields`], which the caller checks first (ADR-0210, superseding
    /// this method's earlier reading).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.asked.is_empty()
    }
}

/// Plans which provider targets a search must ask (§34's search budget).
///
/// `object_type` is the `--type` filter, `fields` the root field names the predicate reads, and
/// `declares` answers whether a target's records carry a field — the caller passes it because
/// the schemas belong to the provider registry, and this crate plans rather than observes
/// (§2.16). `serves` says whether any provider answers for a target at all.
#[must_use]
pub fn targets_for(
    object_type: Option<SpatialType>,
    fields: &BTreeSet<String>,
    serves: &impl Fn(&str) -> bool,
    declares: &impl Fn(&str, &str) -> bool,
) -> TargetPlan {
    let mut plan = TargetPlan::default();
    for (target, cost) in SPATIAL_TARGETS {
        if !serves(target) {
            plan.skipped.push((target, Skipped::NoProvider));
            continue;
        }
        if let Some(wanted) = object_type
            && !types_of_target(target)
                .iter()
                .any(|served| served.is_a(wanted))
        {
            plan.skipped.push((target, Skipped::WrongType(wanted)));
            continue;
        }
        plan.candidates.push(target);
        if let Some(missing) = fields.iter().find(|field| !declares(target, field)) {
            plan.skipped
                .push((target, Skipped::MissingField(missing.clone())));
            continue;
        }
        // An expensive target is asked only when it was asked for by name: `--type file`,
        // `--type directory`. Otherwise `find place nginx` would walk the filesystem before it
        // looked at a single process (§33.3, §34).
        if *cost == CostClass::Expensive && object_type.is_none() {
            plan.skipped.push((target, Skipped::TooExpensive(*cost)));
            continue;
        }
        plan.asked.push(target);
    }
    plan.unknown_fields = fields
        .iter()
        .filter(|field| {
            !plan
                .candidates
                .iter()
                .any(|target| declares(target, field.as_str()))
        })
        .cloned()
        .collect();
    plan
}

/// The root field names a predicate reads, in the form a schema declares them.
///
/// `local.port == 8080` reads `local`; `state == "running"` reads `state`. The root is what a
/// schema can be asked about, because a nested field belongs to whatever record the root holds.
#[must_use]
pub fn root_fields(paths: impl IntoIterator<Item = String>) -> BTreeSet<String> {
    paths
        .into_iter()
        .filter_map(|path| {
            path.split('.')
                .next()
                .filter(|root| !root.is_empty())
                .map(str::to_owned)
        })
        .collect()
}

/// Every target this crate considers spatial, cheapest first.
#[must_use]
pub fn spatial_targets() -> Vec<&'static str> {
    SPATIAL_TARGETS.iter().map(|(target, _)| *target).collect()
}

/// Where the objects of one canonical space come from (§7, §32.1, §45.3).
///
/// A space that holds objects holds them because some provider answers for them. Which provider
/// targets those are is a planning decision, and it is made here rather than in the shell so that
/// `look`, `near` and `find place` all ask the same question of the same places (§45.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpaceSource {
    /// The provider targets whose records can become places here, cheapest first.
    pub targets: &'static [&'static str],
    /// The spatial types this space holds. A record that projects to one of them belongs here.
    ///
    /// The match is exact rather than by [`SpatialType::is_a`]: `ono.socket/1` serves both
    /// `network.listeners` and `network.connections`, and a listener is not a connection (§14.3,
    /// §14.4). Where a space genuinely holds a family — DEVICES holds every kernel-visible device
    /// (§7.7) — the family is written out.
    pub accepts: &'static [SpatialType],
    /// What enumerating it costs (§32.1). An [`CostClass::Expensive`] space is not enumerated by
    /// an orientation command: §33.3 makes the filesystem query-driven, and a `look` that walked
    /// it would spend the §34 budget before it said where the user is standing.
    pub cost: CostClass,
}

/// The objects one canonical space holds, or `None` where it holds only other places (§7).
///
/// `None` is the root and the four domains that are pure geography: their exits are the spaces
/// below them, and asking a provider about "COMPUTE" would be asking about a word. A space that
/// *does* hold objects but that no target serves — `network.addresses`, `compute.cgroups`,
/// `network.namespaces` — answers with an empty `targets`, which is how the place stays visible
/// and reports `unsupported` rather than an empty collection (§4, §35.2, §2.17).
#[must_use]
pub fn source_of_space(space_id: &str) -> Option<SpaceSource> {
    use CostClass::{Cheap, Expensive, Normal};
    use SpatialType as T;
    let source = |targets: &'static [&'static str],
                  accepts: &'static [SpatialType],
                  cost: CostClass| SpaceSource {
        targets,
        accepts,
        cost,
    };
    Some(match space_id {
        "containers" => source(&["container"], &[T::Container], Normal),
        "devices" => source(&["device"], &[T::BlockDevice, T::Device], Normal),
        "compute.processes" => source(&["process"], &[T::Process], Cheap),
        "compute.services" => source(&["service"], &[T::Service], Normal),
        "compute.jobs" => source(&["job"], &[T::Job], Cheap),
        "compute.cgroups" => source(&[], &[T::Cgroup], Normal),
        "compute.workloads" => source(&[], &[T::Workload], Normal),
        "network.interfaces" => source(&["interface"], &[T::Interface], Cheap),
        "network.addresses" => source(&[], &[T::Address], Cheap),
        "network.routes" => source(&["route"], &[T::Route], Normal),
        "network.neighbors" => source(&["neighbor"], &[T::Neighbor], Normal),
        "network.listeners" => source(&["socket"], &[T::Listener], Normal),
        "network.connections" => source(&["socket"], &[T::Connection], Normal),
        "network.namespaces" => source(&[], &[T::Namespace], Normal),
        "storage.filesystems" => source(&["filesystem"], &[T::Filesystem], Normal),
        "storage.mounts" => source(&["mount"], &[T::Mount], Cheap),
        "storage.devices" => source(&["device"], &[T::BlockDevice], Normal),
        "storage.directories" => source(&["dir"], &[T::Directory], Expensive),
        "identity.users" => source(&["user"], &[T::User], Normal),
        "identity.groups" => source(&["group"], &[T::Group], Normal),
        "identity.sessions" => source(&["session"], &[T::Session], Normal),
        _ => return None,
    })
}
