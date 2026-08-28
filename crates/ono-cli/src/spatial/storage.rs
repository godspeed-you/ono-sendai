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
    if path.is_absolute() {
        path
    } else {
        session.cwd().join(path)
    }
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
    let query = Query::target("file").with(Selector::field(
        "path",
        Value::Path(std::sync::Arc::from(path)),
    ));
    let providers = providers.clone();
    let now = Timestamp::now();
    let display = path.display().to_string();

    runtime.block_on(async move {
        let records: Vec<RecordValue> = match providers.snapshot(&query) {
            Ok(stream) => stream
                .collect()
                .await
                .into_values()
                .into_iter()
                .filter_map(|value| match value {
                    Value::Record(record) => Some(RecordValue::clone(&record)),
                    _ => None,
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        let Some(record) = records.first() else {
            return Err(ErrorValue::new(
                ErrorCode::SpatialNotFound,
                format!("no place answers to `{display}`"),
            )
            .with_help("the path does not exist, or no provider can read it (spec v0.4 §40)"));
        };
        let mut state = crate::spatial::spatial_session().await;
        let there = state.projection_of(record)?;
        state.absorb(&records, now);
        let is_directory = state
            .index()
            .get(&there)
            .is_some_and(|entry| entry.object().object_type() == SpatialType::Directory);
        let here = state.current_place().clone();
        if here != there {
            state.trail_mut().record(NavigationStep::new(
                now,
                here,
                there.clone(),
                Movement::Enter,
            ));
        }
        Ok((there, is_directory))
    })
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
