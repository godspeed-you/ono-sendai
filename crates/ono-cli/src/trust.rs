//! The pinned host keys this shell will accept, and the commands that read and change them
//! (spec §21.5, §49; ADR-0015 T5/T6, ADR-0355).
//!
//! `ono-protocol` owns the decision — is this the key this host had last time? — and the file
//! format. This module owns two things the protocol crate deliberately does not: **where** the
//! store lives for a session, and **how a person sees and changes it**.
//!
//! A pin is not an observation of a machine, it is a decision this shell recorded, so it is
//! session state answered by `ono.shell` exactly as the link table is (ADR-0090), and the four
//! commands are the ordinary verbs: `get host-key` shows what is pinned, `add host-key` records a
//! fingerprint checked by some other means, `set host-key` is the deliberate re-trust after a key
//! really changed, and `remove host-key` forgets one. There is no "continue anyway" anywhere,
//! because ADR-0015 standing rule 4 forbids one.

use std::path::PathBuf;

use ono_core::ErrorCode;
use ono_protocol::{Fingerprint, TrustEntry, TrustStore};
use ono_value::ErrorValue;

/// The file the pins live in, under the configuration directory of ADR-0010.
pub const STORE_FILE: &str = "trusted_hosts";

/// The file a listening agent keeps its own identity in, under the same directory.
pub const HOST_KEY_FILE: &str = "host_key.pem";

/// How a key was proved, when a command does not say.
///
/// The authenticated transport of spec §21.5 is the only thing that pins a key on its own today
/// (ADR-0353), and it proves an X.509 certificate.
pub const DEFAULT_ALGORITHM: &str = "tls-x509";

/// The trust store the session's configuration directory points at.
///
/// A session with no configuration directory — no `HOME`, no `XDG_CONFIG_HOME`, no
/// `ONO_CONFIG_DIR` — gets a store that lives only as long as the process. That is honest rather
/// than convenient: nothing is silently written somewhere nobody will look for it, and
/// [`store_path`] answers `None` so `get host-key` can say where the pins are not being kept.
///
/// # Errors
///
/// `parse.syntax` naming the line when the file is not the format
/// [`TrustStore`] defines, and an I/O error when it cannot be read. A
/// store that cannot be read is never treated as an empty store: that would silently un-pin
/// every host in it.
pub fn open(sources: &crate::hosts::HostSources) -> Result<TrustStore, ErrorValue> {
    match store_path(sources) {
        Some(path) => TrustStore::open(path),
        None => Ok(TrustStore::in_memory()),
    }
}

/// Where the pins are kept, when this session keeps them anywhere.
#[must_use]
pub fn store_path(sources: &crate::hosts::HostSources) -> Option<PathBuf> {
    sources.trust_store.clone()
}

/// One pinned key, as `get host-key` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRow {
    /// The host the key is pinned for.
    pub host: String,
    /// How the key was proved.
    pub algorithm: String,
    /// The full `sha256:` fingerprint.
    pub fingerprint: String,
    /// The file the pin is recorded in; `None` when this session keeps no store.
    pub path: Option<PathBuf>,
}

impl KeyRow {
    /// The row for one store entry.
    #[must_use]
    pub fn of(entry: &TrustEntry, path: Option<&PathBuf>) -> Self {
        Self {
            host: entry.host().to_owned(),
            algorithm: entry.algorithm().to_owned(),
            fingerprint: entry.fingerprint().to_string(),
            path: path.cloned(),
        }
    }
}

/// The rows `get host-key` answers with, in the order the file records them.
///
/// # Errors
///
/// As [`open`].
pub fn rows(sources: &crate::hosts::HostSources) -> Result<Vec<KeyRow>, ErrorValue> {
    let store = open(sources)?;
    let path = store_path(sources);
    Ok(store
        .entries()
        .iter()
        .map(|entry| KeyRow::of(entry, path.as_ref()))
        .collect())
}

/// The fingerprint a person typed, or a refusal that says what one looks like.
///
/// A pin made by hand carries no key material and needs none: what a person reads off the host's
/// own console is the fingerprint, and the fingerprint is what the store compares.
///
/// # Errors
///
/// `parse.syntax` when `fingerprint` is not `sha256:` and 64 hex digits.
fn parse_fingerprint(fingerprint: &str) -> Result<Fingerprint, ErrorValue> {
    fingerprint.parse()
}

/// Pins `fingerprint` for `host`, refusing to replace a different key.
///
/// # Errors
///
/// `remote.host_key_changed` (E0603) when a *different* key is already pinned — replacing one is
/// [`repin`], a separate and deliberate act — and `parse.syntax` for a fingerprint that is not
/// one.
pub fn pin(
    sources: &crate::hosts::HostSources,
    host: &str,
    algorithm: &str,
    fingerprint: &str,
) -> Result<bool, ErrorValue> {
    let fingerprint = parse_fingerprint(fingerprint)?;
    let store = open(sources)?;
    if store.fingerprint(host) == Some(fingerprint) {
        return Ok(false);
    }
    store.pin_fingerprint(host, algorithm, fingerprint)?;
    Ok(true)
}

/// Replaces whatever is pinned for `host` — the deliberate re-trust of ADR-0015 T6.
///
/// # Errors
///
/// `parse.syntax` for a fingerprint that is not one, and an I/O error when the store cannot be
/// written.
pub fn repin(
    sources: &crate::hosts::HostSources,
    host: &str,
    algorithm: &str,
    fingerprint: &str,
) -> Result<bool, ErrorValue> {
    let fingerprint = parse_fingerprint(fingerprint)?;
    let store = open(sources)?;
    let already = store.fingerprint(host) == Some(fingerprint);
    store.repin_fingerprint(host, algorithm, fingerprint)?;
    Ok(!already)
}

/// Forgets the pin for `host`, so it must be trusted again deliberately.
///
/// # Errors
///
/// `resolve.target_not_found` when nothing is pinned for `host`, and an I/O error when the store
/// cannot be written.
pub fn forget(sources: &crate::hosts::HostSources, host: &str) -> Result<(), ErrorValue> {
    let store = open(sources)?;
    if store.fingerprint(host).is_none() {
        return Err(ErrorValue::new(
            ErrorCode::ResolveTargetNotFound,
            format!("no host key is pinned for `{host}`"),
        )
        .with_help("`get host-key` lists what this shell has pinned"));
    }
    store.forget(host)
}
