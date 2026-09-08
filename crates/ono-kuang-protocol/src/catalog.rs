//! Plugin references and catalogs: how a short name becomes a canonical package without
//! running anything (K11P §4.1–§4.3, §10, §11; ADR-0601).
//!
//! A [`PluginRef`] is what a user typed; a [`Catalog`] is a signed-or-operator-placed index that
//! maps names to releases. Reading either executes no package code, and neither confers trust:
//! a catalog saying a package exists makes nobody trusted (K11P §11.3).

use std::cmp::Ordering;
use std::path::PathBuf;

use serde::Deserialize;

use crate::{ApiVersion, KuangError, KuangErrorCode, VersionRange};

/// The catalog document format this build reads.
pub const CATALOG_FORMAT: &str = "kuang-catalog/1";

/// A user-supplied plugin selector (K11P §4.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginRef {
    /// An explicit local package directory: `./x`, `../x`, `/x`, `~/x`, or the legacy
    /// `path:<dir>`. Never a bare word.
    Path(PathBuf),
    /// A source scheme this build does not resolve, e.g. `registry:` or `git:`.
    Source {
        /// The scheme, without its colon.
        scheme: String,
        /// The reference as written.
        reference: String,
    },
    /// `<catalog>/<name>`: a name inside one catalog.
    Catalog {
        /// The catalog's name.
        catalog: String,
        /// The package's short name.
        name: String,
    },
    /// A bare word: an exact canonical id, or a short name (K11P §10.1's steps 2 and 4).
    Word(String),
}

impl PluginRef {
    /// Reads a reference. A bare word is never a filesystem path (K11P §10.1).
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let text = text.trim();
        if let Some(path) = text.strip_prefix("path:") {
            return PluginRef::Path(PathBuf::from(path));
        }
        if text == "."
            || text == ".."
            || text.starts_with("./")
            || text.starts_with("../")
            || text.starts_with('/')
            || text == "~"
            || text.starts_with("~/")
        {
            return PluginRef::Path(PathBuf::from(text));
        }
        if let Some((scheme, _)) = text.split_once(':')
            && !scheme.is_empty()
            && scheme
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '+' || c == '-')
        {
            return PluginRef::Source {
                scheme: scheme.to_owned(),
                reference: text.to_owned(),
            };
        }
        if let Some((catalog, name)) = text.split_once('/')
            && !catalog.is_empty()
            && !name.is_empty()
            && !name.contains('/')
        {
            return PluginRef::Catalog {
                catalog: catalog.to_owned(),
                name: name.to_owned(),
            };
        }
        PluginRef::Word(text.to_owned())
    }

    /// Whether the reference names an explicit source rather than a name to resolve.
    #[must_use]
    pub fn is_explicit(&self) -> bool {
        matches!(self, PluginRef::Path(_) | PluginRef::Source { .. })
    }

    /// The reference as it was written, for messages and records.
    #[must_use]
    pub fn display(&self) -> String {
        match self {
            PluginRef::Path(path) => format!("path:{}", path.display()),
            PluginRef::Source { reference, .. } => reference.clone(),
            PluginRef::Catalog { catalog, name } => format!("{catalog}/{name}"),
            PluginRef::Word(word) => word.clone(),
        }
    }
}

/// Whether `word` could be a canonical package id: at least two lowercase dotted segments.
#[must_use]
pub fn looks_like_id(word: &str) -> bool {
    word.contains('.')
        && word.split('.').all(|segment| {
            let mut chars = segment.chars();
            chars.next().is_some_and(|first| first.is_ascii_lowercase())
                && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

/// How the shell knows a catalog is what it claims to be (K11P §11.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogVerification {
    /// Part of the `ono` release, covered by its integrity chain (K11P §11.4).
    BuiltIn,
    /// Placed by the operator under the configuration directory, like `trust.yaml`.
    Operator,
    /// A document that claims a remote origin this build cannot verify.
    Unknown,
}

impl CatalogVerification {
    /// The word `ono.plugin-package/1.catalog_verification` carries.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            CatalogVerification::BuiltIn => "built-in",
            CatalogVerification::Operator => "operator",
            CatalogVerification::Unknown => "unknown",
        }
    }
}

/// One release a catalog offers (K11P §11.2).
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogRelease {
    /// The version.
    pub version: String,
    /// The platform tuples the artifact supports.
    pub platforms: Vec<String>,
    /// The host API range.
    pub kuang_api: VersionRange,
    /// The Ono language range.
    pub ono_language: String,
    /// Where the artifact is: a source reference such as `path:/srv/packages/x` or
    /// `git:https://…#v1`. This build resolves local artifacts only (ADR-0601 §3).
    pub artifact: String,
    /// The content hash the catalog vouches for, `sha256:…`, or `None` when it does not.
    pub digest: Option<String>,
    /// The signing key the catalog expects, `ed25519:…`, or `None`.
    pub signing_key: Option<String>,
    /// When the catalog says the release was published.
    pub published_at: Option<String>,
}

impl CatalogRelease {
    /// Whether this host can run the release, or why not (K11P §10.1, `plugin.release_not_compatible`).
    ///
    /// # Errors
    ///
    /// The dimension that excludes this host.
    pub fn compatible(&self, host_api: ApiVersion, platform: &str) -> Result<(), String> {
        if !self.kuang_api.contains(host_api) {
            return Err(format!(
                "kuang_api `{}` excludes this host's `{host_api}`",
                self.kuang_api
            ));
        }
        if !self.platforms.iter().any(|candidate| candidate == platform) {
            return Err(format!(
                "platforms {:?} exclude `{platform}`",
                self.platforms
            ));
        }
        Ok(())
    }
}

/// One package a catalog knows (K11P §11.2).
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogEntry {
    /// The canonical package id.
    pub id: String,
    /// The short name.
    pub name: String,
    /// One line, what the package is for.
    pub description: String,
    /// The publisher namespace.
    pub publisher: String,
    /// The releases, newest first once parsed.
    pub releases: Vec<CatalogRelease>,
    /// Categories, for humans.
    pub categories: Vec<String>,
    /// A URL for humans. Never fetched.
    pub homepage: Option<String>,
    /// Why the package is deprecated, when it is.
    pub deprecated: Option<String>,
}

impl CatalogEntry {
    /// The newest release this host can run, or every reason none can.
    ///
    /// # Errors
    ///
    /// One line per release, newest first, saying what excluded it.
    pub fn release_for(
        &self,
        host_api: ApiVersion,
        platform: &str,
    ) -> Result<&CatalogRelease, Vec<String>> {
        let mut reasons = Vec::new();
        for release in &self.releases {
            match release.compatible(host_api, platform) {
                Ok(()) => return Ok(release),
                Err(reason) => reasons.push(format!("{}: {reason}", release.version)),
            }
        }
        Err(reasons)
    }

    /// The release with exactly this version.
    #[must_use]
    pub fn release(&self, version: &str) -> Option<&CatalogRelease> {
        self.releases
            .iter()
            .find(|release| release.version == version)
    }
}

/// A catalog: a name, and entries (K11P §11.1).
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    /// The catalog's name, what `<catalog>/<name>` selects by.
    pub name: String,
    /// One line about it.
    pub description: String,
    /// How the shell knows it.
    pub verification: CatalogVerification,
    /// What it offers.
    pub entries: Vec<CatalogEntry>,
}

impl Catalog {
    /// Reads a `kuang-catalog/1` document. Nothing is executed.
    ///
    /// # Errors
    ///
    /// `package.invalid` for a document that is not a catalog, or whose entries break the
    /// identity rules a package would have to satisfy.
    pub fn parse(text: &str, verification: CatalogVerification) -> Result<Self, KuangError> {
        let invalid = |detail: String| {
            KuangError::new(
                KuangErrorCode::PackageInvalid,
                format!("not a valid {CATALOG_FORMAT} document: {detail}"),
            )
        };
        let depth = ono_value::yaml_depth(text);
        if depth > ono_value::MAX_YAML_DEPTH {
            return Err(invalid(format!(
                "the document nests {depth} collections deep, and {} is the limit",
                ono_value::MAX_YAML_DEPTH
            )));
        }
        let raw: RawCatalog =
            serde_yaml_ng::from_str(text).map_err(|error| invalid(error.to_string()))?;
        if raw.format != CATALOG_FORMAT {
            return Err(invalid(format!(
                "format is `{}`, this host reads `{CATALOG_FORMAT}`",
                raw.format
            )));
        }
        if !crate::is_role_word(&raw.catalog.name) {
            return Err(invalid(format!(
                "`{}` is not a catalog name: a kebab-case slug",
                raw.catalog.name
            )));
        }
        let mut entries = Vec::new();
        for entry in raw.entries {
            if !looks_like_id(&entry.id) {
                return Err(invalid(format!(
                    "`{}` is not a reverse-DNS package id",
                    entry.id
                )));
            }
            if entry.id == "ono" || entry.id.starts_with("ono.") {
                return Err(invalid(format!(
                    "`{}` claims the `ono.*` namespace, which only the Ono project may claim",
                    entry.id
                )));
            }
            if !entry
                .id
                .strip_prefix(&entry.publisher)
                .is_some_and(|rest| rest.starts_with('.') && rest.len() > 1)
            {
                return Err(invalid(format!(
                    "`{}` does not begin with its publisher `{}`",
                    entry.id, entry.publisher
                )));
            }
            if entry.name.trim().is_empty() || entry.name.contains(['/', ':', ' ']) {
                return Err(invalid(format!(
                    "`{}` is not a short name for `{}`",
                    entry.name, entry.id
                )));
            }
            if entries
                .iter()
                .any(|existing: &CatalogEntry| existing.id == entry.id)
            {
                return Err(invalid(format!("`{}` is listed twice", entry.id)));
            }
            let mut releases = Vec::new();
            for release in entry.releases {
                let kuang_api: VersionRange = release.kuang_api.parse()?;
                if release.platforms.is_empty() {
                    return Err(invalid(format!(
                        "release {} of `{}` supports no platform",
                        release.version, entry.id
                    )));
                }
                if release.artifact.trim().is_empty() {
                    return Err(invalid(format!(
                        "release {} of `{}` names no artifact",
                        release.version, entry.id
                    )));
                }
                if let Some(digest) = &release.digest
                    && !digest.starts_with("sha256:")
                {
                    return Err(invalid(format!(
                        "release {} of `{}` carries a digest that is not `sha256:…`",
                        release.version, entry.id
                    )));
                }
                releases.push(CatalogRelease {
                    version: release.version,
                    platforms: release.platforms,
                    kuang_api,
                    ono_language: release.ono_language.unwrap_or_else(|| ">=0.2".to_owned()),
                    artifact: release.artifact,
                    digest: release.digest,
                    signing_key: release.signing_key,
                    published_at: release.published_at,
                });
            }
            releases.sort_by(|a, b| compare_versions(&b.version, &a.version));
            entries.push(CatalogEntry {
                id: entry.id,
                name: entry.name,
                description: crate::sanitize(&entry.description, crate::PURPOSE_LIMIT),
                publisher: entry.publisher,
                releases,
                categories: entry.categories,
                homepage: entry.homepage,
                deprecated: entry.deprecated,
            });
        }
        Ok(Self {
            name: raw.catalog.name,
            description: crate::sanitize(&raw.catalog.description, crate::PURPOSE_LIMIT),
            verification,
            entries,
        })
    }

    /// The entry with this canonical id.
    #[must_use]
    pub fn entry(&self, id: &str) -> Option<&CatalogEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// Every entry with this short name.
    pub fn named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a CatalogEntry> + 'a {
        self.entries.iter().filter(move |entry| entry.name == name)
    }
}

/// Compares two versions numerically, segment by segment; a pre-release suffix sorts below
/// the release it precedes.
#[must_use]
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    fn split(version: &str) -> (Vec<u64>, Option<&str>) {
        let (core, pre) = version
            .split_once('-')
            .map_or((version, None), |(core, pre)| (core, Some(pre)));
        (
            core.split('.')
                .map(|part| part.parse().unwrap_or(0))
                .collect(),
            pre,
        )
    }
    let (a_core, a_pre) = split(a);
    let (b_core, b_pre) = split(b);
    match a_core.cmp(&b_core) {
        Ordering::Equal => match (a_pre, b_pre) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(a), Some(b)) => a.cmp(b),
        },
        other => other,
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCatalog {
    format: String,
    catalog: RawCatalogInfo,
    #[serde(default)]
    entries: Vec<RawEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCatalogInfo {
    name: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    id: String,
    name: String,
    #[serde(default)]
    description: String,
    publisher: String,
    #[serde(default)]
    releases: Vec<RawRelease>,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    homepage: Option<String>,
    #[serde(default)]
    deprecated: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRelease {
    version: String,
    platforms: Vec<String>,
    kuang_api: String,
    #[serde(default)]
    ono_language: Option<String>,
    artifact: String,
    #[serde(default)]
    digest: Option<String>,
    #[serde(default)]
    signing_key: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_read_every_reference_form_of_the_specification() {
        assert_eq!(
            PluginRef::parse("kubernetes"),
            PluginRef::Word("kubernetes".into())
        );
        assert_eq!(
            PluginRef::parse("io.github.godspeed-you.kubernetes"),
            PluginRef::Word("io.github.godspeed-you.kubernetes".into())
        );
        assert_eq!(
            PluginRef::parse("./my-plugin"),
            PluginRef::Path(PathBuf::from("./my-plugin"))
        );
        assert_eq!(
            PluginRef::parse("/srv/packages/my-plugin"),
            PluginRef::Path(PathBuf::from("/srv/packages/my-plugin"))
        );
        assert_eq!(
            PluginRef::parse("path:/srv/packages/my-plugin"),
            PluginRef::Path(PathBuf::from("/srv/packages/my-plugin"))
        );
        assert_eq!(
            PluginRef::parse("official/kubernetes"),
            PluginRef::Catalog {
                catalog: "official".into(),
                name: "kubernetes".into()
            }
        );
        assert!(matches!(
            PluginRef::parse("registry:dev.example/packet-eye@2.4.1"),
            PluginRef::Source { scheme, .. } if scheme == "registry"
        ));
    }

    #[test]
    fn should_never_read_a_bare_word_as_a_path() {
        assert_eq!(
            PluginRef::parse("plugins"),
            PluginRef::Word("plugins".into())
        );
    }

    #[test]
    fn should_order_versions_numerically_rather_than_lexically() {
        assert_eq!(compare_versions("0.10.0", "0.9.0"), Ordering::Greater);
        assert_eq!(compare_versions("1.0.0-rc1", "1.0.0"), Ordering::Less);
    }
}
