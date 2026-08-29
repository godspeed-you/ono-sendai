//! Whose keys this machine accepts (spec §31.36, ADR-0312).
//!
//! Publisher trust is a different question from "did a key sign these bytes", and its answer
//! lives somewhere a package cannot reach: two files outside the plugin home, one the machine's
//! administrator writes and one the operator writes. A valid signature from a key that is in
//! neither is `unknown`, never trusted — the signature proves who signed, not whether to care.

use std::path::{Path, PathBuf};

use ono_core::ErrorCode;
use ono_kuang_protocol::PublicKey;
use ono_value::ErrorValue;

/// The file both stores are called.
pub const TRUST_FILE: &str = "trust.yaml";

/// The `format:` line a trust store opens with.
const TRUST_FORMAT: &str = "kuang-trust/1";

/// Where the system-wide store lives when the environment names no other.
const SYSTEM_TRUST_PATH: &str = "/etc/ono/kuang/trust.yaml";

/// Which store an entry came from — a machine-wide decision or the operator's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// `/etc/ono/kuang/trust.yaml`: whoever administers the machine.
    System,
    /// The operator's configuration directory.
    User,
}

/// What a store says about one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// Accepted.
    Trusted,
    /// Accepted once and no longer. A positive statement, not an absence.
    Revoked,
}

/// The answer to spec §31.36's "do I trust that key or publisher?", in the vocabulary
/// `ono.verification-result/1` declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// The key is trusted machine-wide.
    SystemTrusted,
    /// The key is trusted by this operator.
    UserTrusted,
    /// The key is revoked in a store. This blocks (K11005).
    Untrusted,
    /// No store says anything about the key, or there is no key to ask about.
    Unknown,
}

impl Trust {
    /// The word `ono.verification-result/1.trust` carries.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::SystemTrusted => "system-trusted",
            Self::UserTrusted => "user-trusted",
            Self::Untrusted => "untrusted",
            Self::Unknown => "unknown",
        }
    }

    /// Whether this answer prevents installing and loading (ADR-0312).
    #[must_use]
    pub const fn blocks(self) -> bool {
        matches!(self, Self::Untrusted)
    }
}

/// One enrolled key.
#[derive(Debug, Clone)]
struct Entry {
    origin: Origin,
    publisher: String,
    key: PublicKey,
    standing: Standing,
}

/// The keys this machine accepts, system store first.
#[derive(Debug, Clone, Default)]
pub struct TrustStore {
    entries: Vec<Entry>,
}

/// A store as it is written.
#[derive(Debug, serde::Deserialize)]
struct Document {
    format: String,
    #[serde(default)]
    keys: Vec<RawEntry>,
}

/// One line of a store.
#[derive(Debug, serde::Deserialize)]
struct RawEntry {
    publisher: String,
    key: String,
    #[serde(default = "trusted_word")]
    trust: String,
    #[serde(default)]
    #[allow(dead_code, reason = "the operator's note, read by the operator")]
    comment: Option<String>,
}

fn trusted_word() -> String {
    "trusted".to_owned()
}

/// Where the machine-wide store is: `ONO_KUANG_SYSTEM_TRUST`, else `/etc/ono/kuang/trust.yaml`.
#[must_use]
pub fn system_path(named: Option<&std::ffi::OsStr>) -> PathBuf {
    named.map_or_else(|| PathBuf::from(SYSTEM_TRUST_PATH), PathBuf::from)
}

/// Where the operator's store is, under the configuration directory of ADR-0010.
#[must_use]
pub fn user_path(config_dir: Option<&Path>) -> Option<PathBuf> {
    config_dir.map(|dir| dir.join("kuang").join(TRUST_FILE))
}

impl TrustStore {
    /// Reads both stores. A store that does not exist is empty; a store that exists and does not
    /// parse is a problem the caller shows, and its keys are not silently treated as absent.
    #[must_use]
    pub fn read(system: Option<&Path>, user: Option<&Path>) -> (Self, Vec<ErrorValue>) {
        let mut store = Self::default();
        let mut problems = Vec::new();
        for (origin, path) in [(Origin::System, system), (Origin::User, user)] {
            let Some(path) = path else { continue };
            match read_one(origin, path) {
                Ok(entries) => store.entries.extend(entries),
                Err(problem) => problems.push(problem),
            }
        }
        (store, problems)
    }

    /// What the stores say about `key` signing for `publisher`.
    ///
    /// Both must match. A key enrolled for one publisher does not vouch for another, which is
    /// what stops one accepted key from covering every namespace. A revocation wins over a
    /// trust, wherever either was written.
    #[must_use]
    pub fn judge(&self, publisher: &str, key: &PublicKey) -> Trust {
        let matching = || {
            self.entries
                .iter()
                .filter(|entry| entry.publisher == publisher && entry.key == *key)
        };
        if matching().any(|entry| entry.standing == Standing::Revoked) {
            return Trust::Untrusted;
        }
        let trusted = |origin: Origin| {
            matching().any(|entry| entry.origin == origin && entry.standing == Standing::Trusted)
        };
        if trusted(Origin::System) {
            Trust::SystemTrusted
        } else if trusted(Origin::User) {
            Trust::UserTrusted
        } else {
            Trust::Unknown
        }
    }

    /// How many keys are enrolled, over both stores.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no store enrolls anything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn read_one(origin: Origin, path: &Path) -> Result<Vec<Entry>, ErrorValue> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Ok(Vec::new());
    };
    let refuse = |detail: String| {
        ErrorValue::new(
            ErrorCode::ProviderSchemaViolation,
            format!("{}: {detail}", path.display()),
        )
        .with_help("a trust store is a `kuang-trust/1` document (spec §31.36, ADR-0312)")
    };
    let depth = ono_value::yaml_depth(&text);
    if depth > ono_value::MAX_YAML_DEPTH {
        return Err(refuse(format!(
            "the store nests {depth} collections deep, and {} is the limit",
            ono_value::MAX_YAML_DEPTH
        )));
    }
    let document: Document =
        serde_yaml_ng::from_str(&text).map_err(|error| refuse(format!("{error}")))?;
    if document.format != TRUST_FORMAT {
        return Err(refuse(format!(
            "`{}` is not a trust store format this build reads; it reads `{TRUST_FORMAT}`",
            document.format
        )));
    }
    let mut entries = Vec::with_capacity(document.keys.len());
    for raw in document.keys {
        let key = PublicKey::parse(&raw.key)
            .map_err(|error| refuse(format!("`{}`: {}", raw.publisher, error.message())))?;
        let standing = match raw.trust.as_str() {
            "trusted" => Standing::Trusted,
            "revoked" => Standing::Revoked,
            other => {
                return Err(refuse(format!(
                    "`{other}` is not a standing; a key is `trusted` or `revoked`"
                )));
            }
        };
        entries.push(Entry {
            origin,
            publisher: raw.publisher,
            key,
            standing,
        });
    }
    Ok(entries)
}
