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

use std::path::{Path, PathBuf};

use ono_core::ErrorCode;
use ono_protocol::{
    ActionGrant, AuthorizedClient, AuthorizedClients, Fingerprint, TrustEntry, TrustStore,
};
use ono_remote::PeerIdentity;
use ono_value::ErrorValue;

/// The file the pins live in, under the configuration directory of ADR-0010.
pub const STORE_FILE: &str = "trusted_hosts";

/// The file a listening agent keeps its own identity in, under the same directory.
///
/// The name v0.4.0 gave it, when only a listening host had an identity. v0.4.1 §8.1 renames the
/// canonical file to [`LINK_IDENTITY_FILE`]; this one is still read, still written by
/// `--agent --host-key`, and never deleted (§8.2 rule 4).
pub const HOST_KEY_FILE: &str = "host_key.pem";

/// The file this shell keeps its own peer identity in (v0.4.1 §8.1).
///
/// `~/.config/ono/link_identity.pem`, under whatever configuration directory ADR-0010's
/// resolution answers — `ONO_CONFIG_DIR`, then XDG, then `HOME`.
pub const LINK_IDENTITY_FILE: &str = "link_identity.pem";

/// The identity this shell proves it holds on a direct link, from `directory` (v0.4.1 §8.1, §8.2).
///
/// Both ends of a direct TCP link present a certificate now (§7.1), so the identity is no longer
/// something only a listening agent has. §8.2 fixes what a machine that already ran one does with
/// the file it already has, and the ladder is followed in the order the specification writes it:
///
/// 1. `link_identity.pem` exists — use it;
/// 2. else `host_key.pem` exists *and parses* — copy it across, preserving mode `0600`;
/// 3. else generate `link_identity.pem`;
/// 4. the legacy file is never deleted.
///
/// The copy is one-time and by value rather than a symlink or a read fallback (ADR-0435): after
/// it, one file is the identity and the other is a file the old flag still names.
///
/// # Errors
///
/// `parse.syntax` when the canonical file exists and is not an identity, `io.permission_denied`
/// when it cannot be written, and the security refusal of §8.3 when its permissions expose the
/// private key.
pub fn link_identity(directory: &Path) -> Result<PeerIdentity, ErrorValue> {
    let canonical = directory.join(LINK_IDENTITY_FILE);
    if canonical.exists() {
        return PeerIdentity::open_or_create(&canonical);
    }
    let legacy = directory.join(HOST_KEY_FILE);
    if legacy.is_file() {
        match PeerIdentity::open_or_create(&legacy) {
            Ok(_) => migrate(&legacy, &canonical)?,
            // §8.3's refusal travels: an exposed legacy key must not be quietly stepped over,
            // because stepping over it is exactly the "second unrelated identity" §8.2 forbids,
            // and it would generate one out of a security problem the operator never saw.
            Err(error) if error.code() == ErrorCode::RemoteIdentityPermissions => {
                return Err(error);
            }
            // Rule 2 is conditional on the legacy file *parsing*: a `host_key.pem` that is not
            // an identity is not an identity to inherit, and refusing here would leave a shell
            // unable to link because of a file no current path writes.
            Err(_) => {}
        }
    }
    PeerIdentity::open_or_create(&canonical)
}

/// The identity this session presents on a direct link, from its configuration directory.
///
/// A session with no configuration directory — no `HOME`, no `XDG_CONFIG_HOME`, no
/// `ONO_CONFIG_DIR` — has nowhere to keep an identity, and v0.4.1 §8.1 wants a *persistent* one.
/// Generating a fresh key per process instead would make every direct link a first contact and
/// every authorization the operator granted worthless, so it refuses and says where to put one.
///
/// # Errors
///
/// `io.not_found` when there is no configuration directory, and as [`link_identity`] otherwise.
pub fn identity(sources: &crate::hosts::HostSources) -> Result<PeerIdentity, ErrorValue> {
    let Some(directory) = &sources.config_dir else {
        return Err(ErrorValue::new(
            ErrorCode::IoNotFound,
            "this account has no configuration directory to keep a peer identity in",
        )
        .with_help(
            "a direct link proves who this shell is with a persistent key (v0.4.1 §8.1). Set \
             `HOME`, `XDG_CONFIG_HOME` or `ONO_CONFIG_DIR` so there is somewhere to keep one.",
        ));
    };
    link_identity(directory)
}

/// Copies `legacy` to `canonical` with mode `0600`, without ever exposing a half-written key.
///
/// Written to a temporary file in the same directory and renamed, so the canonical path is either
/// absent or a complete identity — a truncated private key would be an identity nobody can prove
/// and every peer has pinned.
fn migrate(legacy: &Path, canonical: &Path) -> Result<(), ErrorValue> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    let contents = std::fs::read(legacy).map_err(|error| identity_io_error(legacy, &error))?;
    let temporary = canonical.with_extension("pem.tmp");
    let _ = std::fs::remove_file(&temporary);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|error| identity_io_error(&temporary, &error))?;
    file.write_all(&contents)
        .map_err(|error| identity_io_error(&temporary, &error))?;
    file.sync_all()
        .map_err(|error| identity_io_error(&temporary, &error))?;
    drop(file);
    std::fs::rename(&temporary, canonical).map_err(|error| identity_io_error(canonical, &error))
}

fn identity_io_error(path: &Path, error: &std::io::Error) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::IoPermissionDenied,
        format!(
            "the peer identity at {} is not usable: {error}",
            path.display()
        ),
    )
}

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

// --- the clients this machine authorizes (v0.4.1 §9, ADR-0468) --------------------------------
//
// The other half of remote trust, and the half v0.4.0 did not have. `trusted_hosts` above is
// which machines this shell will *link to*; `authorized_clients` is which clients its listening
// agent will *serve*. §9.1 is why the second is not implied by the first: a certificate proves
// which key connected, never that the operator wants that key here.
//
// `ono-protocol` owns the file format, the strict parser and the atomic writer, exactly as it
// owns the trust store's. This module owns where the file lives for a session and how a person
// sees and changes it.

/// The file the authorized client keys live in, under the configuration directory of ADR-0010.
///
/// `~/.config/ono/authorized_clients`, the reference path of v0.4.1 §9.2.
pub const AUTHORIZED_CLIENTS_FILE: &str = "authorized_clients";

/// The authorization store this session's configuration directory points at.
///
/// A session with no configuration directory has no store, and a store nobody wrote authorizes
/// nobody — which is the fail-closed answer §9.2 requires, not an inconvenience to work around.
///
/// # Errors
///
/// `remote.authorization_store_invalid` (E1204) naming the line when the file is not the format
/// §9.2 defines. Never an empty store: §9.2 forbids reading a malformed one as one.
pub fn open_authorized_clients(
    sources: &crate::hosts::HostSources,
) -> Result<AuthorizedClients, ErrorValue> {
    match authorized_clients_path(sources) {
        Some(path) => AuthorizedClients::open(path),
        None => Ok(AuthorizedClients::empty()),
    }
}

/// Where the authorized client keys are kept, when this session keeps them anywhere.
#[must_use]
pub fn authorized_clients_path(sources: &crate::hosts::HostSources) -> Option<PathBuf> {
    sources.authorized_clients.clone()
}

/// One authorized client, as `get client-key` shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientKeyRow {
    /// The full `sha256:` fingerprint of the client's key.
    pub fingerprint: String,
    /// What the operator called the client, where they called it anything.
    pub label: Option<String>,
    /// Whether the client may query and subscribe (§9.4).
    pub observe: bool,
    /// The exact capability ids the client may act with; empty for an observer.
    pub actions: Vec<String>,
    /// The file the grant is recorded in; `None` when this session keeps no store.
    pub path: Option<PathBuf>,
}

impl ClientKeyRow {
    /// The row for one store entry.
    #[must_use]
    pub fn of(entry: &AuthorizedClient, path: Option<&PathBuf>) -> Self {
        Self {
            fingerprint: entry.fingerprint().to_string(),
            label: entry.label().map(ToOwned::to_owned),
            observe: entry.observes(),
            actions: entry
                .actions()
                .iter()
                .map(|grant| grant.id().to_owned())
                .collect(),
            path: path.cloned(),
        }
    }
}

/// The rows `get client-key` answers with, in the order the file records them.
///
/// # Errors
///
/// As [`open_authorized_clients`].
pub fn client_key_rows(
    sources: &crate::hosts::HostSources,
) -> Result<Vec<ClientKeyRow>, ErrorValue> {
    let store = open_authorized_clients(sources)?;
    let path = authorized_clients_path(sources);
    Ok(store
        .entries()
        .iter()
        .map(|entry| ClientKeyRow::of(entry, path.as_ref()))
        .collect())
}

/// Authorizes `fingerprint` to observe this machine, and nothing else (§9.4).
///
/// The default grant is the whole of what adding a client does. An operator who wants an action
/// says so afterwards, with [`set_client_key`], naming the capability.
///
/// # Errors
///
/// `parse.syntax` for a fingerprint that is not one, `conflict.*` when the client is already
/// authorized — widening a grant is `set`, a separate act, exactly as re-trusting a host is —
/// and an I/O error when the store cannot be written.
pub fn authorize_client(
    sources: &crate::hosts::HostSources,
    fingerprint: &str,
    label: Option<&str>,
) -> Result<(), ErrorValue> {
    let fingerprint: Fingerprint = fingerprint.parse()?;
    let store = open_authorized_clients(sources)?;
    if store.client(fingerprint).is_some() {
        return Err(ErrorValue::new(
            ErrorCode::IoAlreadyExists,
            format!("{fingerprint} is already an authorized client"),
        )
        .with_help(
            "`set client-key <fingerprint> --allow …` changes what it may do; adding it again \
             would silently reset a grant somebody made deliberately",
        ));
    }
    let mut entry = AuthorizedClient::observing(fingerprint);
    if let Some(label) = label {
        entry = entry.with_label(label);
    }
    let mut entries = store.entries().to_vec();
    entries.push(entry);
    persist_authorized(sources, &entries)
}

/// Replaces what an authorized client may do (§9.5, §9.7).
///
/// `allow` is a comma-separated list of **exact** capability ids and never a pattern; each is
/// parsed as an [`ActionGrant`], so `*`, `process.*` and a risk class are refused rather than
/// stored. `observe` and `label` change only what they name; a field left `None` is left alone,
/// so `--allow` preserves observe state as §9.7 requires.
///
/// # Errors
///
/// `resolve.target_not_found` when the client is not authorized, the grant refusal when an
/// allowlist entry is not a capability id, and an I/O error when the store cannot be written.
pub fn set_client_key(
    sources: &crate::hosts::HostSources,
    fingerprint: &str,
    allow: Option<&str>,
    observe: Option<bool>,
    label: Option<&str>,
) -> Result<bool, ErrorValue> {
    let fingerprint: Fingerprint = fingerprint.parse()?;
    let store = open_authorized_clients(sources)?;
    let Some(existing) = store.client(fingerprint) else {
        return Err(unauthorized_client(fingerprint));
    };
    let mut entry = existing.clone();
    if let Some(allow) = allow {
        let mut granted = Vec::new();
        for id in allow.split(',').map(str::trim).filter(|id| !id.is_empty()) {
            granted.push(id.parse::<ActionGrant>()?);
        }
        entry = entry.with_actions(granted);
    }
    if let Some(observe) = observe {
        entry = entry.with_observe(observe);
    }
    if let Some(label) = label {
        entry = entry.with_label(label);
    }
    if &entry == existing {
        return Ok(false);
    }
    let entries: Vec<AuthorizedClient> = store
        .entries()
        .iter()
        .map(|candidate| {
            if candidate.fingerprint() == fingerprint {
                entry.clone()
            } else {
                candidate.clone()
            }
        })
        .collect();
    persist_authorized(sources, &entries)?;
    Ok(true)
}

/// Revokes a client, so its next connection is refused (§9.7, §12.5).
///
/// # Errors
///
/// `resolve.target_not_found` when the client is not authorized, and an I/O error when the store
/// cannot be written.
pub fn revoke_client(
    sources: &crate::hosts::HostSources,
    fingerprint: &str,
) -> Result<(), ErrorValue> {
    let fingerprint: Fingerprint = fingerprint.parse()?;
    let store = open_authorized_clients(sources)?;
    if store.client(fingerprint).is_none() {
        return Err(unauthorized_client(fingerprint));
    }
    let entries: Vec<AuthorizedClient> = store
        .entries()
        .iter()
        .filter(|entry| entry.fingerprint() != fingerprint)
        .cloned()
        .collect();
    persist_authorized(sources, &entries)
}

fn unauthorized_client(fingerprint: Fingerprint) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ResolveTargetNotFound,
        format!("{fingerprint} is not an authorized client of this machine"),
    )
    .with_help("`get client-key` lists the clients this machine authorizes")
}

fn persist_authorized(
    sources: &crate::hosts::HostSources,
    entries: &[AuthorizedClient],
) -> Result<(), ErrorValue> {
    let Some(path) = authorized_clients_path(sources) else {
        return Err(ErrorValue::new(
            ErrorCode::IoNotFound,
            "this account has no configuration directory to keep an authorization store in",
        )
        .with_help(
            "a listening agent decides who it serves from `~/.config/ono/authorized_clients` \
             (v0.4.1 section 9.2). Set `HOME`, `XDG_CONFIG_HOME` or `ONO_CONFIG_DIR` so there is \
             somewhere to keep one.",
        ));
    };
    ono_protocol::write_store(&path, entries)
}
