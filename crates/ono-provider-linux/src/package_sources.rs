//! The repositories a package manager reads its index from, as `ono.package-source/1` records,
//! and the refresh that acts on one of them (issue #17, ADR-0562, ADR-0565).
//!
//! Spec §9.1 names no verb for the index. A mutation acts on the object it names (§11.5), and a
//! refresh has no package to name: its object is the repository, so the repository is an object
//! first. What is read here comes from the managers' machine interfaces or from their own
//! configuration, never from a human listing (spec §31.58, ADR-0115):
//!
//! - apt: `apt-get update --print-uris` (apt-get(8): "instead of fetching the files … their URIs
//!   are printed", with the destination file name), one line per index file, grouped into one
//!   source per repository root, suite and component. It answers before the first update ever
//!   ran, which `apt-get indextargets` does not; `indextargets` supplies the labels where a
//!   fetched index exists. The destination file names, under `Dir::State::lists` as
//!   `apt-config shell` reports it, are what `refreshed` is read from.
//! - dnf and yum: the `.repo` files of `/etc/yum.repos.d`, which are dnf's own configuration in
//!   the INI form dnf.conf(5) documents; `dnf repolist` is a human listing and is not read.
//! - zypper: `zypper --xmlout lr`, the machine interface zypper(8) documents.
//!
//! What a refresh changed is read from the index the manager keeps rather than from its prose:
//! the index file's modification time before against after.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_provider_api::{ObjectId, Query, Selector};
use ono_value::{ErrorValue, Provenance, RecordValue, Schema, SchemaId, Value, builtin_schemas};
use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};

/// The target this module answers for.
pub(crate) const TARGET: &str = "package-source";

/// The format `apt-get indextargets` is asked for: one index file per line, tab-separated.
pub(crate) const APT_INDEXTARGETS_FORMAT: &str =
    "$(REPO_URI)\t$(SUITE)\t$(COMPONENT)\t$(ORIGIN)\t$(LABEL)\t$(FILENAME)";

/// Where apt keeps fetched indexes when `apt-config` cannot be asked (apt.conf(5)'s default).
pub(crate) const APT_LISTS_DIR: &str = "/var/lib/apt/lists/";

/// How long one `apt-get update` counts as the run for the results that follow it.
///
/// apt refreshes every source in one run and cannot refresh one. A pipeline that asks for one
/// result per source — `get package-source | refresh package-source` — would otherwise run it
/// once per source; instead the run is made once and each result is read from its own index.
/// The window is measured from the moment the run finished, and the actions of one pipeline
/// follow each other within milliseconds (ADR-0565).
pub(crate) const APT_RUN_WINDOW: Duration = Duration::from_secs(5);

/// The `ono.package-source/1` schema, as `docs/contracts/schemas/package-source.v1.yaml` fixes it.
///
/// ```
/// let schema = ono_provider_linux::package_source_schema();
/// assert_eq!(schema.id().to_string(), "ono.package-source/1");
/// assert_eq!(schema.identity(), ["provider".into(), "id".into()]);
/// ```
#[must_use]
#[allow(
    clippy::expect_used,
    reason = "AGENTS.md section 16 admits `expect` in a provably unreachable state. \
              `ono.package-source/1` is embedded from docs/contracts/schemas/ at compile time and \
              crates/ono-value/tests/builtin_schemas.rs turns red the moment it is not."
)]
pub fn package_source_schema() -> Arc<Schema> {
    static SCHEMA: OnceLock<Arc<Schema>> = OnceLock::new();
    Arc::clone(SCHEMA.get_or_init(|| {
        builtin_schemas()
            .get(&SchemaId::new("ono.package-source", 1))
            .expect("ono.package-source/1 is one of the schemas the shell ships")
    }))
}

/// One repository, as the manager describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Source {
    /// The manager's id for it; the second half of the identity.
    pub(crate) id: String,
    /// The label it gives itself.
    pub(crate) name: Option<String>,
    /// Where its index comes from.
    pub(crate) url: Option<String>,
    /// Whether the manager reads it.
    pub(crate) enabled: Option<bool>,
    /// The index files the manager keeps for it; `refreshed` is their newest modification time.
    pub(crate) index_files: Vec<PathBuf>,
}

impl Source {
    /// When the local index was last written, read from the index files that exist.
    pub(crate) fn refreshed(&self) -> Option<Timestamp> {
        self.index_files
            .iter()
            .filter_map(|path| modified(path))
            .max()
    }
}

/// The modification time of `path`, or `None` when it cannot be read.
fn modified(path: &Path) -> Option<Timestamp> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let since_epoch = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    let seconds = i64::try_from(since_epoch.as_secs()).ok()?;
    crate::common::timestamp(seconds, i64::from(since_epoch.subsec_nanos()))
}

/// The `ono.package-source/1` record for `source`.
pub(crate) fn source_record(
    source: &Source,
    database: &str,
    provider_id: &str,
    origin: &str,
) -> Result<RecordValue, ErrorValue> {
    let schema = package_source_schema();
    let provenance = Provenance::local(provider_id, schema.id().clone())
        .from_source(origin)
        .observed_at(Timestamp::now());
    Ok(RecordValue::builder(schema, provenance)
        .set("id", Value::string(&source.id))?
        .set(
            "name",
            source.name.as_deref().map_or(Value::Null, Value::string),
        )?
        .set(
            "url",
            source.url.as_deref().map_or(Value::Null, Value::string),
        )?
        .set("enabled", source.enabled.map_or(Value::Null, Value::Bool))?
        .set(
            "refreshed",
            source.refreshed().map_or(Value::Null, Value::Timestamp),
        )?
        .set("provider", Value::string(database))?
        .build())
}

/// The source id an identity refers to: `ono.package-source/1` is `provider + id`.
pub(crate) fn source_id(id: &ObjectId) -> Result<&str, ErrorValue> {
    let expected = SchemaId::new("ono.package-source", 1);
    let name = match id.values() {
        [name] => Some(name),
        [_, name, ..] => Some(name),
        [] => None,
    };
    name.and_then(|value| value.as_str().ok())
        .filter(|_| id.schema() == &expected)
        .ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("`{id}` does not name a package source"),
            )
            .with_help("a refresh needs an `ono.package-source/1` identity of `provider` and `id`")
        })
}

/// Whether `selector` is about a package source rather than a package.
///
/// A provider resolves a selector without being told the target, and the two objects it answers
/// for are told apart by what the selector names: a package by `name`, a source by `id` or by an
/// `ono.package-source/1` identity.
pub(crate) fn is_source_selector(selector: &Selector) -> bool {
    match selector {
        Selector::Field { name, .. } => name == "id",
        Selector::Identity(id) => id.schema() == &SchemaId::new("ono.package-source", 1),
        _ => false,
    }
}

/// What a `package-source` query asks for.
pub(crate) struct SourcePlan {
    /// The one source it names, when it does.
    pub(crate) named: Option<String>,
    remaining: Vec<Selector>,
}

impl SourcePlan {
    pub(crate) fn of(query: &Query) -> Self {
        let mut plan = Self {
            named: None,
            remaining: Vec::new(),
        };
        for selector in query.selectors() {
            match selector {
                Selector::Field { name, value } if name == "id" && plan.named.is_none() => {
                    if let Ok(text) = value.as_str() {
                        plan.named = Some(text.to_owned());
                    }
                    plan.remaining.push(selector.clone());
                }
                Selector::Identity(id) if plan.named.is_none() => {
                    if let Ok(name) = source_id(id) {
                        plan.named = Some(name.to_owned());
                    }
                    plan.remaining.push(selector.clone());
                }
                other => plan.remaining.push(other.clone()),
            }
        }
        plan
    }

    pub(crate) fn keeps(&self, record: &RecordValue) -> bool {
        self.remaining
            .iter()
            .all(|selector| selector.matches(record))
    }
}

/// A `provider.schema_violation`: the tool answered in something other than its machine format.
fn violation(tool: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderSchemaViolation,
        format!("`{tool}` did not answer in its machine format: {detail}"),
    )
    .with_help("nothing was invented from it (spec §35.3); check the tool on this machine")
}

// --- apt ---------------------------------------------------------------------------------------

/// The id of an apt source: the repository root without its scheme, then the suite and the
/// component — `archive.ubuntu.com/ubuntu/noble/main`.
fn apt_source_id(repo_uri: &str, suite: &str, component: &str) -> String {
    let root = repo_uri
        .split_once("://")
        .map_or(repo_uri, |(_, rest)| rest)
        .trim_end_matches('/');
    let mut id = format!("{root}/{suite}");
    if !component.is_empty() {
        id.push('/');
        id.push_str(component);
    }
    id
}

/// Reads the sources out of what `apt-get indextargets --format` printed, one per repository
/// root, suite and component, in the order apt listed them.
///
/// # Errors
///
/// `provider.schema_violation` when a line does not carry the six fields the format asked for.
pub(crate) fn parse_indextargets(bytes: &[u8]) -> Result<Vec<Source>, ErrorValue> {
    let tool = "apt-get indextargets --format";
    let text = std::str::from_utf8(bytes)
        .map_err(|error| violation(tool, &format!("the answer is not UTF-8: {error}")))?;
    let mut sources: Vec<Source> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let [repo_uri, suite, component, origin, label, filename] = fields[..] else {
            return Err(violation(
                tool,
                &format!(
                    "a line carries {} fields where 6 were asked for",
                    fields.len()
                ),
            ));
        };
        if repo_uri.is_empty() || suite.is_empty() || component.is_empty() {
            return Err(violation(
                tool,
                "a line has no repository, suite or component",
            ));
        }
        let id = apt_source_id(repo_uri, suite, component);
        let name = [label, origin]
            .into_iter()
            .find(|text| !text.trim().is_empty())
            .map(|text| text.trim().to_owned());
        let index = PathBuf::from(filename);
        match sources.iter_mut().find(|source| source.id == id) {
            Some(source) => {
                if !filename.is_empty() && !source.index_files.contains(&index) {
                    source.index_files.push(index);
                }
                if source.name.is_none() {
                    source.name = name;
                }
            }
            None => sources.push(Source {
                id,
                name,
                url: Some(repo_uri.to_owned()),
                enabled: Some(true),
                index_files: if filename.is_empty() {
                    Vec::new()
                } else {
                    vec![index]
                },
            }),
        }
    }
    Ok(sources)
}

/// One index file `apt-get update --print-uris` would fetch: where from, and where to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AptIndex {
    /// The repository root, scheme and all, with its trailing `/`.
    pub(crate) repo_uri: String,
    /// The suite, `bookworm` or `noble-updates`; `./` for a flat repository.
    pub(crate) suite: String,
    /// The component, `main`; empty when the line is a release file or the repository is flat.
    pub(crate) component: String,
    /// The destination file name, relative to `Dir::State::lists`.
    pub(crate) filename: String,
}

/// Reads what `apt-get update --print-uris` printed: `'<uri>' <filename> <size> <hash>` per
/// line, in apt-get(8)'s documented shape.
///
/// # Errors
///
/// `provider.schema_violation` when a line is not in that shape.
pub(crate) fn parse_print_uris(bytes: &[u8]) -> Result<Vec<AptIndex>, ErrorValue> {
    let tool = "apt-get update --print-uris";
    let text = std::str::from_utf8(bytes)
        .map_err(|error| violation(tool, &format!("the answer is not UTF-8: {error}")))?;
    let mut indexes = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some(rest) = line.strip_prefix('\'') else {
            return Err(violation(tool, "a line does not start with a quoted URI"));
        };
        let Some((uri, rest)) = rest.split_once('\'') else {
            return Err(violation(tool, "a line's URI is not closed"));
        };
        let Some(filename) = rest.split_whitespace().next() else {
            return Err(violation(tool, "a line carries no destination file name"));
        };
        let (repo_uri, suite, component) = match uri.split_once("/dists/") {
            Some((root, path)) => {
                let mut parts = path.split('/');
                let suite = parts.next().unwrap_or_default().to_owned();
                let component = match parts.next() {
                    // `<suite>/InRelease`, `<suite>/Release`: the release file, no component.
                    Some(_) if path.matches('/').count() < 2 => String::new(),
                    Some(component) => component.to_owned(),
                    None => String::new(),
                };
                (format!("{root}/"), suite, component)
            }
            // A flat repository: `deb http://host/path ./`, whose files sit beside the root.
            None => {
                let root = uri.rsplit_once('/').map_or(uri, |(root, _)| root);
                (format!("{root}/"), "./".to_owned(), String::new())
            }
        };
        if repo_uri.is_empty() || suite.is_empty() {
            return Err(violation(
                tool,
                &format!("`{uri}` names no repository or suite"),
            ));
        }
        indexes.push(AptIndex {
            repo_uri,
            suite,
            component,
            filename: filename.to_owned(),
        });
    }
    Ok(indexes)
}

/// The directory `apt-config shell LISTS Dir::State::lists/f` reports, out of its
/// `LISTS='…'` answer.
pub(crate) fn parse_apt_lists_dir(bytes: &[u8]) -> Option<PathBuf> {
    let text = std::str::from_utf8(bytes).ok()?;
    let line = text.lines().find(|line| line.starts_with("LISTS="))?;
    let value = line.trim_start_matches("LISTS=").trim().trim_matches('\'');
    (!value.is_empty()).then(|| PathBuf::from(value))
}

/// apt's sources: one per repository root, suite and component, from the index files an update
/// would fetch, with the labels `indextargets` knows where an index has been fetched already.
pub(crate) fn apt_sources(
    indexes: &[AptIndex],
    lists_dir: &Path,
    labelled: &[Source],
) -> Vec<Source> {
    let mut sources: Vec<Source> = Vec::new();
    for index in indexes {
        let flat = index.suite == "./";
        if index.component.is_empty() && !flat {
            continue;
        }
        let id = if flat {
            apt_source_id(&index.repo_uri, "./", "")
        } else {
            apt_source_id(&index.repo_uri, &index.suite, &index.component)
        };
        let file = lists_dir.join(&index.filename);
        match sources.iter_mut().find(|source| source.id == id) {
            Some(source) => {
                if !source.index_files.contains(&file) {
                    source.index_files.push(file);
                }
            }
            None => sources.push(Source {
                name: labelled
                    .iter()
                    .find(|source| source.id == id)
                    .and_then(|source| source.name.clone()),
                id,
                url: Some(index.repo_uri.clone()),
                enabled: Some(true),
                index_files: vec![file],
            }),
        }
    }
    sources
}

/// What one `apt-get update` left behind, for the results that follow it (ADR-0565).
#[derive(Debug)]
pub(crate) struct AptRun {
    finished: Instant,
    before: BTreeMap<String, Option<Timestamp>>,
}

impl AptRun {
    pub(crate) fn new(before: BTreeMap<String, Option<Timestamp>>) -> Self {
        Self {
            finished: Instant::now(),
            before,
        }
    }

    /// The index time `id` had before this run, when the run is recent enough to count.
    pub(crate) fn before(&self, id: &str) -> Option<Option<Timestamp>> {
        if self.finished.elapsed() > APT_RUN_WINDOW {
            return None;
        }
        self.before.get(id).copied()
    }
}

// --- dnf and yum --------------------------------------------------------------------------------

/// The cache directories dnf and yum keep their downloaded metadata in, by generation.
pub(crate) const DNF_CACHE_DIRS: [&str; 3] =
    ["var/cache/libdnf5", "var/cache/dnf", "var/cache/yum"];

/// The repository configuration directory dnf.conf(5) documents.
pub(crate) const DNF_REPOS_DIR: &str = "etc/yum.repos.d";

/// Reads the sources out of the `.repo` files under `<root>/etc/yum.repos.d`, sorted by id.
///
/// `root` is `/` on a real machine. The index files are looked for under the cache directories
/// dnf keeps, `<cachedir>/<id>-<hash>/repodata/repomd.xml`, which is the layout dnf.conf(5)
/// describes; a repository without a cache has no `refreshed` yet.
///
/// # Errors
///
/// `provider.unavailable` when the directory exists and cannot be read.
pub(crate) fn read_repo_files(root: &Path) -> Result<Vec<Source>, ErrorValue> {
    let directory = root.join(DNF_REPOS_DIR);
    let entries = match std::fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!("`{}` could not be read: {error}", directory.display()),
            ));
        }
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "repo")
        })
        .collect();
    files.sort();
    let mut sources = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).map_err(|error| {
            ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!("`{}` could not be read: {error}", file.display()),
            )
        })?;
        for mut source in parse_repo_ini(&text) {
            source.index_files = dnf_index_files(root, &source.id);
            sources.push(source);
        }
    }
    sources.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(sources)
}

/// The `repomd.xml` files dnf keeps for `id`, in whichever cache generation has one.
pub(crate) fn dnf_index_files(root: &Path, id: &str) -> Vec<PathBuf> {
    let prefix = format!("{id}-");
    let mut found = Vec::new();
    for cache in DNF_CACHE_DIRS {
        let Ok(entries) = std::fs::read_dir(root.join(cache)) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with(&prefix) {
                continue;
            }
            let index = entry.path().join("repodata").join("repomd.xml");
            if index.is_file() {
                found.push(index);
            }
        }
    }
    found.sort();
    found
}

/// Reads one `.repo` file: a `[id]` section per repository, with `name`, `enabled` and one of
/// `baseurl`, `metalink` or `mirrorlist` (dnf.conf(5)).
pub(crate) fn parse_repo_ini(text: &str) -> Vec<Source> {
    let mut sources: Vec<Source> = Vec::new();
    let mut current: Option<Source> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if let Some(section) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            if let Some(done) = current.take() {
                sources.push(done);
            }
            let id = section.trim();
            if id.is_empty() || id == "main" {
                continue;
            }
            current = Some(Source {
                id: id.to_owned(),
                name: None,
                url: None,
                enabled: Some(true),
                index_files: Vec::new(),
            });
            continue;
        }
        let Some(source) = current.as_mut() else {
            continue;
        };
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        match key {
            "name" => source.name = Some(value.to_owned()),
            "baseurl" | "metalink" | "mirrorlist" => {
                // The first URL is the source; a `baseurl` can list several.
                if source.url.is_none() {
                    source.url = value.split_whitespace().next().map(str::to_owned);
                }
            }
            "enabled" => {
                source.enabled = match value {
                    "1" | "true" | "yes" | "True" => Some(true),
                    "0" | "false" | "no" | "False" => Some(false),
                    _ => None,
                };
            }
            _ => {}
        }
    }
    if let Some(done) = current.take() {
        sources.push(done);
    }
    sources
}

// --- zypper -------------------------------------------------------------------------------------

/// Where zypper keeps a repository's downloaded metadata: `<raw>/<alias>/repodata/repomd.xml`.
pub(crate) const ZYPPER_RAW_CACHE: &str = "var/cache/zypp/raw";

/// Reads the repositories out of the document `zypper --xmlout lr` printed.
///
/// The answer is a `repo` element per repository with its alias, name and `enabled` flag, and a
/// `url` child. `root` is where the cache is looked for, `/` on a real machine.
///
/// # Errors
///
/// `provider.schema_violation` when the bytes are not zypper's XML or a `repo` has no alias.
pub(crate) fn parse_zypper_repos(bytes: &[u8], root: &Path) -> Result<Vec<Source>, ErrorValue> {
    let tool = "zypper --xmlout lr";
    let mut reader = Reader::from_reader(bytes);
    let mut buffer = Vec::new();
    let mut sources: Vec<Source> = Vec::new();
    let mut seen = HashSet::new();
    let mut is_a_stream = false;
    let mut in_url = false;
    loop {
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(element)) if element.name().as_ref() == "stream" => {
                is_a_stream = true;
            }
            Ok(Event::Start(element) | Event::Empty(element))
                if element.name().as_ref() == "repo" =>
            {
                let mut alias = None;
                let mut name = None;
                let mut enabled = None;
                for attribute in element.attributes() {
                    let attribute = attribute.map_err(|error| {
                        violation(tool, &format!("a `repo` attribute: {error}"))
                    })?;
                    let value = attribute
                        .normalized_value(XmlVersion::Explicit1_0)
                        .map_err(|error| violation(tool, &format!("a `repo` attribute: {error}")))?
                        .into_owned();
                    match attribute.key.as_ref() {
                        "alias" => alias = Some(value),
                        "name" => name = Some(value),
                        "enabled" => {
                            enabled = match value.as_str() {
                                "1" | "true" => Some(true),
                                "0" | "false" => Some(false),
                                _ => None,
                            };
                        }
                        _ => {}
                    }
                }
                let Some(alias) = alias else {
                    return Err(violation(tool, "a `repo` element carries no `alias`"));
                };
                if seen.insert(alias.clone()) {
                    let index = root
                        .join(ZYPPER_RAW_CACHE)
                        .join(&alias)
                        .join("repodata")
                        .join("repomd.xml");
                    sources.push(Source {
                        id: alias,
                        name,
                        url: None,
                        enabled,
                        index_files: vec![index],
                    });
                }
            }
            Ok(Event::Start(element)) if element.name().as_ref() == "url" => in_url = true,
            Ok(Event::End(element)) if element.name().as_ref() == "url" => in_url = false,
            Ok(Event::Text(text)) if in_url => {
                let raw = text.into_inner();
                let url = quick_xml::escape::unescape(&raw)
                    .map_err(|error| violation(tool, &format!("a `url` element: {error}")))?
                    .trim()
                    .to_owned();
                if let Some(source) = sources.last_mut()
                    && source.url.is_none()
                    && !url.is_empty()
                {
                    source.url = Some(url);
                }
            }
            Ok(_) => {}
            Err(error) => return Err(violation(tool, &format!("{error}"))),
        }
        buffer.clear();
    }
    if !is_a_stream {
        return Err(violation(tool, "the document has no `stream` element"));
    }
    Ok(sources)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
mod tests {
    use super::*;

    #[test]
    fn should_group_apt_index_files_into_one_source_per_root_suite_and_component() {
        let listing = concat!(
            "http://de.archive.ubuntu.com/ubuntu/\tresolute\tmain\tUbuntu\tUbuntu\t/var/lib/apt/lists/de.archive.ubuntu.com_ubuntu_dists_resolute_main_binary-amd64_Packages\n",
            "http://de.archive.ubuntu.com/ubuntu/\tresolute\tmain\tUbuntu\tUbuntu\t/var/lib/apt/lists/de.archive.ubuntu.com_ubuntu_dists_resolute_main_i18n_Translation-en\n",
            "https://download.docker.com/linux/ubuntu/\tresolute\tstable\tDocker\tDocker CE\t/var/lib/apt/lists/download.docker.com_linux_ubuntu_dists_resolute_stable_binary-amd64_Packages\n",
        );
        let sources = parse_indextargets(listing.as_bytes()).expect("apt's format");
        assert_eq!(sources.len(), 2, "two sources, three index files");
        assert_eq!(sources[0].id, "de.archive.ubuntu.com/ubuntu/resolute/main");
        assert_eq!(sources[0].name.as_deref(), Some("Ubuntu"));
        assert_eq!(
            sources[0].url.as_deref(),
            Some("http://de.archive.ubuntu.com/ubuntu/")
        );
        assert_eq!(sources[0].enabled, Some(true));
        assert_eq!(sources[0].index_files.len(), 2);
        assert_eq!(
            sources[1].id,
            "download.docker.com/linux/ubuntu/resolute/stable"
        );
        assert_eq!(
            sources[1].name.as_deref(),
            Some("Docker CE"),
            "the label before the origin"
        );
    }

    #[test]
    fn should_group_the_uris_an_update_would_fetch_into_sources_with_their_list_files() {
        let printed = concat!(
            "'http://deb.debian.org/debian/dists/bookworm/InRelease' deb.debian.org_debian_dists_bookworm_InRelease 0 \n",
            "'http://deb.debian.org/debian/dists/bookworm/main/binary-amd64/Packages.xz' deb.debian.org_debian_dists_bookworm_main_binary-amd64_Packages 0 \n",
            "'http://deb.debian.org/debian/dists/bookworm/main/binary-all/Packages.xz' deb.debian.org_debian_dists_bookworm_main_binary-all_Packages 0 \n",
            "'http://deb.debian.org/debian-security/dists/bookworm-security/main/binary-amd64/Packages.xz' deb.debian.org_debian-security_dists_bookworm-security_main_binary-amd64_Packages 0 \n",
            "'https://flat.example/repo/Packages.gz' flat.example_repo_Packages 0 \n",
        );
        let indexes = parse_print_uris(printed.as_bytes()).expect("apt's --print-uris shape");
        assert_eq!(indexes.len(), 5);
        assert_eq!(
            indexes[0].component, "",
            "a release file names no component"
        );
        assert_eq!(indexes[1].suite, "bookworm");
        assert_eq!(indexes[1].component, "main");
        assert_eq!(indexes[4].suite, "./", "a flat repository");

        let lists = Path::new("/var/lib/apt/lists");
        let sources = apt_sources(&indexes, lists, &[]);
        assert_eq!(
            sources.len(),
            3,
            "release files add no source; two arches are one source"
        );
        assert_eq!(sources[0].id, "deb.debian.org/debian/bookworm/main");
        assert_eq!(
            sources[0].url.as_deref(),
            Some("http://deb.debian.org/debian/")
        );
        assert_eq!(sources[0].name, None, "nothing fetched yet, so no label");
        assert_eq!(
            sources[0].index_files,
            vec![
                lists.join("deb.debian.org_debian_dists_bookworm_main_binary-amd64_Packages"),
                lists.join("deb.debian.org_debian_dists_bookworm_main_binary-all_Packages"),
            ]
        );
        assert_eq!(
            sources[1].id,
            "deb.debian.org/debian-security/bookworm-security/main"
        );
        assert_eq!(sources[2].id, "flat.example/repo/./");
    }

    #[test]
    fn should_take_the_label_from_indextargets_where_an_index_has_been_fetched() {
        let printed = "'http://archive.ubuntu.com/ubuntu/dists/noble/main/binary-amd64/Packages.xz' archive.ubuntu.com_ubuntu_dists_noble_main_binary-amd64_Packages 0 \n";
        let listed = "http://archive.ubuntu.com/ubuntu/\tnoble\tmain\tUbuntu\tUbuntu\t/var/lib/apt/lists/archive.ubuntu.com_ubuntu_dists_noble_main_binary-amd64_Packages\n";
        let indexes = parse_print_uris(printed.as_bytes()).expect("shape");
        let labelled = parse_indextargets(listed.as_bytes()).expect("shape");
        let sources = apt_sources(&indexes, Path::new("/var/lib/apt/lists"), &labelled);
        assert_eq!(sources[0].name.as_deref(), Some("Ubuntu"));
    }

    #[test]
    fn should_read_the_lists_directory_out_of_apt_configs_shell_answer() {
        assert_eq!(
            parse_apt_lists_dir(b"LISTS='/var/lib/apt/lists/'\n"),
            Some(PathBuf::from("/var/lib/apt/lists/"))
        );
        assert_eq!(parse_apt_lists_dir(b"E: something else\n"), None);
    }

    #[test]
    fn should_refuse_uris_that_are_not_in_the_print_uris_shape() {
        let error = parse_print_uris(b"Hit:1 http://deb.debian.org/debian bookworm InRelease\n")
            .expect_err("prose");
        assert_eq!(error.code(), ErrorCode::ProviderSchemaViolation);
    }

    #[test]
    fn should_refuse_an_apt_listing_that_is_not_in_the_asked_format() {
        let error = parse_indextargets(b"Reading package lists... Done\n").expect_err("prose");
        assert_eq!(error.code(), ErrorCode::ProviderSchemaViolation);
    }

    #[test]
    fn should_read_dnf_repo_files_as_sources_with_their_enabled_flag() {
        let ini = concat!(
            "[main]\ngpgcheck=1\n\n",
            "[fedora]\nname=Fedora $releasever - $basearch\n",
            "metalink=https://mirrors.fedoraproject.org/metalink?repo=fedora-$releasever&arch=$basearch\n",
            "enabled=1\n\n",
            "# a commented section stays out\n",
            "[fedora-debuginfo]\nname=Fedora - Debug\nbaseurl=https://a.example/debug https://b.example/debug\nenabled=0\n",
        );
        let sources = parse_repo_ini(ini);
        assert_eq!(
            sources.len(),
            2,
            "`[main]` is dnf's own section, not a repository"
        );
        assert_eq!(sources[0].id, "fedora");
        assert_eq!(
            sources[0].name.as_deref(),
            Some("Fedora $releasever - $basearch")
        );
        assert!(
            sources[0]
                .url
                .as_deref()
                .unwrap()
                .starts_with("https://mirrors.fedoraproject.org/")
        );
        assert_eq!(sources[0].enabled, Some(true));
        assert_eq!(sources[1].id, "fedora-debuginfo");
        assert_eq!(
            sources[1].url.as_deref(),
            Some("https://a.example/debug"),
            "the first of several"
        );
        assert_eq!(sources[1].enabled, Some(false));
    }

    #[test]
    fn should_read_zypper_repositories_with_their_alias_name_url_and_enabled_flag() {
        let xml = concat!(
            "<?xml version='1.0'?>\n<stream>\n<repo-list>\n",
            "<repo alias=\"repo-oss\" name=\"Main Repository\" type=\"rpm-md\" enabled=\"1\" autorefresh=\"1\">",
            "<url>http://download.opensuse.org/tumbleweed/repo/oss/</url></repo>\n",
            "<repo alias=\"repo-debug\" name=\"Debug Repository\" type=\"NONE\" enabled=\"0\" autorefresh=\"0\">",
            "<url>http://download.opensuse.org/debug/tumbleweed/repo/oss/</url></repo>\n",
            "</repo-list>\n</stream>\n",
        );
        let sources = parse_zypper_repos(xml.as_bytes(), Path::new("/")).expect("zypper's XML");
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].id, "repo-oss");
        assert_eq!(sources[0].name.as_deref(), Some("Main Repository"));
        assert_eq!(
            sources[0].url.as_deref(),
            Some("http://download.opensuse.org/tumbleweed/repo/oss/")
        );
        assert_eq!(sources[0].enabled, Some(true));
        assert_eq!(sources[1].enabled, Some(false));
        assert_eq!(
            sources[0].index_files,
            vec![PathBuf::from(
                "/var/cache/zypp/raw/repo-oss/repodata/repomd.xml"
            )]
        );
    }

    #[test]
    fn should_refuse_zypper_output_that_is_not_its_xml_stream() {
        let error = parse_zypper_repos(b"# | Alias | Name | Enabled\n", Path::new("/"))
            .expect_err("a human table");
        assert_eq!(error.code(), ErrorCode::ProviderSchemaViolation);
    }

    #[test]
    fn should_read_refreshed_from_the_newest_index_file_that_exists() {
        let directory = ono_testkit::scratch();
        directory.write("older", "");
        std::thread::sleep(Duration::from_millis(20));
        directory.write("newer", "");
        let source = Source {
            id: "x".to_owned(),
            name: None,
            url: None,
            enabled: None,
            index_files: vec![
                directory.path().join("older"),
                directory.path().join("newer"),
                directory.path().join("absent"),
            ],
        };
        let refreshed = source.refreshed().expect("two files exist");
        assert_eq!(Some(refreshed), modified(&directory.path().join("newer")));
    }

    #[test]
    fn should_tell_a_source_selector_from_a_package_selector() {
        assert!(is_source_selector(&Selector::field(
            "id",
            Value::string("updates")
        )));
        assert!(!is_source_selector(&Selector::field(
            "name",
            Value::string("curl")
        )));
        let source = ObjectId::new(
            SchemaId::new("ono.package-source", 1),
            [Value::string("rpm"), Value::string("updates")],
        );
        assert!(is_source_selector(&Selector::Identity(source.clone())));
        assert_eq!(source_id(&source).expect("a source identity"), "updates");
        let package = ObjectId::new(
            SchemaId::new("ono.package", 1),
            [Value::string("dpkg"), Value::string("curl")],
        );
        assert!(!is_source_selector(&Selector::Identity(package.clone())));
        assert!(source_id(&package).is_err());
    }
}
