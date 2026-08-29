//! Host identity: pinned keys, an explicit trust store, and a refusal that is never a prompt.
//!
//! Spec §21.5 requires "authenticated encryption, explicit host trust and least privilege", and
//! spec §49 lists remote agent impersonation and host key changes among the threats a shell must
//! answer for. ADR-0015 makes those rows T5 and T6, owned by this crate.
//!
//! # What this module decides, and what it does not
//!
//! It decides **whether a key is the key this host had last time**. It does not perform the
//! cryptography that established the key: that belongs to the [`Transport`](crate::Transport),
//! which reports what it authenticated. Separating the two means the pinning logic is testable
//! without a key exchange, and a deployment can supply TLS, Noise or an SSH channel without this
//! module changing.
//!
//! # The rules
//!
//! - A **changed** key is [`ErrorCode::RemoteHostKeyChanged`] (E0603), which ADR-0006 classifies
//!   as `safety` rather than transport, because it is a trust decision.
//! - A refusal is **never a prompt** (ADR-0015 standing rule 4). Nothing here offers "continue
//!   anyway": a script would eventually answer it, and then the mitigation is gone. Re-trusting a
//!   host is [`TrustStore::repin`] — a separate, deliberate act, or an edit of the file by hand.
//! - The store is **an explicit file a person can read and edit**, one line per peer.
//!
//! # The file
//!
//! ```text
//! # ono trust store: one line per peer, `<host> <algorithm> <fingerprint>`
//! # A line whose fingerprint does not match what the peer presents refuses the link.
//! db.example.com ed25519 sha256:1f0c…
//! ```
//!
//! Blank lines and lines beginning with `#` are ignored. A line that is not one of those and not
//! three fields is a parse error naming its line number, rather than a line quietly skipped —
//! a trust store that silently drops what it cannot read is a trust store that silently stops
//! protecting a host.

use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use ono_core::ErrorCode;
use ono_value::ErrorValue;
use sha2::{Digest as _, Sha256};

/// A peer's public key, as the transport authenticated it.
///
/// The material is opaque here: this crate compares it and hashes it, and never interprets it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKey {
    algorithm: String,
    material: Vec<u8>,
}

impl HostKey {
    /// A key of `algorithm` with this material.
    #[must_use]
    pub fn new(algorithm: impl Into<String>, material: impl Into<Vec<u8>>) -> Self {
        Self {
            algorithm: algorithm.into(),
            material: material.into(),
        }
    }

    /// The key algorithm, as the trust store spells it.
    #[must_use]
    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }

    /// The key material.
    #[must_use]
    pub fn material(&self) -> &[u8] {
        &self.material
    }

    /// The key's fingerprint: what the trust store records and compares.
    ///
    /// The algorithm name is hashed along with the material, so the same bytes offered under two
    /// algorithm names are two different keys.
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        let mut hasher = Sha256::new();
        hasher.update(self.algorithm.as_bytes());
        hasher.update([0u8]);
        hasher.update(&self.material);
        let digest = hasher.finalize();
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&digest);
        Fingerprint(bytes)
    }
}

/// The SHA-256 fingerprint of a [`HostKey`], rendered as `sha256:<64 hex digits>`.
///
/// The full digest, never a truncation: a shortened fingerprint is a fingerprint an impersonator
/// can search for a collision against, and the whole point of pinning is that they cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Fingerprint([u8; 32]);

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("sha256:")?;
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for Fingerprint {
    type Err = ErrorValue;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let malformed = || {
            ErrorValue::new(
                ErrorCode::ParseSyntax,
                format!("`{text}` is not a fingerprint; expected `sha256:` and 64 hex digits"),
            )
        };
        let digits = text.strip_prefix("sha256:").ok_or_else(malformed)?;
        if digits.len() != 64 {
            return Err(malformed());
        }
        let mut bytes = [0u8; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            let pair = digits.get(index * 2..index * 2 + 2).ok_or_else(malformed)?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| malformed())?;
        }
        Ok(Fingerprint(bytes))
    }
}

/// What the trust store concluded about a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustDecision {
    /// The key matches the one recorded for this host.
    Pinned,
    /// This host had no recorded key, and this one has now been recorded.
    NewlyPinned,
    /// The transport authenticated nobody, and the policy allowed that.
    Unauthenticated,
}

/// How much a link demands of a peer's identity before it will carry anything.
///
/// A changed key is refused under every policy: the policy decides what happens to a peer that
/// is *unknown*, never to a peer that contradicts what is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrustPolicy {
    /// The transport must authenticate a peer key. An unknown key is recorded and the link
    /// proceeds; a changed key is refused.
    ///
    /// Trust on first use: the first machine to answer for a name becomes that name. It is a
    /// deliberate choice a caller has to make, never what a caller gets by saying nothing
    /// (ADR-0354).
    Required,
    /// The key must already be in the store. An unknown key is refused, so a host is trusted
    /// only by a deliberate act taken beforehand. This is the default (ADR-0015 T5, ADR-0354).
    #[default]
    Pinned,
    /// A transport that authenticates nobody is accepted.
    ///
    /// Named so that nobody enables it by accident: it turns off the mitigation for ADR-0015 T5
    /// entirely, and exists for a transport whose peer is not reachable by anyone else — a unix
    /// socket in the shell's own runtime directory, or an in-process test duplex.
    Unauthenticated,
}

/// One recorded peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustEntry {
    host: String,
    algorithm: String,
    fingerprint: Fingerprint,
}

impl TrustEntry {
    /// The host, as the user named it when the link was made.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The key algorithm.
    #[must_use]
    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }

    /// The pinned fingerprint.
    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        self.fingerprint
    }
}

/// The pinned host keys, and the file they live in.
///
/// Cloning shares the store rather than copying it, so the shell and a link being established
/// are looking at the same pins.
///
/// ```
/// use ono_protocol::{HostKey, TrustDecision, TrustStore};
///
/// let store = TrustStore::in_memory();
/// let key = HostKey::new("ed25519", b"a key".to_vec());
/// assert_eq!(store.verify("db.example.com", &key)?, TrustDecision::NewlyPinned);
/// assert_eq!(store.verify("db.example.com", &key)?, TrustDecision::Pinned);
/// # Ok::<(), ono_value::ErrorValue>(())
/// ```
#[derive(Debug, Clone, Default)]
pub struct TrustStore {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Debug, Default)]
struct Inner {
    entries: Vec<TrustEntry>,
    path: Option<PathBuf>,
}

impl TrustStore {
    /// A store that keeps its pins only for as long as it lives.
    #[must_use]
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// The store recorded in `path`, or an empty store when the file does not exist yet.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::IoPermissionDenied`] or [`ErrorCode::IoNotFound`] when the file
    /// exists but cannot be read, and [`ErrorCode::ParseSyntax`] naming the line number when a
    /// line is not one this format defines.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ErrorValue> {
        let path = path.as_ref().to_path_buf();
        let entries = match std::fs::read_to_string(&path) {
            Ok(text) => parse(&text, &path)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => return Err(io_error(&path, &error)),
        };
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                entries,
                path: Some(path),
            })),
        })
    }

    /// The fingerprint recorded for `host`, if any.
    #[must_use]
    pub fn fingerprint(&self, host: &str) -> Option<Fingerprint> {
        self.with(|inner| {
            inner
                .entries
                .iter()
                .find(|entry| entry.host == host)
                .map(TrustEntry::fingerprint)
        })
    }

    /// Every recorded peer, in the order the file lists them.
    #[must_use]
    pub fn entries(&self) -> Vec<TrustEntry> {
        self.with(|inner| inner.entries.clone())
    }

    /// Checks `key` against what is recorded for `host`, recording it if nothing was.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::RemoteHostKeyChanged`] when `host` has a different key recorded. The
    /// error carries both fingerprints, because a user cannot judge what happened without seeing
    /// them, and it carries no way to proceed, because ADR-0015 standing rule 4 forbids one.
    pub fn verify(&self, host: &str, key: &HostKey) -> Result<TrustDecision, ErrorValue> {
        let presented = key.fingerprint();
        match self.fingerprint(host) {
            Some(recorded) if recorded == presented => Ok(TrustDecision::Pinned),
            Some(recorded) => Err(host_key_changed(host, recorded, presented)),
            None => {
                self.write_entry(host, key)?;
                Ok(TrustDecision::NewlyPinned)
            }
        }
    }

    /// Records `key` for `host`.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::RemoteHostKeyChanged`] when a *different* key is already recorded:
    /// replacing one is [`repin`](Self::repin), so that re-trusting a host can never be something
    /// that merely happened.
    pub fn pin(&self, host: &str, key: &HostKey) -> Result<(), ErrorValue> {
        let presented = key.fingerprint();
        match self.fingerprint(host) {
            Some(recorded) if recorded == presented => Ok(()),
            Some(recorded) => Err(host_key_changed(host, recorded, presented)),
            None => self.write_entry(host, key),
        }
    }

    /// Replaces whatever is recorded for `host` with `key`.
    ///
    /// This is the deliberate act that re-trusts a host whose key really did change — a rebuilt
    /// server, a rotated key. It is never reached from establishing a link; a user or an explicit
    /// command calls it, having checked the new fingerprint by some other means.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the store's file cannot be written.
    pub fn repin(&self, host: &str, key: &HostKey) -> Result<(), ErrorValue> {
        self.with(|inner| inner.entries.retain(|entry| entry.host != host));
        self.write_entry(host, key)
    }

    /// Records `fingerprint` for `host` under `algorithm`, refusing to replace a different key.
    ///
    /// This is [`pin`](Self::pin) for a caller that holds a fingerprint rather than a key — a
    /// person who read one off the host's own console, which is the only channel that makes a
    /// first pin worth anything. Nothing is weakened by it: the store compares fingerprints, and
    /// a fingerprint is what it records.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorCode::RemoteHostKeyChanged`] when a different fingerprint is recorded.
    pub fn pin_fingerprint(
        &self,
        host: &str,
        algorithm: &str,
        fingerprint: Fingerprint,
    ) -> Result<(), ErrorValue> {
        match self.fingerprint(host) {
            Some(recorded) if recorded == fingerprint => Ok(()),
            Some(recorded) => Err(host_key_changed(host, recorded, fingerprint)),
            None => self.record(host, algorithm, fingerprint),
        }
    }

    /// Replaces whatever is recorded for `host` with `fingerprint` — the deliberate re-trust.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the store's file cannot be written.
    pub fn repin_fingerprint(
        &self,
        host: &str,
        algorithm: &str,
        fingerprint: Fingerprint,
    ) -> Result<(), ErrorValue> {
        self.with(|inner| inner.entries.retain(|entry| entry.host != host));
        self.record(host, algorithm, fingerprint)
    }

    /// Forgets `host`, so it is trusted again only by a deliberate act.
    ///
    /// A host that is not recorded is already forgotten, so this succeeds; the caller decides
    /// whether asking about an unknown host is worth a refusal of its own.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the store's file cannot be written.
    pub fn forget(&self, host: &str) -> Result<(), ErrorValue> {
        let (path, rendered) = self.with(|inner| {
            inner.entries.retain(|entry| entry.host != host);
            (inner.path.clone(), render(&inner.entries))
        });
        let Some(path) = path else {
            return Ok(());
        };
        persist(&path, &rendered)
    }

    fn write_entry(&self, host: &str, key: &HostKey) -> Result<(), ErrorValue> {
        self.record(host, key.algorithm(), key.fingerprint())
    }

    fn record(
        &self,
        host: &str,
        algorithm: &str,
        fingerprint: Fingerprint,
    ) -> Result<(), ErrorValue> {
        let entry = TrustEntry {
            host: host.to_owned(),
            algorithm: algorithm.to_owned(),
            fingerprint,
        };
        let (path, rendered) = self.with(|inner| {
            inner.entries.push(entry);
            (inner.path.clone(), render(&inner.entries))
        });
        let Some(path) = path else {
            return Ok(());
        };
        persist(&path, &rendered)
    }

    fn with<T>(&self, body: impl FnOnce(&mut Inner) -> T) -> T {
        // A poisoned lock means another thread panicked while holding it; the pins it saw are
        // still the pins, so recovering is strictly better than turning a panic into a refusal to
        // ever check a host key again.
        let mut guard = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        body(&mut guard)
    }
}

/// The header written above the pins, so a person opening the file knows what it is.
const FILE_HEADER: &str = "\
# ono trust store: one line per peer, `<host> <algorithm> <fingerprint>`.
# A peer presenting a different key than the line here records is refused, not prompted.
# Remove a line to forget a host; edit one only if you have checked the new key another way.
";

fn render(entries: &[TrustEntry]) -> String {
    let mut text = String::from(FILE_HEADER);
    for entry in entries {
        text.push_str(&format!(
            "{} {} {}\n",
            entry.host, entry.algorithm, entry.fingerprint
        ));
    }
    text
}

fn persist(path: &Path, contents: &str) -> Result<(), ErrorValue> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|error| io_error(parent, &error))?;
    }
    // Written whole and replaced, so a store is never half a store: a truncated trust store would
    // silently un-pin every host below the point the write stopped.
    let temporary = path.with_extension("tmp");
    let mut file =
        std::fs::File::create(&temporary).map_err(|error| io_error(&temporary, &error))?;
    file.write_all(contents.as_bytes())
        .map_err(|error| io_error(&temporary, &error))?;
    file.sync_all()
        .map_err(|error| io_error(&temporary, &error))?;
    drop(file);
    std::fs::rename(&temporary, path).map_err(|error| io_error(path, &error))
}

fn parse(text: &str, path: &Path) -> Result<Vec<TrustEntry>, ErrorValue> {
    let mut entries = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [host, algorithm, fingerprint] = fields.as_slice() else {
            return Err(ErrorValue::new(
                ErrorCode::ParseSyntax,
                format!(
                    "{}: line {} is not `<host> <algorithm> <fingerprint>`",
                    path.display(),
                    index + 1
                ),
            )
            .with_help("remove the line, or restore it to three whitespace-separated fields"));
        };
        entries.push(TrustEntry {
            host: (*host).to_owned(),
            algorithm: (*algorithm).to_owned(),
            fingerprint: fingerprint.parse()?,
        });
    }
    Ok(entries)
}

fn io_error(path: &Path, error: &std::io::Error) -> ErrorValue {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => ErrorCode::IoNotFound,
        std::io::ErrorKind::PermissionDenied => ErrorCode::IoPermissionDenied,
        _ => ErrorCode::IoNotFound,
    };
    ErrorValue::new(
        code,
        format!(
            "the trust store at {} is not usable: {error}",
            path.display()
        ),
    )
}

fn host_key_changed(host: &str, recorded: Fingerprint, presented: Fingerprint) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::RemoteHostKeyChanged,
        format!("{host} presented {presented}, and the trust store records {recorded} for it"),
    )
    .with_retryable(false)
    .with_help(
        "the link was not established. Either the host's key really was replaced, or something \
         is answering in its place. Confirm the new key by some other means, then re-trust the \
         host deliberately; nothing here will do it for you.",
    )
}

/// Decides what a policy makes of the key a transport authenticated.
pub(crate) fn decide(
    policy: TrustPolicy,
    store: &TrustStore,
    host: &str,
    key: Option<&HostKey>,
) -> Result<TrustDecision, ErrorValue> {
    match (policy, key) {
        (TrustPolicy::Unauthenticated, None) => Ok(TrustDecision::Unauthenticated),
        (TrustPolicy::Unauthenticated, Some(key)) => store.verify(host, key),
        (_, None) => Err(ErrorValue::new(
            ErrorCode::SafetyPolicyDenied,
            format!(
                "the transport to {host} authenticates nobody, and this link requires a host key"
            ),
        )
        .with_retryable(false)
        .with_help(
            "spec §21.5 requires authenticated encryption on a remote link; use a transport that \
             authenticates the peer, or say so explicitly by asking for an unauthenticated link",
        )),
        (TrustPolicy::Required, Some(key)) => store.verify(host, key),
        (TrustPolicy::Pinned, Some(key)) => {
            let presented = key.fingerprint();
            match store.fingerprint(host) {
                Some(recorded) if recorded == presented => Ok(TrustDecision::Pinned),
                Some(recorded) => Err(host_key_changed(host, recorded, presented)),
                None => Err(ErrorValue::new(
                    ErrorCode::SafetyPolicyDenied,
                    format!(
                        "{host} is not in the trust store, and this link only uses pinned hosts"
                    ),
                )
                .with_retryable(false)
                .with_help(format!(
                    "the host presented {presented}. If that is the right key, trust it \
                     deliberately before connecting; nothing was recorded."
                ))),
            }
        }
    }
}
