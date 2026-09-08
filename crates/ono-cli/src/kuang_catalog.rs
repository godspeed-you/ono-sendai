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

use crate::kuang_host::{Host, Installed, integrity_of, read_package, schema};

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

/// A package a reference resolved to, before anything is verified (ADR-0601).
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

impl Host {
    /// The catalogs this session resolves against.
    #[must_use]
    pub fn catalogs(&self) -> &Catalogs {
        &self.catalogs
    }

    /// Resolves a reference for installation, in K11P §10.1's order (ADR-0601 §1).
    ///
    /// # Errors
    ///
    /// `plugin.not_found`, `plugin.reference_ambiguous` (with `candidates` in the metadata),
    /// `plugin.release_not_compatible`, `plugin.catalog_unavailable`, or a source scheme this
    /// build does not resolve.
    pub fn locate_for_install(&self, reference: &PluginRef) -> Result<Located, ErrorValue> {
        match reference {
            PluginRef::Path(path) => {
                let directory = expand_home(path, self.home_dir().as_deref());
                match read_package(&directory) {
                    Ok(Some(package)) => Ok(Located {
                        package,
                        source: format!("path:{}", directory.display()),
                        catalog: None,
                        entry: None,
                        release: None,
                    }),
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
                "a short name resolves through the catalogs, and `path:<directory>` or `./dir` \
                 names a local package (K11P §4.1, ADR-0601)",
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
                    [entry] => self.locate_release(found, entry),
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
            PluginRef::Word(word) => self.locate_word(word),
        }
    }

    fn locate_word(&self, word: &str) -> Result<Located, ErrorValue> {
        // An exact canonical id: installed, or offered by a catalog (K11P §10.1 step 3).
        if looks_like_id(word) {
            let offered: Vec<(&Catalog, &CatalogEntry)> = self
                .catalogs
                .catalogs
                .iter()
                .filter_map(|catalog| catalog.entry(word).map(|entry| (catalog, entry)))
                .collect();
            if let Some((catalog, entry)) = offered.first() {
                return self.locate_release(catalog, entry);
            }
            if let Some(package) = self.installed_package(word) {
                return Ok(Located {
                    source: format!("path:{}", package.directory.display()),
                    package,
                    catalog: None,
                    entry: None,
                    release: None,
                });
            }
        }
        // A short name across the catalogs (step 5). The same id in two catalogs is one
        // package; different ids under one name are an ambiguity nothing resolves by order.
        let mut candidates: Vec<(&Catalog, &CatalogEntry)> = Vec::new();
        for catalog in &self.catalogs.catalogs {
            for entry in catalog.named(word) {
                if candidates
                    .iter()
                    .all(|(_, existing)| existing.id != entry.id)
                {
                    candidates.push((catalog, entry));
                }
            }
        }
        match candidates.as_slice() {
            [(catalog, entry)] => self.locate_release(catalog, entry),
            [] => {
                // An installed package by its name, so `install plugin <name>` of something
                // already installed says so rather than "not found".
                if let Ok(package) = self.resolve_installed(word) {
                    return Ok(Located {
                        source: format!("path:{}", package.directory.display()),
                        package,
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
                    .map(|(catalog, entry)| Candidate {
                        id: entry.id.clone(),
                        selector: format!("{}/{}", catalog.name, entry.name),
                        description: entry.description.clone(),
                    })
                    .collect(),
            )),
        }
    }

    /// The newest release this host can run, found locally (ADR-0601 §3).
    fn locate_release(
        &self,
        catalog: &Catalog,
        entry: &CatalogEntry,
    ) -> Result<Located, ErrorValue> {
        let platform = ono_kuang_supervisor::host_platform();
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
        let (package, source) = self.local_release(entry, release).ok_or_else(|| {
            let mut looked = Vec::new();
            if let Some(cache) = &self.cache_dir {
                looked.push(Value::Path(Arc::from(cache.as_path())));
            }
            looked.extend(
                self.sources
                    .iter()
                    .map(|source| Value::Path(Arc::from(source.as_path()))),
            );
            ErrorValue::new(
                ErrorCode::PluginCatalogUnavailable,
                format!(
                    "`{}` {} is in the `{}` catalog and its artifact `{}` is not available to \
                     this build, which fetches nothing over the network",
                    entry.id, release.version, catalog.name, release.artifact
                ),
            )
            .with_help(format!(
                "place the unpacked package under {}, or install it from where it is with \
                 `install plugin path:<directory>` (ADR-0601)",
                self.sources.first().map_or_else(
                    || "a local package source".to_owned(),
                    |source| source.display().to_string()
                )
            ))
            .with_metadata("artifact", Value::string(&release.artifact))
            .with_metadata("looked_in", Value::list(looked))
        })?;
        if package.manifest.package.id != entry.id {
            return Err(ErrorValue::new(
                ErrorCode::KuangPackageIntegrityFailed,
                format!(
                    "the package at {} identifies as `{}`, and the catalog entry is `{}`",
                    source, package.manifest.package.id, entry.id
                ),
            ));
        }
        if let Some(digest) = &release.digest {
            let actual = integrity_of(&package);
            if &actual != digest {
                return Err(ErrorValue::new(
                    ErrorCode::KuangPackageIntegrityFailed,
                    format!(
                        "`{}` {}: the catalog vouches for {digest} and the local package hashes \
                         to {actual}",
                        entry.id, release.version
                    ),
                )
                .with_help(
                    "a hash mismatch is indistinguishable from a substituted artifact; \
                     re-obtain the package (K11P §11.3)",
                ));
            }
        }
        Ok(Located {
            package,
            source,
            catalog: Some((catalog.name.clone(), catalog.verification)),
            entry: Some(entry.clone()),
            release: Some(release.clone()),
        })
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

/// `find plugin <term>` over the catalogs (K11P §11.6): every entry whose id or name contains
/// the term, as `ono.plugin-package/1` records — from the local package where the release is
/// here, and from the catalog's own metadata where it is not.
///
/// # Errors
///
/// `provider.schema_violation` when a record does not fit its contract.
pub fn search(
    host: &Host,
    term: &str,
    installed: &[Installed],
) -> Result<(Vec<RecordValue>, Vec<ErrorValue>), ErrorValue> {
    let schema = schema("ono.plugin-package")?;
    let needle = term.to_lowercase();
    let mut records = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
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
            let platform = ono_kuang_supervisor::host_platform();
            let release = entry
                .release_for(HOST_API, &platform)
                .ok()
                .or_else(|| entry.releases.first());
            let already = |version: &str| {
                installed.iter().any(|held| {
                    held.manifest.package.id == entry.id && held.manifest.package.version == version
                })
            };
            let catalog_words = (catalog.name.as_str(), catalog.verification.id());
            match release.and_then(|release| {
                host.local_release(entry, release)
                    .map(|found| (release, found))
            }) {
                Some((release, (package, source))) => {
                    records.push(crate::kuang_host::package_record(
                        &schema,
                        &package,
                        &source,
                        already(&release.version),
                        host.trust(),
                        Some(catalog_words),
                    )?)
                }
                None => records.push(catalog_record(
                    &schema,
                    entry,
                    release,
                    catalog_words,
                    release.is_some_and(|release| already(&release.version)),
                )?),
            }
        }
    }
    Ok((records, host.catalogs.problems.clone()))
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
