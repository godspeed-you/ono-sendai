//! Where a name resolves from, and where a release is looked for (K11P §10, §11, §30;
//! ADR-0601): the bootstrap catalog embedded in the binary, the operator's catalogs beside it,
//! the local package sources and the package cache. Nothing here executes package code.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_kuang_protocol::{
    Catalog, CatalogEntry, CatalogRelease, CatalogVerification, HOST_API, PluginRef, looks_like_id,
};
use ono_value::{ErrorValue, RecordValue, Schema, Value};

use crate::kuang_acquire::{Origin, Preference, SystemScan, acquire_network, scan_system};
use crate::kuang_host::{Host, Installed, read_package, schema};
use ono_kuang_protocol::{Artifact, SourceKind, content_digest};

/// The bootstrap catalog, embedded from `docs/contracts/kuang/bootstrap-catalog.yaml` so that a
/// clean installation resolves the reference packages without a configured source (K11P §11.4).
const BOOTSTRAP: &str = include_str!("../../../docs/contracts/kuang/bootstrap-catalog.yaml");

/// The catalogs a session resolves names against, and the documents that did not read.
#[derive(Debug, Default, Clone)]
pub struct Catalogs {
    /// The catalogs, the built-in one first.
    pub catalogs: Vec<Catalog>,
    /// Operator documents that are not catalogs, reported rather than skipped.
    pub problems: Vec<ErrorValue>,
}

impl Catalogs {
    /// The built-in catalog, the operator's under `<config>/kuang/catalogs/`, and the machine's
    /// under `<system>/kuang/catalogs/` (ADR-0601 §2).
    #[must_use]
    pub fn read(config_dir: Option<&Path>, system_dir: &Path) -> Self {
        let mut read = Self::default();
        match Catalog::parse(BOOTSTRAP, CatalogVerification::BuiltIn) {
            Ok(catalog) => read.catalogs.push(catalog),
            Err(error) => read.problems.push(
                crate::plugins::error_value(&error)
                    .with_metadata("catalog", Value::string("built-in")),
            ),
        }
        for directory in [Some(system_dir), config_dir].into_iter().flatten() {
            let directory = directory.join("kuang").join("catalogs");
            let Ok(entries) = std::fs::read_dir(&directory) else {
                continue;
            };
            let mut paths: Vec<PathBuf> = entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.extension()
                        .is_some_and(|extension| extension == "yaml")
                })
                .collect();
            paths.sort();
            for path in paths {
                let text = match std::fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(error) => {
                        read.problems
                            .push(crate::kuang_host::io_error(&path, &error));
                        continue;
                    }
                };
                match Catalog::parse(&text, CatalogVerification::Operator) {
                    Ok(catalog) => {
                        if read
                            .catalogs
                            .iter()
                            .any(|existing| existing.name == catalog.name)
                        {
                            read.problems.push(
                                ErrorValue::new(
                                    ErrorCode::ProviderSchemaViolation,
                                    format!(
                                        "{} names the catalog `{}`, which another catalog already \
                                         claims",
                                        path.display(),
                                        catalog.name
                                    ),
                                )
                                .with_help("a catalog name is what `<catalog>/<name>` selects by; rename one"),
                            );
                            continue;
                        }
                        read.catalogs.push(catalog);
                    }
                    Err(error) => read.problems.push(
                        crate::plugins::error_value(&error)
                            .with_metadata("catalog", Value::Path(Arc::from(path.as_path()))),
                    ),
                }
            }
        }
        read
    }

    /// The catalog with this name.
    #[must_use]
    pub fn named(&self, name: &str) -> Option<&Catalog> {
        self.catalogs.iter().find(|catalog| catalog.name == name)
    }
}

/// The local package sources: `ONO_PLUGIN_SOURCES`, or the user's and the machine's package
/// directories (ADR-0601 §3). Each holds unpacked packages by id.
#[must_use]
pub fn local_sources(env: impl Fn(&str) -> Option<String>, home: Option<&Path>) -> Vec<PathBuf> {
    if let Some(sources) = env("ONO_PLUGIN_SOURCES") {
        return std::env::split_paths(&sources).collect();
    }
    let mut sources = Vec::new();
    if let Some(data) = env("XDG_DATA_HOME") {
        sources.push(
            PathBuf::from(data)
                .join(ono_core::SHORT_NAME)
                .join("packages"),
        );
    } else if let Some(home) = home {
        sources.push(
            home.join(".local")
                .join("share")
                .join(ono_core::SHORT_NAME)
                .join("packages"),
        );
    }
    sources.push(
        PathBuf::from("/usr/share")
            .join(ono_core::SHORT_NAME)
            .join("packages"),
    );
    sources
}

/// The package cache a resolved release is looked for in first (K11P §30.3).
#[must_use]
pub fn cache_dir(env: impl Fn(&str) -> Option<String>, home: Option<&Path>) -> Option<PathBuf> {
    if let Some(cache) = env("XDG_CACHE_HOME") {
        return Some(
            PathBuf::from(cache)
                .join(ono_core::SHORT_NAME)
                .join("kuang")
                .join("packages"),
        );
    }
    home.map(|home| {
        home.join(".cache")
            .join(ono_core::SHORT_NAME)
            .join("kuang")
            .join("packages")
    })
}

/// A package a reference resolved to, before anything is verified (ADR-0601, ADR-0606).
#[derive(Debug, Clone)]
pub struct Located {
    /// The package, read from disk.
    pub package: Installed,
    /// The source reference the install records.
    pub source: String,
    /// The catalog that answered, and how it is known.
    pub catalog: Option<(String, CatalogVerification)>,
    /// The catalog entry, when a catalog answered.
    pub entry: Option<CatalogEntry>,
    /// The release chosen, when a catalog answered.
    pub release: Option<CatalogRelease>,
    /// Where the payload came from, remembered with the install (K11A §18).
    pub origin: Origin,
}

/// One candidate of an ambiguous name (K11P §10.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The canonical id.
    pub id: String,
    /// The `<catalog>/<name>` selector that names exactly it, or the installed id.
    pub selector: String,
    /// One line about it.
    pub description: String,
}

/// One place a release was found, before one is chosen (K11A §10.2, §19).
#[derive(Debug, Clone)]
struct Found {
    kind: SourceKind,
    /// The payload, when it is on this machine already.
    package: Option<Installed>,
    /// The source reference.
    source: String,
    /// The content digest: computed for a local payload, vouched for by the catalog for a
    /// network artifact.
    digest: Option<String>,
    /// What the origin record will say.
    origin: Origin,
}

impl Found {
    /// The order the default preference chooses in (K11A §10.3): a system package, then a
    /// local copy, then the network.
    const fn rank(&self) -> u8 {
        match self.kind {
            SourceKind::SystemPackage => 0,
            SourceKind::LocalPath => 1,
            SourceKind::CatalogNetwork => 2,
        }
    }

    fn label(&self) -> String {
        format!("{} at {}", self.kind.human(), self.source)
    }
}

/// The origin of a payload read from a local directory.
fn local_origin(directory: &Path, identity: &str, catalog: Option<&str>) -> Origin {
    Origin {
        kind: SourceKind::LocalPath.id().to_owned(),
        identity: identity.to_owned(),
        system_package: None,
        package_manager: None,
        source_path: Some(directory.to_path_buf()),
        catalog: catalog.map(str::to_owned),
        digest: content_digest(directory),
    }
}

impl Host {
    /// The catalogs this session resolves against.
    #[must_use]
    pub fn catalogs(&self) -> &Catalogs {
        &self.catalogs
    }

    /// What the system source roots hold right now (K11A §8, §10.1). Read on demand; nothing
    /// is executed.
    #[must_use]
    pub fn system_scan(&self) -> SystemScan {
        scan_system(&self.system_roots)
    }

    /// Resolves a reference for installation, in K11P §10.1's order (ADR-0601 §1), choosing
    /// among the sources that offer the release under `preference` (K11A §10, ADR-0606).
    ///
    /// # Errors
    ///
    /// `plugin.not_found`, `plugin.reference_ambiguous` (with `candidates` in the metadata),
    /// `plugin.release_not_compatible`, `plugin.catalog_unavailable`, `plugin.source_conflict`,
    /// `plugin.transport_refused`, or a source scheme this build does not resolve.
    pub fn locate_for_install(
        &self,
        reference: &PluginRef,
        preference: Preference,
    ) -> Result<Located, ErrorValue> {
        match reference {
            PluginRef::Path(path) => {
                let directory = expand_home(path, self.home_dir().as_deref());
                match read_package(&directory) {
                    Ok(Some(package)) => {
                        let source = format!("path:{}", directory.display());
                        Ok(Located {
                            origin: local_origin(&directory, &source, None),
                            package,
                            source,
                            catalog: None,
                            entry: None,
                            release: None,
                        })
                    }
                    Ok(None) => Err(ErrorValue::new(
                        ErrorCode::PluginNotFound,
                        format!("{} holds no `manifest.yaml`", directory.display()),
                    )
                    .with_help("an explicit path names an unpacked package directory (K11P §4.1)")),
                    Err(error) => Err(error),
                }
            }
            PluginRef::Source { scheme, reference } => Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                format!(
                    "the `{scheme}:` source scheme is not available in this build ({reference})"
                ),
            )
            .with_help(
                "a short name resolves through the catalogs and the system sources, and \
                 `path:<directory>` or `./dir` names a local package (K11P §4.1, ADR-0601)",
            )),
            PluginRef::Catalog { catalog, name } => {
                let Some(found) = self.catalogs.named(catalog) else {
                    return Err(ErrorValue::new(
                        ErrorCode::PluginNotFound,
                        format!("no catalog is named `{catalog}`"),
                    )
                    .with_help(format!(
                        "the catalogs are {}",
                        self.catalogs
                            .catalogs
                            .iter()
                            .map(|catalog| format!("`{}`", catalog.name))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )));
                };
                let entries: Vec<&CatalogEntry> = found.named(name).collect();
                match entries.as_slice() {
                    [entry] => self.locate_release(found, entry, preference),
                    [] => Err(not_found(&format!("{catalog}/{name}"))),
                    several => Err(ambiguous(
                        name,
                        several
                            .iter()
                            .map(|entry| Candidate {
                                id: entry.id.clone(),
                                selector: entry.id.clone(),
                                description: entry.description.clone(),
                            })
                            .collect(),
                    )),
                }
            }
            PluginRef::Word(word) => self.locate_word(word, preference),
        }
    }

    /// A word: an exact id, or a short name across the catalogs and the system sources.
    fn locate_word(&self, word: &str, preference: Preference) -> Result<Located, ErrorValue> {
        let scan = self.system_scan();
        // An exact canonical id: offered by a catalog, provided by the system, or installed
        // (K11P §10.1 step 3; K11A §20).
        if looks_like_id(word) {
            let offered: Vec<(&Catalog, &CatalogEntry)> = self
                .catalogs
                .catalogs
                .iter()
                .filter_map(|catalog| catalog.entry(word).map(|entry| (catalog, entry)))
                .collect();
            if let Some((catalog, entry)) = offered.first() {
                return self.locate_release(catalog, entry, preference);
            }
            if let Some(located) = self.locate_system_only(&scan, word, preference)? {
                return Ok(located);
            }
            if let Some(package) = self.installed_package(word) {
                let source = format!("path:{}", package.directory.display());
                return Ok(Located {
                    origin: local_origin(&package.directory, &source, None),
                    package,
                    source,
                    catalog: None,
                    entry: None,
                    release: None,
                });
            }
        }
        // A short name across the catalogs and the system sources (step 5). The same id in
        // two places is one package; different ids under one name are an ambiguity nothing
        // resolves by order — and a locally available payload does not either (K11A §10.3,
        // §19.3).
        let mut candidates: Vec<(Option<(&Catalog, &CatalogEntry)>, Candidate)> = Vec::new();
        for catalog in &self.catalogs.catalogs {
            for entry in catalog.named(word) {
                if candidates
                    .iter()
                    .all(|(_, existing)| existing.id != entry.id)
                {
                    candidates.push((
                        Some((catalog, entry)),
                        Candidate {
                            id: entry.id.clone(),
                            selector: format!("{}/{}", catalog.name, entry.name),
                            description: entry.description.clone(),
                        },
                    ));
                }
            }
        }
        for candidate in &scan.candidates {
            let info = &candidate.package.manifest.package;
            if info.name == word
                && candidates
                    .iter()
                    .all(|(_, existing)| existing.id != info.id)
            {
                candidates.push((
                    None,
                    Candidate {
                        id: info.id.clone(),
                        selector: info.id.clone(),
                        description: format!("{} (system package)", info.description),
                    },
                ));
            }
        }
        match candidates.as_slice() {
            [(Some((catalog, entry)), _)] => self.locate_release(catalog, entry, preference),
            [(None, candidate)] => self
                .locate_system_only(&scan, &candidate.id, preference)?
                .ok_or_else(|| not_found(word)),
            [] => {
                // An installed package by its name, so `install plugin <name>` of something
                // already installed says so rather than "not found".
                if let Ok(package) = self.resolve_installed(word) {
                    let source = format!("path:{}", package.directory.display());
                    return Ok(Located {
                        origin: local_origin(&package.directory, &source, None),
                        package,
                        source,
                        catalog: None,
                        entry: None,
                        release: None,
                    });
                }
                Err(not_found(word))
            }
            several => Err(ambiguous(
                word,
                several
                    .iter()
                    .map(|(_, candidate)| candidate.clone())
                    .collect(),
            )),
        }
    }

    /// A system-provided payload no catalog lists (K11A §20, §25.7): the newest version this
    /// host can run, under a preference that admits a system package.
    fn locate_system_only(
        &self,
        scan: &SystemScan,
        id: &str,
        preference: Preference,
    ) -> Result<Option<Located>, ErrorValue> {
        let platform = ono_kuang_supervisor::host_platform();
        let mut offered: Vec<&crate::kuang_acquire::SystemCandidate> = scan
            .candidates
            .iter()
            .filter(|candidate| candidate.package.manifest.package.id == id)
            .collect();
        if offered.is_empty() {
            return Ok(None);
        }
        if !preference.admits(SourceKind::SystemPackage) {
            return Err(ErrorValue::new(
                ErrorCode::PluginCatalogUnavailable,
                format!(
                    "`{id}` is offered only as a system package, and `--source {}` admits none",
                    preference.word()
                ),
            )
            .with_help("`--source system` selects it (K11A §10.4)"));
        }
        offered.sort_by(|a, b| {
            ono_kuang_protocol::compare_versions(
                &b.package.manifest.package.version,
                &a.package.manifest.package.version,
            )
        });
        let mut reasons = Vec::new();
        for candidate in offered {
            let compatibility = &candidate.package.manifest.compatibility;
            if !compatibility.kuang_api.contains(HOST_API) {
                reasons.push(format!(
                    "{}: kuang_api `{}` excludes this host's `{HOST_API}`",
                    candidate.package.manifest.package.version, compatibility.kuang_api
                ));
                continue;
            }
            if !compatibility
                .platforms
                .iter()
                .any(|supported| supported == &platform)
            {
                reasons.push(format!(
                    "{}: platforms {:?} exclude `{platform}`",
                    candidate.package.manifest.package.version, compatibility.platforms
                ));
                continue;
            }
            let origin = candidate.origin_record(None);
            return Ok(Some(Located {
                package: candidate.package.clone(),
                source: origin.identity.clone(),
                catalog: None,
                entry: None,
                release: None,
                origin,
            }));
        }
        Err(ErrorValue::new(
            ErrorCode::PluginReleaseNotCompatible,
            format!(
                "no system-provided release of `{id}` runs on this host ({} {platform}): {}",
                HOST_API.protocol_id(),
                reasons.join("; ")
            ),
        )
        .with_help("a package version is never installed to be found incompatible at load"))
    }

    /// The newest release this host can run, from whichever source offers it under
    /// `preference` (ADR-0601 §3, ADR-0606 §2).
    fn locate_release(
        &self,
        catalog: &Catalog,
        entry: &CatalogEntry,
        preference: Preference,
    ) -> Result<Located, ErrorValue> {
        let platform = ono_kuang_supervisor::host_platform();
        // A system package may carry a release the catalog does not list yet — the
        // distribution's own pin, or an upgrade `apt` brought — and under a preference that
        // admits it, a newer system release is the candidate (K11A §10.3, §14.2).
        let scan = self.system_scan();
        if preference.admits(SourceKind::SystemPackage)
            && let Some(system) = self.locate_system_only(&scan, &entry.id, preference)?
        {
            let newer = match entry.release_for(HOST_API, &platform) {
                Ok(release) => {
                    ono_kuang_protocol::compare_versions(
                        &system.package.manifest.package.version,
                        &release.version,
                    ) == std::cmp::Ordering::Greater
                }
                Err(_) => true,
            };
            if newer {
                let mut located = system;
                located.catalog = Some((catalog.name.clone(), catalog.verification));
                located.entry = Some(entry.clone());
                located.origin.catalog = Some(catalog.name.clone());
                return Ok(located);
            }
        }
        let release = entry.release_for(HOST_API, &platform).map_err(|reasons| {
            ErrorValue::new(
                ErrorCode::PluginReleaseNotCompatible,
                format!(
                    "no release of `{}` in the `{}` catalog runs on this host ({} {platform}): {}",
                    entry.id,
                    catalog.name,
                    HOST_API.protocol_id(),
                    if reasons.is_empty() {
                        "it offers none".to_owned()
                    } else {
                        reasons.join("; ")
                    }
                ),
            )
            .with_help("a package version is never installed to be found incompatible at load")
        })?;

        // Every place the release is (K11A §10.2): system payloads, local copies, the network.
        let mut found: Vec<Found> = Vec::new();
        for candidate in scan.candidates.iter().filter(|candidate| {
            candidate.package.manifest.package.id == entry.id
                && candidate.package.manifest.package.version == release.version
        }) {
            let origin = candidate.origin_record(Some(&catalog.name));
            found.push(Found {
                kind: SourceKind::SystemPackage,
                package: Some(candidate.package.clone()),
                source: origin.identity.clone(),
                digest: Some(origin.digest.clone()),
                origin,
            });
        }
        if let Some((package, source)) = self.local_release(entry, release) {
            let origin = local_origin(&package.directory, &source, Some(&catalog.name));
            found.push(Found {
                kind: SourceKind::LocalPath,
                package: Some(package),
                source,
                digest: Some(origin.digest.clone()),
                origin,
            });
        }
        if let Artifact::Network(url) = release.artifact_kind() {
            found.push(Found {
                kind: SourceKind::CatalogNetwork,
                package: None,
                source: url.clone(),
                digest: release.digest.clone(),
                origin: Origin {
                    kind: SourceKind::CatalogNetwork.id().to_owned(),
                    identity: url,
                    system_package: None,
                    package_manager: None,
                    source_path: self
                        .cache_dir
                        .as_ref()
                        .map(|cache| cache.join(&entry.id).join(&release.version)),
                    catalog: Some(catalog.name.clone()),
                    digest: release.digest.clone().unwrap_or_default(),
                },
            });
        }

        // The same id and version with two different contents is a supply-chain conflict, and
        // nothing resolves it by order; a person selects a source (K11A §19.2, invariant 8).
        // Under the default preference the catalog's vouched digest is one of the voices.
        let mut voices: Vec<(String, String)> = found
            .iter()
            .filter(|candidate| preference.admits(candidate.kind))
            .filter_map(|candidate| {
                candidate
                    .digest
                    .clone()
                    .map(|digest| (candidate.label(), digest))
            })
            .collect();
        if preference == Preference::Default
            && let Some(digest) = &release.digest
            && !found
                .iter()
                .any(|candidate| candidate.kind == SourceKind::CatalogNetwork)
        {
            voices.push((
                format!("the `{}` catalog's digest", catalog.name),
                digest.clone(),
            ));
        }
        let distinct: std::collections::BTreeSet<&str> =
            voices.iter().map(|(_, digest)| digest.as_str()).collect();
        if distinct.len() > 1 {
            return Err(source_conflict(&entry.id, &release.version, &voices));
        }

        let mut admitted: Vec<Found> = found
            .into_iter()
            .filter(|candidate| preference.admits(candidate.kind))
            .collect();
        admitted.sort_by_key(Found::rank);
        let Some(chosen) = admitted.into_iter().next() else {
            return Err(self.unavailable(catalog, entry, release, preference));
        };
        let (package, source, origin) = match chosen.kind {
            SourceKind::SystemPackage | SourceKind::LocalPath => (
                chosen.package.ok_or_else(|| {
                    ErrorValue::new(
                        ErrorCode::PluginNotFound,
                        format!("{} holds no package", chosen.source),
                    )
                })?,
                chosen.source,
                chosen.origin,
            ),
            SourceKind::CatalogNetwork => {
                let Some(cache) = &self.cache_dir else {
                    return Err(ErrorValue::new(
                        ErrorCode::PluginCatalogUnavailable,
                        format!(
                            "`{}` {} would be fetched from `{}`, and this session has no package \
                             cache to stage it in",
                            entry.id, release.version, chosen.source
                        ),
                    )
                    .with_help("set `XDG_CACHE_HOME` or `HOME` (K11A §6.2)"));
                };
                let Some(digest) = &release.digest else {
                    return Err(ErrorValue::new(
                        ErrorCode::PluginCatalogUnavailable,
                        format!(
                            "`{}` {} names a network artifact and no digest",
                            entry.id, release.version
                        ),
                    ));
                };
                let package = acquire_network(
                    cache,
                    &entry.id,
                    &release.version,
                    &chosen.source,
                    digest,
                    catalog.insecure_http,
                )?;
                (package, chosen.source, chosen.origin)
            }
        };
        if package.manifest.package.id != entry.id {
            return Err(ErrorValue::new(
                ErrorCode::KuangPackageIntegrityFailed,
                format!(
                    "the package at {} identifies as `{}`, and the catalog entry is `{}`",
                    source, package.manifest.package.id, entry.id
                ),
            ));
        }
        Ok(Located {
            package,
            source,
            catalog: Some((catalog.name.clone(), catalog.verification)),
            entry: Some(entry.clone()),
            release: Some(release.clone()),
            origin,
        })
    }

    /// `plugin.catalog_unavailable`: the release is known and no admitted source has it.
    fn unavailable(
        &self,
        catalog: &Catalog,
        entry: &CatalogEntry,
        release: &CatalogRelease,
        preference: Preference,
    ) -> ErrorValue {
        let mut looked = Vec::new();
        if let Some(cache) = &self.cache_dir {
            looked.push(Value::Path(Arc::from(cache.as_path())));
        }
        looked.extend(
            self.sources
                .iter()
                .chain(self.system_roots.iter())
                .map(|source| Value::Path(Arc::from(source.as_path()))),
        );
        let message = match preference {
            Preference::Default => format!(
                "`{}` {} is in the `{}` catalog and its artifact `{}` is not available: it is \
                 not a network artifact, and no system package or local copy holds the release",
                entry.id, release.version, catalog.name, release.artifact
            ),
            other => format!(
                "`{}` {} is in the `{}` catalog and `--source {}` admits no source that has it",
                entry.id,
                release.version,
                catalog.name,
                other.word()
            ),
        };
        ErrorValue::new(ErrorCode::PluginCatalogUnavailable, message)
            .with_help(format!(
                "install the distribution's package, place the unpacked package under {}, or \
                 install it from where it is with `install plugin path:<directory>` (K11A §2, \
                 ADR-0606)",
                self.sources.first().map_or_else(
                    || "a local package source".to_owned(),
                    |source| source.display().to_string()
                )
            ))
            .with_metadata("artifact", Value::string(&release.artifact))
            .with_metadata("looked_in", Value::list(looked))
    }

    /// The release's unpacked package on this machine: the cache, the local sources, or the
    /// artifact itself when it is a local path.
    fn local_release(
        &self,
        entry: &CatalogEntry,
        release: &CatalogRelease,
    ) -> Option<(Installed, String)> {
        let mut places: Vec<(PathBuf, String)> = Vec::new();
        if let Some(cache) = &self.cache_dir {
            let directory = cache.join(&entry.id).join(&release.version);
            places.push((directory.clone(), format!("path:{}", directory.display())));
        }
        for source in &self.sources {
            let directory = source.join(&entry.id);
            places.push((directory.clone(), format!("path:{}", directory.display())));
        }
        if let Some(path) = release.artifact.strip_prefix("path:") {
            let directory = expand_home(Path::new(path), self.home_dir().as_deref());
            places.push((directory, release.artifact.clone()));
        }
        for (directory, source) in places {
            if let Ok(Some(package)) = read_package(&directory)
                && package.manifest.package.id == entry.id
                && package.manifest.package.version == release.version
            {
                return Some((package, source));
            }
        }
        None
    }

    /// The operator's home, for `~` in a written path.
    fn home_dir(&self) -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

/// `plugin.source_conflict` (K11A §19.2): every voice and its digest, so the choice is informed.
fn source_conflict(id: &str, version: &str, voices: &[(String, String)]) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::PluginSourceConflict,
        format!(
            "`{id}` {version} is offered with different contents: {}",
            voices
                .iter()
                .map(|(place, digest)| format!("{place} hashes to {digest}"))
                .collect::<Vec<_>>()
                .join("; ")
        ),
    )
    .with_help(
        "the same version from two sources is one package or a substituted one, and nothing \
         decides which by order. Choose deliberately with `--source system`, `--source local` \
         or `--source catalog`, or remove the copy that is wrong (K11A §19.2)",
    )
    .with_metadata("id", Value::string(id))
    .with_metadata("version", Value::string(version))
    .with_metadata(
        "sources",
        Value::list(voices.iter().map(|(place, digest)| {
            crate::kuang_host::map([
                ("source", Value::string(place)),
                ("digest", Value::string(digest)),
            ])
        })),
    )
}

fn expand_home(path: &Path, home: Option<&Path>) -> PathBuf {
    let text = path.to_string_lossy();
    match (text.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ if text == "~" => home.map_or_else(|| path.to_path_buf(), Path::to_path_buf),
        _ => path.to_path_buf(),
    }
}

fn not_found(reference: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::PluginNotFound,
        format!("no installed package and no catalog entry answers to `{reference}`"),
    )
    .with_help(
        "`find plugin <word>` searches the installed set and every catalog; a short name is the \
         package's `name`, a canonical id its reverse-DNS `id`, and `./dir` or `path:<dir>` an \
         explicit local package (K11P §4.1)",
    )
}

fn ambiguous(reference: &str, candidates: Vec<Candidate>) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::PluginReferenceAmbiguous,
        format!(
            "`{reference}` names {} packages: {}",
            candidates.len(),
            candidates
                .iter()
                .map(|candidate| format!("{} ({})", candidate.id, candidate.selector))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    )
    .with_help(
        "name one of them by its canonical id or as `<catalog>/<name>`; nothing is chosen by \
         catalog order or publisher (K11P §10.3)",
    )
    .with_metadata(
        "candidates",
        Value::list(candidates.iter().map(|candidate| {
            crate::kuang_host::map([
                ("id", Value::string(&candidate.id)),
                ("selector", Value::string(&candidate.selector)),
                ("description", Value::string(&candidate.description)),
            ])
        })),
    )
}

/// The candidates an ambiguous reference carried, read back from its metadata for a picker.
#[must_use]
pub fn candidates_of(error: &ErrorValue) -> Vec<Candidate> {
    let Some(Value::List(items)) = error.metadata().get("candidates") else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let map = item.as_map().ok()?;
            let field = |name: &str| {
                map.get(name)
                    .and_then(|value| value.as_str().ok())
                    .map(str::to_owned)
            };
            Some(Candidate {
                id: field("id")?,
                selector: field("selector")?,
                description: field("description").unwrap_or_default(),
            })
        })
        .collect()
}

/// `find plugin <term>` over the catalogs and the system sources (K11P §11.6, K11A §10.1):
/// every entry whose id or name contains the term, as `ono.plugin-package/1` records — from the
/// payload where a system package or a local copy holds the release, and from the catalog's own
/// metadata where it is not here — and every system payload no catalog lists.
///
/// # Errors
///
/// `provider.schema_violation` when a record does not fit its contract.
pub fn search(
    host: &Host,
    term: &str,
    installed: &[Installed],
    kinds: Option<SourceKind>,
) -> Result<(Vec<RecordValue>, Vec<ErrorValue>), ErrorValue> {
    let schema = schema("ono.plugin-package")?;
    let needle = term.to_lowercase();
    let mut records = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let scan = host.system_scan();
    let platform = ono_kuang_supervisor::host_platform();
    let already = |id: &str, version: &str| {
        installed
            .iter()
            .any(|held| held.manifest.package.id == id && held.manifest.package.version == version)
    };
    if kinds.is_none_or(|kind| kind != SourceKind::SystemPackage) {
        for catalog in &host.catalogs.catalogs {
            for entry in &catalog.entries {
                if !entry.id.to_lowercase().contains(&needle)
                    && !entry.name.to_lowercase().contains(&needle)
                {
                    continue;
                }
                if !seen.insert(entry.id.clone()) {
                    continue;
                }
                let release = entry
                    .release_for(HOST_API, &platform)
                    .ok()
                    .or_else(|| entry.releases.first());
                let catalog_words = (catalog.name.as_str(), catalog.verification.id());
                // A system payload of the release first, then a local copy, then the catalog's
                // own words (K11A §10.3).
                let system = release.and_then(|release| {
                    scan.candidates.iter().find(|candidate| {
                        candidate.package.manifest.package.id == entry.id
                            && candidate.package.manifest.package.version == release.version
                    })
                });
                if let Some(candidate) = system
                    && kinds.is_none_or(|kind| kind == SourceKind::SystemPackage)
                {
                    let origin = candidate.origin_record(Some(&catalog.name));
                    records.push(crate::kuang_host::package_record(
                        &schema,
                        &candidate.package,
                        &origin.identity,
                        already(&entry.id, &candidate.package.manifest.package.version),
                        host.trust(),
                        Some(catalog_words),
                        SourceKind::SystemPackage,
                        origin.system_package.as_deref(),
                    )?);
                    continue;
                }
                match release.and_then(|release| {
                    host.local_release(entry, release)
                        .map(|found| (release, found))
                }) {
                    Some((release, (package, source)))
                        if kinds.is_none_or(|kind| kind == SourceKind::LocalPath) =>
                    {
                        records.push(crate::kuang_host::package_record(
                            &schema,
                            &package,
                            &source,
                            already(&entry.id, &release.version),
                            host.trust(),
                            Some(catalog_words),
                            SourceKind::LocalPath,
                            None,
                        )?);
                    }
                    _ if kinds.is_none_or(|kind| kind == SourceKind::CatalogNetwork) => {
                        records.push(catalog_record(
                            &schema,
                            entry,
                            release,
                            catalog_words,
                            release.is_some_and(|release| already(&entry.id, &release.version)),
                        )?);
                    }
                    _ => {}
                }
            }
        }
    }
    if kinds.is_none_or(|kind| kind == SourceKind::SystemPackage) {
        for candidate in &scan.candidates {
            let info = &candidate.package.manifest.package;
            if seen.contains(&info.id)
                || (!info.id.to_lowercase().contains(&needle)
                    && !info.name.to_lowercase().contains(&needle))
            {
                continue;
            }
            let origin = candidate.origin_record(None);
            records.push(crate::kuang_host::package_record(
                &schema,
                &candidate.package,
                &origin.identity,
                already(&info.id, &info.version),
                host.trust(),
                None,
                SourceKind::SystemPackage,
                origin.system_package.as_deref(),
            )?);
        }
    }
    let mut problems = host.catalogs.problems.clone();
    problems.extend(rejected_roots(&scan));
    problems.extend(scan.failures);
    Ok((records, problems))
}

/// Every root the scan set aside, as the warning `find plugin` and `install plugin` show
/// (K11A §16.1).
#[must_use]
pub fn rejected_roots(scan: &SystemScan) -> Vec<ErrorValue> {
    scan.rejected
        .iter()
        .map(|(root, reason)| {
            ErrorValue::new(ErrorCode::PluginSourceRootRejected, reason.clone())
                .with_metadata("root", Value::Path(Arc::from(root.as_path())))
        })
        .collect()
}

/// A catalog entry whose release is not on this machine, as the catalog describes it: what it
/// does not say is null or `unknown`, never invented (spec §35.3).
fn catalog_record(
    schema: &Arc<Schema>,
    entry: &CatalogEntry,
    release: Option<&CatalogRelease>,
    catalog: (&str, &str),
    installed: bool,
) -> Result<RecordValue, ErrorValue> {
    Ok(
        RecordValue::builder(Arc::clone(schema), crate::kuang_host::provenance(schema))
            .set("id", Value::string(&entry.id))?
            .set("name", Value::string(&entry.name))?
            .set(
                "version",
                Value::string(release.map_or("unknown", |release| release.version.as_str())),
            )?
            .set("publisher", Value::string(&entry.publisher))?
            .set("summary", Value::string(&entry.description))?
            .set(
                "source",
                Value::string(release.map_or("catalog", |release| release.artifact.as_str())),
            )?
            .set(
                "source_kind",
                Value::string(SourceKind::CatalogNetwork.id()),
            )?
            .set("system_package", Value::Null)?
            .set("license", Value::string("unknown"))?
            .set(
                "kuang_api",
                Value::string(release.map_or("unknown", |release| release.kuang_api.source())),
            )?
            .set(
                "platforms",
                Value::list(
                    release
                        .map(|release| release.platforms.clone())
                        .unwrap_or_default()
                        .iter()
                        .map(|platform| Value::string(platform)),
                ),
            )?
            .set("roles", Value::list([]))?
            .set("contributions", crate::kuang_host::map([]))?
            .set("requested_capabilities", Value::list([]))?
            .set(
                "network",
                crate::kuang_host::map([("outbound", Value::string("unknown"))]),
            )?
            .set(
                "integrity",
                release
                    .and_then(|release| release.digest.as_deref())
                    .map_or(Value::Null, Value::string),
            )?
            .set("signature", Value::string("unknown"))?
            .set("trust", Value::string("unknown"))?
            .set("installed", Value::Bool(installed))?
            .set("catalog", Value::string(catalog.0))?
            .set("catalog_verification", Value::string(catalog.1))?
            .set("size", Value::Null)?
            .set(
                "published_at",
                release
                    .and_then(|release| release.published_at.as_deref())
                    .map_or(Value::Null, |at| {
                        Value::parse_timestamp(at).unwrap_or_else(|_| Value::string(at))
                    }),
            )?
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_ship_a_bootstrap_catalog_that_reads_and_names_the_reference_provider() {
        let catalog = Catalog::parse(BOOTSTRAP, CatalogVerification::BuiltIn).expect("reads");
        assert_eq!(catalog.name, "official");
        let kubernetes = catalog
            .entry("io.github.godspeed-you.kubernetes")
            .expect("the reference provider is in the bootstrap catalog (K11P §11.4)");
        assert_eq!(kubernetes.name, "kubernetes");
        assert!(!kubernetes.releases.is_empty());
    }
}
