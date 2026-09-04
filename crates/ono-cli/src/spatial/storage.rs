//! Where the filesystem working directory and the spatial place meet (spec v0.4 §15.1, §30).
//!
//! §30 draws one line and keeps it: "Ono MUST maintain a clear distinction between filesystem
//! working directory and spatial place." They are two pieces of state, and this module is the
//! only place that connects them — in the two directions §30 allows and in no others:
//!
//! - **`enter` a directory changes both** (§30.2), because the Unix path model is older and
//!   better established than the spatial one and §15.1 keeps it intact. Entering anything else
//!   — a file, a socket, a process — moves the place and leaves the working directory alone.
//! - **`cd` moves the place only inside the storage family** (§30.3), which is what the default
//!   `spatial.follow_cwd = "storage-only"` means: a `cd` in the middle of a process
//!   investigation must not throw the user out of it.
//!
//! `PWD` is never touched by any of this (§30.4). It is the filesystem working directory, every
//! external command reads it, and a spatial place encoded into it would break all of them.

use std::path::{Path, PathBuf};

use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_provider_api::{Query, Selector};
use ono_spatial_core::{Movement, NavigationStep, SpatialId, SpatialType};
use ono_value::{ErrorValue, RecordValue, Value};

use crate::session::Session;

/// Whether a word is meant as a filesystem path rather than a place name (§27.1, §30.2).
///
/// Only the spellings that cannot be anything else: an absolute path, an explicitly relative one,
/// a home-relative one, and the two directory names Unix reserves. A bare word is a place name
/// first — §27.1 resolves a visible child before it resolves anything global, and a file in the
/// working directory must not shadow the COMPUTE domain.
#[must_use]
pub fn looks_like_a_path(word: &str) -> bool {
    word == "."
        || word == ".."
        || word.starts_with('/')
        || word.starts_with("./")
        || word.starts_with("../")
        || word.starts_with('~')
}

/// The absolute path a word names, from the session's working directory.
#[must_use]
pub fn absolute(session: &Session, word: &str) -> PathBuf {
    let path = if let Some(rest) = word.strip_prefix("~/") {
        match session.home() {
            Some(home) => home.join(rest),
            None => PathBuf::from(word),
        }
    } else if word == "~" {
        session.home().unwrap_or_else(|| PathBuf::from(word))
    } else {
        PathBuf::from(word)
    };
    let path = if path.is_absolute() {
        path
    } else {
        session.cwd().join(path)
    };
    // §15.1 identifies a filesystem place by the object, not by the text that reached it, and
    // §42.1 makes one object one place: `/srv/app/..` and `/srv` are the same directory, so the
    // spelling is resolved away before anything observes it. A path that cannot be resolved —
    // it is not there, or this user may not walk to it — keeps the words the user typed, so the
    // provider is the one that decides which of those two it is (§35.2).
    std::fs::canonicalize(&path).unwrap_or(path)
}

/// Makes the filesystem object at `path` the current place, observing it first (§2.16, §15.4).
///
/// Returns the place and whether it is a directory, which is what decides §30.2's cwd rule.
///
/// # Errors
///
/// `spatial.not_found` when no provider can answer for the path — which is also the answer for a
/// path that does not exist. §40 requires a refusal rather than an empty place, and a refused
/// `enter` moves neither the place nor the working directory.
pub fn enter_path(session: &mut Session, path: &Path) -> Result<(SpatialId, bool), ErrorValue> {
    let (runtime, providers) = session.pipeline_context().ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::IoPermissionDenied,
            "the operating system refused to start the runtime",
        )
    })?;
    let providers = providers.clone();
    let now = Timestamp::now();
    let path = path.to_path_buf();

    runtime.block_on(async move {
        let mut state = crate::spatial::spatial_session().await;
        let there = observe_place_at(&providers, &mut state, &path, now).await?;
        let is_directory = state
            .index()
            .get(&there)
            .is_some_and(|entry| entry.object().object_type() == SpatialType::Directory);
        // §30.2 and §53: "Entering a directory changes cwd." A directory this user may not read
        // is neither a listing §15.4 could show nor a working directory the shell could run a
        // program from, so the move is refused by name instead of leaving the session standing
        // somewhere it cannot work (§35.1, §40).
        if is_directory
            && let Err(error) = std::fs::read_dir(&path)
            && error.kind() == std::io::ErrorKind::PermissionDenied
        {
            return Err(ErrorValue::new(
                ErrorCode::SpatialPermissionDenied,
                format!("`{}` cannot be read by this user", path.display()),
            )
            .with_help(
                "denied is not empty: the directory is there and its contents are not this \
                 user's to see (spec v0.4 §35.1, §35.2). Navigation never escalates on its own",
            ));
        }
        let here = state.current_place().clone();
        if here != there {
            let mut step = NavigationStep::new(now, here.clone(), there.clone(), Movement::Enter);
            if let Some(crossing) =
                crate::spatial::movement::crossing_between(&state, &here, &there)
            {
                step = step.crossing(crossing);
            }
            state.trail_mut().record(step);
        }
        Ok((there, is_directory))
    })
}

/// Observes the filesystem object at `path`, everything §15 needs around it, and registers it.
///
/// Three observations, because §15 makes a path place three things at once (ADR-0187):
///
/// - the object itself, which is the place;
/// - the **mount table**, so the boundary of §15.3 can be named and the mount point can lead up
///   to its mount (§11.1's rule chain for a directory);
/// - the **enclosing directory**, so `up` from a file reaches it — §15.1 keeps the Unix path
///   tree, and the path tree is hierarchy, not a relation (§3.4).
///
/// Every fact comes from a provider (§2.16): the mount table is `get mount`, not
/// `/proc/self/mountinfo` read behind its back.
///
/// # Errors
///
/// `spatial.not_found` when no provider answers for the path (§40).
pub(crate) async fn observe_place_at(
    providers: &ono_provider_api::ProviderRegistry,
    state: &mut crate::spatial::SpatialSessionState,
    path: &Path,
    now: Timestamp,
) -> Result<SpatialId, ErrorValue> {
    let record = match read_path(providers, path).await {
        PathAnswer::Found(record) => record,
        // §35.2 and §53 keep denied and missing apart. Reporting "the path does not exist" for a
        // path this user may not look at states something untrue about the machine, so the
        // provider's refusal keeps its meaning and §40 names it.
        PathAnswer::Denied(reason) => {
            return Err(ErrorValue::new(
                ErrorCode::SpatialPermissionDenied,
                format!("`{}` cannot be read by this user: {reason}", path.display()),
            )
            .with_help(
                "denied is not missing: the path may well be there (spec v0.4 §35.2, §40). \
                 Navigation never escalates on its own — run it as a user who may read it",
            ));
        }
        PathAnswer::Absent => {
            return Err(ErrorValue::new(
                ErrorCode::SpatialNotFound,
                format!("no place answers to `{}`", path.display()),
            )
            .with_help("the path does not exist, or no provider can read it (spec v0.4 §40)"));
        }
    };
    let there = state.projection_of(&record)?;
    state.absorb(std::slice::from_ref(&record), now);

    crate::spatial::view::observe_targets_with(providers, state, &["mount"], now).await;
    link_mount(state, &there, path, now);

    // §35.2 and §2.17: a directory this user may not read has contents, and the shell does not
    // know what they are. Recording the refusal is what keeps every later view — the place view,
    // `near`, and the map's `completeness` — from reporting a place it could not read as one it
    // read and found nothing in.
    if let Err(error) = std::fs::read_dir(path)
        && error.kind() == std::io::ErrorKind::PermissionDenied
    {
        let (index, _) = state.absorb_with();
        index.record_withheld(
            &there,
            "children",
            ono_spatial_core::PermissionState::PermissionDenied,
            &format!("`{}` cannot be read by this user", path.display()),
        );
    }

    // §15.1: the enclosing directory is the parent of the Unix path tree. `up` consults it at
    // exactly the position `path.parent` holds in the rule chain, so a mount point still goes up
    // to its mount first (§15.3).
    if let Some(parent) = path.parent()
        && parent != path
        && let PathAnswer::Found(record) = read_path(providers, parent).await
    {
        let above = state.projection_of(&record)?;
        state.absorb(std::slice::from_ref(&record), now);
        let (index, _) = state.absorb_with();
        index.set_path_parent(&there, &above);
    }
    Ok(there)
}

/// What the file provider had to say about one path (§35.2's three distinct answers).
enum PathAnswer {
    /// The object is there and this is it.
    Found(RecordValue),
    /// The provider was refused, and this is what it said.
    Denied(String),
    /// Nothing answered, and nothing was refused.
    Absent,
}

/// The record the file provider answers for one path, or why it could not.
async fn read_path(providers: &ono_provider_api::ProviderRegistry, path: &Path) -> PathAnswer {
    let query = Query::target("file").with(Selector::field(
        "path",
        Value::Path(std::sync::Arc::from(path)),
    ));
    let Ok(stream) = providers.snapshot(&query) else {
        return PathAnswer::Absent;
    };
    let collected = stream.collect().await;
    let refusal = collected
        .errors()
        .iter()
        .find(|error| error.code().kind() == ono_core::ErrorKind::Permission)
        .map(|error| error.message().to_owned());
    let found = collected
        .into_values()
        .into_iter()
        .find_map(|value| match value {
            Value::Record(record) => Some(RecordValue::clone(&record)),
            _ => None,
        });
    match (found, refusal) {
        (Some(record), _) => PathAnswer::Found(record),
        (None, Some(reason)) => PathAnswer::Denied(reason),
        (None, None) => PathAnswer::Absent,
    }
}

/// The mount points this session knows, deepest first (§15.2, §2.16).
///
/// Deepest first because that is how a path is matched to the mount that actually provides it:
/// `/var/lib/docker` is provided by `/var/lib/docker` and not by `/`.
fn known_mounts(state: &crate::spatial::SpatialSessionState) -> Vec<(PathBuf, SpatialId)> {
    let mut mounts: Vec<(PathBuf, SpatialId)> = state
        .index()
        .of_type(SpatialType::Mount)
        .into_iter()
        .filter_map(|entry| {
            let target = match entry.canonical_ref().id().values().first() {
                Some(Value::Path(path)) => path.to_path_buf(),
                Some(Value::String(text)) => PathBuf::from(text.as_ref()),
                _ => return None,
            };
            Some((target, entry.object().spatial_id().clone()))
        })
        .collect();
    mounts.sort_by(|a, b| b.0.components().count().cmp(&a.0.components().count()));
    mounts
}

/// The mount that provides `path`, as the mount provider reports the table (§15.3).
pub(crate) fn mount_of(
    state: &crate::spatial::SpatialSessionState,
    path: &Path,
) -> Option<(PathBuf, SpatialId)> {
    known_mounts(state)
        .into_iter()
        .find(|(target, _)| path.starts_with(target))
}

/// The path a place stands on, where it stands on one (§15.1).
pub(crate) fn path_of(
    state: &crate::spatial::SpatialSessionState,
    id: &SpatialId,
) -> Option<PathBuf> {
    match state.record_of(id)?.get("path") {
        Some(Value::Path(path)) => Some(path.to_path_buf()),
        Some(Value::String(text)) => Some(PathBuf::from(text.as_ref())),
        _ => None,
    }
}

/// Records `mount.backs_directory` between a directory and the mount that provides it (§15.3).
///
/// §15.4 lists the mount boundary among a directory place's neighbours and §15.2 files directory
/// roots under MOUNTS, so the edge is drawn for every directory the mount provides — not only for
/// its mount point. It is hierarchy only where the Unix path tree runs out: a directory with a
/// path above it goes up the path (§15.1), and a directory root goes up to its mount (ADR-0187).
pub(crate) fn link_mount_of(
    state: &mut crate::spatial::SpatialSessionState,
    place: &SpatialId,
    now: Timestamp,
) {
    let Some(path) = path_of(state, place) else {
        return;
    };
    link_mount(state, place, &path, now);
}

fn link_mount(
    state: &mut crate::spatial::SpatialSessionState,
    place: &SpatialId,
    path: &Path,
    now: Timestamp,
) {
    if state
        .index()
        .get(place)
        .is_none_or(|entry| entry.object().object_type() != SpatialType::Directory)
    {
        return;
    }
    let Some((_, mount)) = mount_of(state, path) else {
        return;
    };
    let Some(spec) = ono_spatial_core::relation::spec("mount.backs_directory") else {
        return;
    };
    let edge = ono_spatial_core::RelationshipEdge::new(
        mount,
        place.clone(),
        spec.relation_type(),
        ono_spatial_core::Confidence::Exact,
        ono_value::Provenance::local("ono.spatial", ono_value::SchemaId::new("ono.mount", 1))
            .observed_at(now),
        now,
    );
    let (index, _) = state.absorb_with();
    index.record_edge(edge);
}

/// The scope boundary a movement between two path places crosses, where it crosses one (§15.3).
///
/// §3.2 lists `filesystem` among the scope kinds and §15.3 makes the mount its boundary, so a
/// step from a place on one mount to a place on another crosses one — which §2.18 and §3.2
/// require to be observable in the trail. The scopes are named by their mount points, because
/// that is the name a user typed to get here.
pub(crate) fn filesystem_crossing(
    state: &crate::spatial::SpatialSessionState,
    from: &SpatialId,
    to: &SpatialId,
) -> Option<ono_spatial_core::ScopeBoundary> {
    let here = path_of(state, from)?;
    let there = path_of(state, to)?;
    let (left, _) = mount_of(state, &here)?;
    let (entered, _) = mount_of(state, &there)?;
    if left == entered {
        return None;
    }
    let scope = state.scope();
    let from_scope = scope.nest(
        ono_spatial_core::ScopeKind::Filesystem,
        &left.display().to_string(),
    );
    let to_scope = scope.nest(
        ono_spatial_core::ScopeKind::Filesystem,
        &entered.display().to_string(),
    );
    from_scope.boundary_to(&to_scope)
}

/// Moves the place with `cd`, where §30.3 says it should.
///
/// The default `spatial.follow_cwd = "storage-only"` synchronises only while the place is already
/// inside the storage family; `always` synchronises wherever the place is; `never` never does.
/// A failure to observe the new directory is not a failure of `cd`: the working directory did
/// change, and saying otherwise would be worse than a place that stayed put.
pub fn follow_cwd(session: &mut Session, destination: &Path) {
    if crate::spatial::disabled(session) {
        return;
    }
    let mode = session
        .settings()
        .effective("spatial.follow_cwd")
        .map(|resolved| resolved.value.clone())
        .and_then(|value| ono_value::canonical_text(&value).ok())
        .unwrap_or_else(|| "storage-only".to_owned());
    if mode == "never" {
        return;
    }
    if mode != "always" && !place_is_storage() {
        return;
    }
    let _ = enter_path(session, destination);
}

/// Whether the current place belongs to the filesystem/storage navigation family (§30.3).
///
/// Read without awaiting: `cd` is a shell builtin and runs outside the runtime the spatial
/// commands hold the state under. A state some other task holds is not a state that says `yes` —
/// the working directory still moves, and the place stays where it was.
fn place_is_storage() -> bool {
    let state = crate::spatial::session::session_state();
    let Ok(state) = state.try_lock() else {
        return false;
    };
    let here = state.current_place().clone();
    if let Some(space) = ono_spatial_query::resolve::space_of(&here) {
        return ono_spatial_core::path_to_space(space.id)
            .iter()
            .any(|step| step.id == "storage");
    }
    state.index().get(&here).is_some_and(|entry| {
        matches!(
            entry.object().object_type(),
            SpatialType::Directory
                | SpatialType::File
                | SpatialType::Mount
                | SpatialType::Filesystem
                | SpatialType::BlockDevice
        )
    })
}

/// Whether a filesystem lives on another machine (§15.3's `remote yes`).
///
/// The kernel does not report it, so it is read off the two facts the mount provider does give:
/// the filesystem type, and the shape of the source. Both are conservative — a type this build
/// does not know is local, and a local device path is never called remote — because §2.17 makes
/// an honest `no` better than a guessed `yes` (ADR-0187).
#[must_use]
pub fn is_remote_filesystem(filesystem: &str, source: &str) -> bool {
    // The network filesystems Linux ships, plus the FUSE spellings of the same things.
    const NETWORK: &[&str] = &[
        "nfs",
        "nfs4",
        "cifs",
        "smb3",
        "smbfs",
        "afs",
        "ceph",
        "glusterfs",
        "lustre",
        "ocfs2",
        "9p",
        "sshfs",
        "davfs",
        "beegfs",
        "orangefs",
        "gfs2",
        "pvfs2",
    ];
    let kind = filesystem.to_ascii_lowercase();
    let bare = kind.strip_prefix("fuse.").unwrap_or(&kind);
    if NETWORK.contains(&bare) {
        return true;
    }
    // A `host:/export` or `//host/share` source is a network location whatever the type is
    // called; a device path and a pseudo-filesystem name are not.
    if source.starts_with("//") {
        return true;
    }
    match source.split_once(':') {
        Some((host, rest)) => !host.is_empty() && rest.starts_with('/') && !source.starts_with('/'),
        None => false,
    }
}

/// Reads the entries of a directory place and files them under it (§15.1, §15.4, §33.3).
///
/// §33.3 makes the filesystem query-driven, so nothing walks it until somebody stands in it; and
/// then the whole listing is read, because §15.4's summary is a statement about how many entries
/// there *are*, and a count taken from a truncated read would be a number from nowhere (§2.17).
/// What bounds the *view* is the neighborhood budget, not the read (§34.2).
///
/// The listing is remembered like any other provider answer, so standing in a directory and
/// looking twice reads it once (§33.1, ADR-0186).
pub(crate) async fn observe_children(
    providers: &ono_provider_api::ProviderRegistry,
    state: &mut crate::spatial::SpatialSessionState,
    place: &SpatialId,
    path: &Path,
    now: Timestamp,
) {
    if providers.for_target("dir").is_empty() {
        return;
    }
    let key = format!("dir:{}", path.display());
    if let Some(seen) = state.recall(&key, now) {
        // The entries are already in the index, filed under this place: nothing to do but not
        // ask again.
        let _ = seen;
        return;
    }
    let query = Query::target("dir").with(Selector::field(
        "path",
        Value::Path(std::sync::Arc::from(path)),
    ));
    let mut observation = crate::spatial::session::TargetObservation {
        at: now,
        places: std::collections::BTreeMap::new(),
        refusal: None,
        served: false,
        // §33.3 makes the filesystem query-driven: this asks about one named path, so what came
        // back is the whole answer and there is no population a bound left out (ADR-0576).
        population: None,
        bounded: false,
    };
    match providers.snapshot(&query) {
        Err(error) => {
            observation.refusal = Some((
                ono_spatial_core::PermissionState::of_refusal(&error),
                error.message().to_owned(),
            ));
        }
        Ok(stream) => {
            let collected = stream.collect().await;
            if let Some(error) = collected.errors().first() {
                observation.refusal = Some((
                    ono_spatial_core::PermissionState::of_refusal(error),
                    error.message().to_owned(),
                ));
            }
            let records: Vec<RecordValue> = collected
                .into_values()
                .into_iter()
                .filter_map(|value| match value {
                    Value::Record(record) => Some(RecordValue::clone(&record)),
                    _ => None,
                })
                .collect();
            if !records.is_empty() || observation.refusal.is_none() {
                observation.served = true;
                state.absorb(&records, now);
                for record in &records {
                    let Ok(child) = state.projection_of_object(record) else {
                        continue;
                    };
                    let id = child.spatial_id().clone();
                    observation
                        .places
                        .entry(child.object_type())
                        .or_default()
                        .push(id.clone());
                    let (index, _) = state.absorb_with();
                    index.set_path_parent(&id, place);
                }
            }
        }
    }
    if let Some((state_word, detail)) = observation.refusal.clone() {
        let (index, _) = state.absorb_with();
        index.record_withheld(place, "children", state_word, &detail);
    }
    state.remember(key, observation);
}
