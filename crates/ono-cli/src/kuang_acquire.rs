//! How a package payload reaches this host (K11A §4–§8, §16; ADR-0606, ADR-0607).
//!
//! Three kinds of source, one contract: a catalog release fetched over the network, a local
//! directory, or a payload an operating-system package placed under a system source root.
//! Everything here is *acquisition* — reading metadata, fetching bytes, unpacking into staging,
//! hashing — and none of it confers trust, permission or activation. What a package may do is
//! decided by the same verification, trust and permission steps whatever brought it here, and
//! nothing in this module executes a package (K11A §0.2, §23).

use std::path::{Path, PathBuf};

use ono_core::ErrorCode;
use ono_kuang_protocol::{SYSTEM_ORIGIN_FORMAT, SourceKind, SystemOrigin, content_digest};
use ono_value::{ErrorValue, Value};
use serde::{Deserialize, Serialize};

use crate::kuang_host::{Installed, io_error, read_package};

/// The default Linux system source root (K11A §8.2).
pub const DEFAULT_SYSTEM_ROOT: &str = "/usr/lib/ono-sendai/plugin-sources";

/// Where an installed package came from, remembered with it (K11A §18, §24).
///
/// Enough to answer, later, which kind of source, which location, which catalog lineage and
/// which distribution package supplied the release — and to prefer that lineage when the package
/// is upgraded (K11A §14.2) rather than whichever source happens to be found first (§27).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    /// The kind of source, `catalog-network`, `local-path` or `system-package`.
    pub kind: String,
    /// The source reference: a URL, a `path:` reference, or the system payload's path.
    pub identity: String,
    /// The outer distribution package, when the system source said (K11A §10.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_package: Option<String>,
    /// The package manager that placed it, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_manager: Option<String>,
    /// The directory the payload was read from, for a local or system source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<PathBuf>,
    /// The catalog that resolved it, when one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub catalog: Option<String>,
    /// The content digest that was verified at acquisition, which the installed copy is pinned
    /// to (K11A §16.2, invariant 7).
    pub digest: String,
}

impl Origin {
    /// The kind as the protocol names it.
    #[must_use]
    pub fn source_kind(&self) -> Option<SourceKind> {
        SourceKind::from_id(&self.kind)
    }

    /// Whether the source can still supply this release: the directory is there and holds
    /// the same version, or — for a network artifact — it was a URL, which this host does not
    /// probe (K11A §14.4, §21.4).
    #[must_use]
    pub fn available(&self, version: &str) -> Option<bool> {
        match self.source_kind()? {
            SourceKind::CatalogNetwork => None,
            SourceKind::LocalPath | SourceKind::SystemPackage => {
                let path = self.source_path.as_ref()?;
                Some(
                    read_package(path)
                        .ok()
                        .flatten()
                        .is_some_and(|package| package.manifest.package.version == version),
                )
            }
        }
    }

    /// The origin as a record value, for `inspect plugin` (K11A §21.4).
    #[must_use]
    pub fn value(&self, version: &str) -> Value {
        crate::kuang_host::map([
            ("source_kind", Value::string(&self.kind)),
            ("source_identity", Value::string(&self.identity)),
            (
                "system_package",
                self.system_package
                    .as_deref()
                    .map_or(Value::Null, Value::string),
            ),
            (
                "package_manager",
                self.package_manager
                    .as_deref()
                    .map_or(Value::Null, Value::string),
            ),
            (
                "source_path",
                self.source_path
                    .as_deref()
                    .map_or(Value::Null, |path| Value::Path(std::sync::Arc::from(path))),
            ),
            (
                "catalog",
                self.catalog.as_deref().map_or(Value::Null, Value::string),
            ),
            ("installed_digest", Value::string(&self.digest)),
            (
                "origin_available",
                self.available(version).map_or(Value::Null, Value::Bool),
            ),
        ])
    }
}

/// The system source roots this host searches (K11A §8.2): `ONO_PLUGIN_SYSTEM_SOURCES`, or
/// the standard root, which needs no configuration.
#[must_use]
pub fn system_roots(env: impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    if let Some(roots) = env("ONO_PLUGIN_SYSTEM_SOURCES") {
        return std::env::split_paths(&roots).collect();
    }
    vec![PathBuf::from(DEFAULT_SYSTEM_ROOT)]
}

/// A payload a system package placed: `<root>/<id>/<version>/` (K11A §8.2), read but never
/// installed by being there (§9).
#[derive(Debug, Clone)]
pub struct SystemCandidate {
    /// The payload, read from disk.
    pub package: Installed,
    /// The root it sits under.
    pub root: PathBuf,
    /// What the sidecar said about the distribution package, when one was there.
    pub origin: Option<SystemOrigin>,
}

impl SystemCandidate {
    /// The origin record an install from this candidate carries.
    #[must_use]
    pub fn origin_record(&self, catalog: Option<&str>) -> Origin {
        Origin {
            kind: SourceKind::SystemPackage.id().to_owned(),
            identity: format!("system:{}", self.package.directory.display()),
            system_package: self.origin.as_ref().map(|origin| origin.package.clone()),
            package_manager: self.origin.as_ref().map(|origin| origin.manager.clone()),
            source_path: Some(self.package.directory.clone()),
            catalog: catalog.map(str::to_owned),
            digest: content_digest(&self.package.directory),
        }
    }
}

/// What a scan of the system roots found: the candidates, and every root that was set aside
/// with the reason (K11A §16.1).
#[derive(Debug, Clone, Default)]
pub struct SystemScan {
    /// Every readable payload under an accepted root, in root order then id and version.
    pub candidates: Vec<SystemCandidate>,
    /// Roots that were not searched, and why — a root ordinary users can write to is not a
    /// system source, whatever its path says.
    pub rejected: Vec<(PathBuf, String)>,
    /// Payloads that did not read as packages, for `find plugin`'s failure list.
    pub failures: Vec<ErrorValue>,
}

/// Enumerates the payloads under `roots`. Reads manifests and sidecars; runs nothing
/// (K11A invariant 11).
#[must_use]
pub fn scan_system(roots: &[PathBuf]) -> SystemScan {
    let mut scan = SystemScan::default();
    for root in roots {
        let Ok(metadata) = std::fs::metadata(root) else {
            // An absent root is the ordinary case on a machine with no system packages.
            continue;
        };
        if let Err(reason) = root_policy(root, &metadata) {
            scan.rejected.push((root.clone(), reason));
            continue;
        }
        let mut ids: Vec<PathBuf> = std::fs::read_dir(root)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| path.is_dir())
                    .collect()
            })
            .unwrap_or_default();
        ids.sort();
        for id_dir in ids {
            let mut versions: Vec<PathBuf> = std::fs::read_dir(&id_dir)
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|entry| entry.path())
                        .filter(|path| path.is_dir())
                        .collect()
                })
                .unwrap_or_default();
            versions.sort();
            for version_dir in versions {
                match read_package(&version_dir) {
                    Ok(Some(package)) => {
                        let expected_id = id_dir.file_name().map(|name| name.to_string_lossy());
                        let expected_version =
                            version_dir.file_name().map(|name| name.to_string_lossy());
                        // The layout is the id and the version; a payload that says otherwise
                        // is a payload in the wrong place, and is not offered under either.
                        if expected_id.as_deref() != Some(package.manifest.package.id.as_str())
                            || expected_version.as_deref()
                                != Some(package.manifest.package.version.as_str())
                        {
                            scan.failures.push(
                                ErrorValue::new(
                                    ErrorCode::KuangPackageInvalid,
                                    format!(
                                        "{} holds `{}` {}, and a system payload sits under its \
                                         own id and version (K11A §8.2)",
                                        version_dir.display(),
                                        package.manifest.package.id,
                                        package.manifest.package.version
                                    ),
                                )
                                .with_help("the layout is `<root>/<id>/<version>/`"),
                            );
                            continue;
                        }
                        let origin = read_sidecar(&version_dir);
                        scan.candidates.push(SystemCandidate {
                            package,
                            root: root.clone(),
                            origin,
                        });
                    }
                    Ok(None) => {}
                    Err(error) => scan.failures.push(error),
                }
            }
        }
    }
    scan
}

/// Why a root is not a system source (K11A §16.1): a directory ordinary users can write to.
///
/// The root must not be writable by group or others, and must be owned by root or by the
/// user running the shell — the second is what a scratch root on a developer machine is, and it
/// is no wider than the environment variable that named it.
fn root_policy(root: &Path, metadata: &std::fs::Metadata) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt as _;
    if !metadata.is_dir() {
        return Err(format!("{} is not a directory", root.display()));
    }
    let mode = metadata.mode();
    if mode & 0o022 != 0 {
        return Err(format!(
            "{} is writable by {} (mode {:04o}), so anything under it could have been placed by \
             anyone; a system source is owned by the package manager and read-only to users \
             (K11A §16.1)",
            root.display(),
            if mode & 0o002 != 0 {
                "everyone"
            } else {
                "its group"
            },
            mode & 0o7777
        ));
    }
    let owner = metadata.uid();
    // The shell's own user id, read from procfs rather than through an unsafe call; where
    // that is not readable, only root's roots are searched.
    let me = std::fs::metadata("/proc/self")
        .map(|own| own.uid())
        .unwrap_or(0);
    if owner != 0 && owner != me {
        return Err(format!(
            "{} is owned by uid {owner}, neither root nor this user, so it is not a system \
             source this host trusts to be read-only (K11A §16.1)",
            root.display()
        ));
    }
    Ok(())
}

/// The sidecar beside a payload: `<root>/<id>/<version>.origin.yaml`, data about the outer
/// package. Absent or unreadable is simply unknown; it is never required.
fn read_sidecar(version_dir: &Path) -> Option<SystemOrigin> {
    let name = version_dir.file_name()?.to_string_lossy().into_owned();
    let sidecar = version_dir.with_file_name(format!("{name}.origin.yaml"));
    let text = std::fs::read_to_string(sidecar).ok()?;
    SystemOrigin::parse(&text).ok()
}

/// Writes the sidecar a distribution package installs beside its payload — the form the
/// Kubernetes wrapper and the tests write (K11A §10.1).
#[must_use]
pub fn sidecar_text(package: &str, manager: &str) -> String {
    format!("format: {SYSTEM_ORIGIN_FORMAT}\npackage: {package}\nmanager: {manager}\n")
}

/// Fetches a network artifact into memory (K11A §6.2 steps 1–2). Plain `http://` is refused
/// unless the catalog allowed it (§6.3); the caller has already checked that.
///
/// # Errors
///
/// `plugin.catalog_unavailable` when the artifact cannot be fetched, with the URL and the
/// transport's own words.
pub fn fetch(url: &str, allow_insecure: bool) -> Result<Vec<u8>, ErrorValue> {
    if url.starts_with("http://") && !allow_insecure {
        return Err(ErrorValue::new(
            ErrorCode::PluginTransportRefused,
            format!("`{url}` is a plain `http://` artifact, and this catalog did not allow one"),
        )
        .with_help(
            "a catalog artifact travels over HTTPS; an operator catalog may declare \
             `insecure_http: true` for a source it controls, and the digest and the signature \
             are still checked (K11A §6.3)",
        ));
    }
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(ErrorValue::new(
            ErrorCode::PluginCatalogUnavailable,
            format!("`{url}` is not an artifact this build can fetch"),
        ));
    }
    let unavailable = |detail: String| {
        ErrorValue::new(
            ErrorCode::PluginCatalogUnavailable,
            format!("the artifact `{url}` could not be fetched: {detail}"),
        )
        .with_help(
            "nothing was installed. Check the network and the catalog, or obtain the package \
             another way — a local directory, or the distribution's own package (K11A §2, §20)",
        )
    };
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .max_redirects(4)
        .build()
        .new_agent();
    let mut response = agent
        .get(url)
        .call()
        .map_err(|error| unavailable(error.to_string()))?;
    let bytes = response
        .body_mut()
        .with_config()
        .limit(MAX_ARTIFACT_BYTES)
        .read_to_vec()
        .map_err(|error| unavailable(error.to_string()))?;
    Ok(bytes)
}

/// The largest artifact this host will fetch: a plugin is a program and its documents, not a
/// data set.
const MAX_ARTIFACT_BYTES: u64 = 256 * 1024 * 1024;

/// Unpacks a `.kuang` archive — a plain tar of the package's files (K11A §7, ADR-0607) — into
/// `into`, which must be empty. Every entry is a regular file under a relative path with no
/// `..`, or the archive is refused: a payload chooses no path on this machine.
///
/// # Errors
///
/// `package.invalid` for an archive that is not one, or that names a path outside `into`.
pub fn unpack(bytes: &[u8], into: &Path) -> Result<(), ErrorValue> {
    let invalid = |detail: String| {
        ErrorValue::new(
            ErrorCode::KuangPackageInvalid,
            format!("the artifact is not a `.kuang` archive this host unpacks: {detail}"),
        )
        .with_help(
            "a `.kuang` archive is an uncompressed tar of regular files under relative paths, \
             which is what `kuang-sign pack` writes (K11A §7)",
        )
    };
    std::fs::create_dir_all(into).map_err(|error| io_error(into, &error))?;
    let mut archive = tar::Archive::new(bytes);
    let entries = archive
        .entries()
        .map_err(|error| invalid(error.to_string()))?;
    let mut count = 0_usize;
    for entry in entries {
        let mut entry = entry.map_err(|error| invalid(error.to_string()))?;
        let kind = entry.header().entry_type();
        if !matches!(kind, tar::EntryType::Regular | tar::EntryType::Directory) {
            return Err(invalid(format!(
                "entry `{}` is a {kind:?}, and a payload holds regular files only",
                entry
                    .path()
                    .map_or_else(|_| "?".into(), |p| p.display().to_string())
            )));
        }
        let path = entry
            .path()
            .map_err(|error| invalid(error.to_string()))?
            .into_owned();
        if path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err(invalid(format!(
                "entry `{}` leaves the package directory",
                path.display()
            )));
        }
        let target = into.join(&path);
        if kind == tar::EntryType::Directory {
            std::fs::create_dir_all(&target).map_err(|error| io_error(&target, &error))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|error| io_error(parent, &error))?;
        }
        let mut file = std::fs::File::create(&target).map_err(|error| io_error(&target, &error))?;
        std::io::copy(&mut entry, &mut file).map_err(|error| io_error(&target, &error))?;
        // The mode travels with the entry, which is how the runtime stays executable; nothing
        // else about ownership does.
        if let Ok(mode) = entry.header().mode() {
            use std::os::unix::fs::PermissionsExt as _;
            let _ = std::fs::set_permissions(
                &target,
                std::fs::Permissions::from_mode(mode & 0o755 | 0o600),
            );
        }
        count += 1;
    }
    if count == 0 {
        return Err(invalid("it holds no file".to_owned()));
    }
    Ok(())
}

/// Fetches, unpacks and verifies a network artifact into the package cache (K11A §6.2 steps
/// 2–4, §6.4): the bytes land in a staging directory, the unpacked payload is hashed, and only
/// a payload whose digest is the one the catalog vouched for moves to
/// `<cache>/<id>/<version>`. Nothing under staging is ever executed (§16.3).
///
/// # Errors
///
/// The fetch's refusal, `package.invalid` for a malformed archive, or
/// `package.integrity_failed` for a digest mismatch — after which nothing is left behind.
pub fn acquire_network(
    cache: &Path,
    id: &str,
    version: &str,
    url: &str,
    expected_digest: &str,
    allow_insecure: bool,
) -> Result<Installed, ErrorValue> {
    let bytes = fetch(url, allow_insecure)?;
    let nonce = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default()
    );
    let staging = cache
        .join(".staging")
        .join(format!("{id}-{version}-{nonce}"));
    let outcome = (|| -> Result<Installed, ErrorValue> {
        unpack(&bytes, &staging)?;
        let actual = content_digest(&staging);
        if actual != expected_digest {
            return Err(ErrorValue::new(
                ErrorCode::KuangPackageIntegrityFailed,
                format!(
                    "`{id}` {version}: the catalog vouches for {expected_digest} and the artifact \
                     `{url}` unpacks to {actual}"
                ),
            )
            .with_help(
                "a digest mismatch is indistinguishable from a substituted artifact; nothing was \
                 installed (K11A §6.2, invariant 8)",
            ));
        }
        let package = read_package(&staging)?.ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::KuangPackageInvalid,
                format!("the artifact `{url}` holds no `manifest.yaml`"),
            )
        })?;
        if package.manifest.package.id != id || package.manifest.package.version != version {
            return Err(ErrorValue::new(
                ErrorCode::KuangPackageIntegrityFailed,
                format!(
                    "the artifact `{url}` identifies as `{}` {}, and the catalog release is \
                     `{id}` {version}",
                    package.manifest.package.id, package.manifest.package.version
                ),
            ));
        }
        let destination = cache.join(id).join(version);
        if destination.exists() {
            std::fs::remove_dir_all(&destination)
                .map_err(|error| io_error(&destination, &error))?;
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent).map_err(|error| io_error(parent, &error))?;
        }
        std::fs::rename(&staging, &destination).map_err(|error| io_error(&destination, &error))?;
        read_package(&destination)?.ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::KuangPackageInvalid,
                format!("{} holds no `manifest.yaml`", destination.display()),
            )
        })
    })();
    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    outcome
}

/// What a person may name with `--source` (K11A §10.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preference {
    /// The default: the installed package's own lineage, then a system package, then a local
    /// copy, then the network (K11A §10.3, §14.2, §27).
    Default,
    /// Only a system-provided payload.
    System,
    /// Only the catalog's artifact — fetched, or the cached copy of it.
    Catalog,
    /// Only a local package directory or source.
    Local,
    /// The lineage an installed package was acquired through, which an upgrade keeps until a
    /// person names another source (K11A §14.2, §27).
    Lineage(SourceKind),
}

impl Preference {
    /// Reads `--source`.
    ///
    /// # Errors
    ///
    /// `type.mismatch` for a word that is not one of the three.
    pub fn parse(word: Option<&str>) -> Result<Self, ErrorValue> {
        match word {
            None => Ok(Preference::Default),
            Some("system") => Ok(Preference::System),
            Some("catalog") => Ok(Preference::Catalog),
            Some("local") => Ok(Preference::Local),
            Some(other) => Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`{other}` is not a source kind"),
            )
            .with_help("`--source system`, `--source catalog` or `--source local` (K11A §10.4)")),
        }
    }

    /// Whether a candidate of `kind` may be chosen under this preference.
    #[must_use]
    pub fn admits(self, kind: SourceKind) -> bool {
        match self {
            Preference::Default => true,
            Preference::System => kind == SourceKind::SystemPackage,
            Preference::Catalog => kind == SourceKind::CatalogNetwork,
            Preference::Local => kind == SourceKind::LocalPath,
            Preference::Lineage(lineage) => kind == lineage,
        }
    }

    /// The word, for messages.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Preference::Default => "default",
            Preference::System => "system",
            Preference::Catalog => "catalog",
            Preference::Local => "local",
            Preference::Lineage(SourceKind::SystemPackage) => "system",
            Preference::Lineage(SourceKind::CatalogNetwork) => "catalog",
            Preference::Lineage(SourceKind::LocalPath) => "local",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_refuse_an_archive_that_leaves_the_package_directory() {
        // The `tar` crate refuses to *write* such a path; an attacker's archive is written by
        // something else, so the header is assembled by hand.
        let data = b"format: kuang-package/1\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.as_mut_bytes()[..14].copy_from_slice(b"../escape.yaml");
        header.set_cksum();
        let mut bytes = header.as_bytes().to_vec();
        bytes.extend_from_slice(data);
        bytes.resize(bytes.len().div_ceil(512) * 512, 0);
        bytes.extend_from_slice(&[0_u8; 1024]);
        let scratch = std::env::temp_dir().join(format!("ono-unpack-{}", std::process::id()));
        let refused = unpack(&bytes, &scratch);
        let _ = std::fs::remove_dir_all(&scratch);
        assert!(
            refused.is_err_and(|error| error.code() == ErrorCode::KuangPackageInvalid),
            "an entry with `..` is refused before it is written"
        );
    }

    #[test]
    fn should_search_the_standard_root_without_any_configuration() {
        // K11A §8.2, §25.3 (17): the ordinary user configures nothing; the Linux root is
        // searched by default, and the environment names another list where a test or an
        // operator needs one.
        assert_eq!(
            system_roots(|_| None),
            vec![PathBuf::from("/usr/lib/ono-sendai/plugin-sources")]
        );
        assert_eq!(
            system_roots(|name| (name == "ONO_PLUGIN_SYSTEM_SOURCES").then(|| "/a:/b".to_owned())),
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
    }

    #[test]
    fn should_read_source_words_and_refuse_others() {
        assert_eq!(
            Preference::parse(None).expect("default"),
            Preference::Default
        );
        assert_eq!(
            Preference::parse(Some("system")).expect("system"),
            Preference::System
        );
        assert!(Preference::parse(Some("apt")).is_err());
        assert!(Preference::System.admits(SourceKind::SystemPackage));
        assert!(!Preference::System.admits(SourceKind::CatalogNetwork));
    }
}
