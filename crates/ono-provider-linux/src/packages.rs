//! The `package` provider: dpkg and apt, through their machine formats.
//!
//! Spec §31.58 and AGENTS.md §6 fix the rule — a provider asks a tool for an explicit
//! machine-readable mode and never parses its human listing — and the Debian tools have such
//! modes: `dpkg-query -W -f` prints exactly the fields a format string names, tab-separated,
//! one package per line; `apt-cache search` prints `name - description`, one hit per line, as
//! apt-cache(8) documents. Everything an `ono.package/1` record carries comes from one of those
//! two answers (spec §50). A listing that is not in the declared format is a provider defect —
//! `provider.schema_violation` — and never a source of records named after whatever the bytes
//! happened to say (spec §35.3, ADR-0115).
//!
//! # Being honest about not being there
//!
//! The managers are discovered on `PATH`, never at an absolute path. Where none is found the
//! provider reports [`Availability::Unavailable`] naming what it looked for, and refuses to
//! answer queries: an empty list would say "no packages", which is false on every machine.
//! Only dpkg is served by this build; the refusal says so about rpm rather than pretending to
//! have looked (ADR-0115).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, OnceLock};

use jiff::Timestamp;
use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, ObjectId, ObjectRef, Provider, Query, Risk,
    Selector,
};
use ono_value::{ErrorValue, Provenance, RecordValue, Schema, SchemaId, Value, builtin_schemas};
use tokio::sync::Mutex;

use crate::package_sources::{self, AptRun, Source, SourcePlan};

/// The id this provider signs its records with.
pub const PACKAGE_PROVIDER_ID: &str = "linux.packages";

/// The `provider` field of a record dpkg answered, and the first half of its identity.
const DPKG: &str = "dpkg";

/// The format `dpkg-query -W -f` is asked for: name, version, status — tab-separated.
const LISTING_FORMAT: &str = "${Package}\\t${Version}\\t${Status}\\n";

/// The `ono.package/1` schema, as `docs/contracts/schemas/package.v1.yaml` fixes it.
///
/// ```
/// let schema = ono_provider_linux::package_schema();
/// assert_eq!(schema.id().to_string(), "ono.package/1");
/// assert_eq!(schema.identity(), ["provider".into(), "name".into()]);
/// ```
#[must_use]
#[allow(
    clippy::expect_used,
    reason = "AGENTS.md section 16 admits `expect` in a provably unreachable state. `ono.package/1` is \
              embedded from docs/contracts/schemas/ at compile time and \
              crates/ono-value/tests/builtin_schemas.rs turns red the moment it is not."
)]
pub fn package_schema() -> Arc<Schema> {
    static SCHEMA: OnceLock<Arc<Schema>> = OnceLock::new();
    Arc::clone(SCHEMA.get_or_init(|| {
        builtin_schemas()
            .get(&SchemaId::new("ono.package", 1))
            .expect("ono.package/1 is one of the schemas the shell ships")
    }))
}

/// The dpkg family of tools, as found on `PATH`.
#[derive(Debug, Clone)]
pub(crate) struct Dpkg {
    pub(crate) dpkg_query: PathBuf,
    pub(crate) apt_cache: Option<PathBuf>,
    pub(crate) apt_get: Option<PathBuf>,
    pub(crate) apt_mark: Option<PathBuf>,
    pub(crate) apt_config: Option<PathBuf>,
}

/// The package provider: `ono.package/1` records from dpkg and apt.
///
/// ```
/// use ono_provider_api::Provider;
/// use ono_provider_linux::PackageProvider;
///
/// let provider = PackageProvider::with_path(Some("/nonexistent".into()));
/// let reason = provider.availability().reason().map(str::to_owned);
/// assert!(reason.is_some_and(|reason| reason.contains("dpkg")));
/// ```
#[derive(Debug)]
pub struct PackageProvider {
    manager: Option<Dpkg>,
    /// The last `apt-get update`, for the results of one pipeline that follow it (ADR-0565).
    runs: Mutex<Option<AptRun>>,
}

impl Default for PackageProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl PackageProvider {
    /// A provider over the managers found on this process's `PATH`.
    #[must_use]
    pub fn new() -> Self {
        Self::with_path(std::env::var_os("PATH"))
    }

    /// A provider over the managers found on `path`, or none.
    #[must_use]
    pub fn with_path(path: Option<OsString>) -> Self {
        let find = |name: &str| on_path(path.as_ref(), name);
        let manager = find("dpkg-query").map(|dpkg_query| Dpkg {
            dpkg_query,
            apt_cache: find("apt-cache"),
            apt_get: find("apt-get"),
            apt_mark: find("apt-mark"),
            apt_config: find("apt-config"),
        });
        Self {
            manager,
            runs: Mutex::new(None),
        }
    }

    /// The `package-source` records: apt's sources, from `apt-get indextargets`.
    fn snapshot_sources(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let dpkg = self.manager()?;
        let plan = SourcePlan::of(query);
        let limit = query.max();
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                let sources = match list_sources(&dpkg).await {
                    Ok(sources) => sources,
                    Err(error) => {
                        let _ = sink.fail(error).await;
                        return;
                    }
                };
                let mut emitted = 0usize;
                for source in sources {
                    if limit.is_some_and(|limit| emitted >= limit) {
                        return;
                    }
                    if plan.named.as_ref().is_some_and(|named| named != &source.id) {
                        continue;
                    }
                    let record = match package_sources::source_record(
                        &source,
                        DPKG,
                        PACKAGE_PROVIDER_ID,
                        "apt-get update --print-uris",
                    ) {
                        Ok(record) => record,
                        Err(error) => {
                            let _ = sink.fail(error).await;
                            return;
                        }
                    };
                    if !plan.keeps(&record) {
                        continue;
                    }
                    emitted += 1;
                    if sink.send(record.into_value()).await.is_err() {
                        return;
                    }
                }
            },
        ))
    }

    /// Resolves a `package-source` selector to the sources apt lists.
    async fn resolve_source(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        let query = Query::target(package_sources::TARGET).with(selector.clone());
        let collected = self.snapshot_sources(&query)?.collect().await;
        if let Some(error) = collected.errors().first()
            && collected.values().is_empty()
        {
            return Err(error.clone());
        }
        Ok(collected
            .values()
            .iter()
            .filter_map(|value| match value {
                Value::Record(record) => ObjectRef::of(record),
                _ => None,
            })
            .collect())
    }

    /// `refresh package-source`: `apt-get update`, and what it did to the named source's index.
    ///
    /// apt refreshes every source in one run. The results of one pipeline share that run
    /// (ADR-0565): the first result makes it, the ones that follow within
    /// [`package_sources::APT_RUN_WINDOW`] read their own index against what it was before.
    async fn refresh_source(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        let dpkg = self.manager()?;
        let id = package_sources::source_id(action.target())?.to_owned();
        if action.is_dry_run() {
            return Ok(ActionOutcome::skipped(
                action,
                format!("would run `apt-get update` to refresh `{id}`"),
            ));
        }
        // Spec §17.2: elevation is explicit. apt's lists are root's, and the outcome of asking
        // apt-get as anyone else is known before it runs (ADR-0115 §5).
        let uid = nix::unistd::geteuid();
        if !uid.is_root() {
            return Ok(ActionOutcome::failed(
                action,
                ErrorValue::new(
                    ErrorCode::IoPermissionDenied,
                    format!("to refresh `{id}` needs root, and this shell runs as uid {uid}"),
                )
                .with_help(
                    "run it as root — `sudo ono -c '…'` — after `explain` has shown you the                      privilege it needs (spec §17.2)",
                ),
            ));
        }
        let Some(apt_get) = dpkg.apt_get.clone() else {
            return Ok(ActionOutcome::failed(
                action,
                ErrorValue::new(
                    ErrorCode::ProviderUnsupported,
                    "no `apt-get` is on PATH, so the index cannot be refreshed here",
                ),
            ));
        };
        let sources = list_sources(&dpkg).await?;
        let Some(source) = sources.iter().find(|source| source.id == id) else {
            return Ok(ActionOutcome::failed(
                action,
                ErrorValue::new(
                    ErrorCode::ResolveTargetNotFound,
                    format!("apt reads no source `{id}`"),
                )
                .with_help("`get package-source` lists the sources apt reads, by id"),
            ));
        };
        let mut runs = self.runs.lock().await;
        let before = match runs.as_ref().and_then(|run| run.before(&id)) {
            Some(before) => before,
            None => {
                let before_all: BTreeMap<String, Option<Timestamp>> = sources
                    .iter()
                    .map(|source| (source.id.clone(), source.refreshed()))
                    .collect();
                let answer = run(&apt_get, &["update".to_owned()]).await?;
                if answer.status != Some(0) {
                    return Ok(ActionOutcome::failed(
                        action,
                        manager_failure(&apt_get, &answer),
                    ));
                }
                let before = before_all.get(&id).copied().flatten();
                *runs = Some(AptRun::new(before_all));
                before
            }
        };
        // What changed is the index's to say, not apt's prose: its time before against after.
        let changed = source.refreshed() != before;
        Ok(ActionOutcome::succeeded(action, changed))
    }

    pub(crate) fn manager(&self) -> Result<Dpkg, ErrorValue> {
        self.manager.clone().ok_or_else(|| {
            ErrorValue::new(ErrorCode::ProviderUnavailable, unavailable_reason()).with_help(
                "`package` needs a package manager the shell can ask in a machine format. \
                 Having none is not the same as having no packages, so this is a refusal to \
                 answer rather than an empty answer.",
            )
        })
    }
}

/// The first `name` on `path`, which is how every manager is discovered: a package manager at an
/// absolute path is an assumption about a distribution, and `PATH` is the answer the machine
/// itself gives (ADR-0115).
pub(crate) fn on_path(path: Option<&OsString>, name: &str) -> Option<PathBuf> {
    path.and_then(|path| {
        std::env::split_paths(path)
            .map(|directory| directory.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn unavailable_reason() -> String {
    // What this provider looked for, and only that: the rpm database has a provider of its own
    // (ADR-0422), and the registry states both refusals when neither is here.
    "no supported package manager is on PATH: looked for `dpkg-query` (dpkg/apt)".to_owned()
}

/// One line of the `dpkg-query -W -f` listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Listed {
    pub(crate) name: String,
    pub(crate) version: Option<String>,
    pub(crate) installed: bool,
}

/// Whether `name` is a package name dpkg would accept: lower-case letters, digits, `+`, `-`,
/// `.`, at least two characters, starting with a letter or digit (deb-control(5)).
pub(crate) fn is_package_name(name: &str) -> bool {
    let mut chars = name.chars();
    let first = chars.next();
    first.is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name.len() >= 2
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '+' | '-' | '.'))
}

/// Reads the listing `dpkg-query -W -f` printed.
///
/// # Errors
///
/// `provider.schema_violation` when the bytes are not the declared format: not text, or a line
/// that is not `Package\tVersion\tStatus` with a well-formed name.
pub(crate) fn parse_listing(bytes: &[u8]) -> Result<Vec<Listed>, ErrorValue> {
    let violation = |detail: String| {
        ErrorValue::new(
            ErrorCode::ProviderSchemaViolation,
            format!("`dpkg-query -W -f` did not answer in its machine format: {detail}"),
        )
        .with_help(
            "the provider asked for `${Package}\\t${Version}\\t${Status}` per line and read \
             something else; this is a defect at the package-manager boundary, not in your \
             pipeline, and no record was made from it",
        )
    };
    let text = std::str::from_utf8(bytes)
        .map_err(|_| violation("the output is not UTF-8 text".to_owned()))?;
    let mut listed = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let [name, version, status] = fields[..] else {
            return Err(violation(format!(
                "line {} has {} tab-separated fields, not 3",
                index + 1,
                fields.len()
            )));
        };
        if !is_package_name(name) {
            return Err(violation(format!(
                "line {} names {name:?}, which is not a package name",
                index + 1
            )));
        }
        // The status is `want flag status`; `install ok installed` is the installed state and
        // every other status word — `config-files`, `not-installed`, `half-installed`,
        // `unpacked` — is a package that is not (yet, or any more) installed.
        let installed = status.split_whitespace().nth(2) == Some("installed");
        listed.push(Listed {
            name: name.to_owned(),
            version: Some(version).filter(|v| !v.is_empty()).map(str::to_owned),
            installed,
        });
    }
    Ok(listed)
}

/// Reads the hits `apt-cache search` printed: `name - description` per line.
///
/// # Errors
///
/// `provider.schema_violation` when a line is not in that shape.
pub(crate) fn parse_search(bytes: &[u8]) -> Result<Vec<(String, String)>, ErrorValue> {
    let violation = |detail: String| {
        ErrorValue::new(
            ErrorCode::ProviderSchemaViolation,
            format!("`apt-cache search` did not answer in its documented format: {detail}"),
        )
    };
    let text = std::str::from_utf8(bytes)
        .map_err(|_| violation("the output is not UTF-8 text".to_owned()))?;
    let mut hits = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let Some((name, description)) = line.split_once(" - ") else {
            return Err(violation(format!(
                "line {} is not `name - description`",
                index + 1
            )));
        };
        if !is_package_name(name) {
            return Err(violation(format!(
                "line {} names {name:?}, which is not a package name",
                index + 1
            )));
        }
        hits.push((name.to_owned(), description.to_owned()));
    }
    Ok(hits)
}

/// The `ono.package/1` record for one package.
///
/// `database` is the manager that answered — the first half of the record's identity — and
/// `provider_id` the provider that asked it, which is what provenance names (ADR-0422).
pub(crate) fn package_record(
    name: &str,
    version: Option<&str>,
    installed: Option<bool>,
    description: Option<&str>,
    database: &str,
    provider_id: &str,
    source: &str,
) -> Result<RecordValue, ErrorValue> {
    let schema = package_schema();
    let provenance = Provenance::local(provider_id, schema.id().clone())
        .from_source(source)
        .observed_at(Timestamp::now());
    Ok(RecordValue::builder(schema, provenance)
        .set("name", Value::string(name))?
        .set("version", version.map_or(Value::Null, Value::string))?
        .set("installed", installed.map_or(Value::Null, Value::Bool))?
        .set(
            "description",
            description.map_or(Value::Null, Value::string),
        )?
        .set("provider", Value::string(database))?
        .build())
}

/// What a manager said when run: its status and both streams.
pub(crate) struct Answer {
    pub(crate) status: Option<i32>,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

/// Runs one manager invocation to completion, in the C locale so its answers are stable.
pub(crate) async fn run(program: &Path, arguments: &[String]) -> Result<Answer, ErrorValue> {
    let output = tokio::process::Command::new(program)
        .args(arguments)
        .env("LC_ALL", "C")
        .env("DEBIAN_FRONTEND", "noninteractive")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|error| {
            ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!("`{}` could not be run: {error}", program.display()),
            )
        })?;
    Ok(Answer {
        status: output.status.code(),
        stdout: output.stdout,
        stderr: output.stderr,
    })
}

/// The sources apt reads: the index files `apt-get update --print-uris` would fetch, grouped
/// by repository root, suite and component (apt-get(8)); their destination under
/// `Dir::State::lists` as `apt-config shell` reports it; and the labels `apt-get indextargets`
/// knows for the indexes that have been fetched already (ADR-0565).
pub(crate) async fn list_sources(dpkg: &Dpkg) -> Result<Vec<Source>, ErrorValue> {
    let Some(apt_get) = &dpkg.apt_get else {
        return Err(ErrorValue::new(
            ErrorCode::ProviderUnsupported,
            "no `apt-get` is on PATH, so the sources apt reads cannot be listed",
        )
        .with_help("`get package` still lists what dpkg has installed"));
    };
    let arguments = vec!["update".to_owned(), "--print-uris".to_owned()];
    let answer = run(apt_get, &arguments).await?;
    if answer.status != Some(0) {
        return Err(ErrorValue::new(
            ErrorCode::ProviderUnavailable,
            format!(
                "`apt-get update --print-uris` failed: {}",
                String::from_utf8_lossy(&answer.stderr).trim()
            ),
        ));
    }
    let indexes = package_sources::parse_print_uris(&answer.stdout)?;

    let mut lists_dir = PathBuf::from(package_sources::APT_LISTS_DIR);
    if let Some(apt_config) = &dpkg.apt_config {
        let arguments = vec![
            "shell".to_owned(),
            "LISTS".to_owned(),
            "Dir::State::lists/f".to_owned(),
        ];
        if let Ok(answer) = run(apt_config, &arguments).await
            && answer.status == Some(0)
            && let Some(directory) = package_sources::parse_apt_lists_dir(&answer.stdout)
        {
            lists_dir = directory;
        }
    }

    // Labels are a courtesy: `indextargets` answers only for indexes that have been fetched,
    // and a source is a source before its first update.
    let arguments = vec![
        "indextargets".to_owned(),
        "--format".to_owned(),
        package_sources::APT_INDEXTARGETS_FORMAT.to_owned(),
    ];
    let labelled = match run(apt_get, &arguments).await {
        Ok(answer) if answer.status == Some(0) => {
            package_sources::parse_indextargets(&answer.stdout).unwrap_or_default()
        }
        _ => Vec::new(),
    };
    Ok(package_sources::apt_sources(
        &indexes, &lists_dir, &labelled,
    ))
}

/// The installed set, or the named packages' entries, from `dpkg-query -W -f`.
///
/// dpkg-query exits 1 when one of the names it was given is unknown, and still prints the
/// others; that is the ordinary answer to "is this installed?", not a failure.
pub(crate) async fn installed(dpkg: &Dpkg, names: &[String]) -> Result<Vec<Listed>, ErrorValue> {
    let mut arguments = vec!["-W".to_owned(), "-f".to_owned(), LISTING_FORMAT.to_owned()];
    arguments.extend(names.iter().cloned());
    let answer = run(&dpkg.dpkg_query, &arguments).await?;
    match answer.status {
        Some(0 | 1) => parse_listing(&answer.stdout),
        status => Err(ErrorValue::new(
            ErrorCode::ProviderUnavailable,
            format!(
                "`dpkg-query -W` failed with status {}: {}",
                status.map_or("signal".to_owned(), |code| code.to_string()),
                String::from_utf8_lossy(&answer.stderr).trim()
            ),
        )),
    }
}

/// How a query is answered. Shared by every package provider: the selectors of
/// `docs/contracts/commands/package.yaml` mean the same thing whichever database answers them.
pub(crate) struct Plan {
    /// One package asked for by name.
    pub(crate) named: Option<String>,
    /// A repository search.
    pub(crate) search: Option<String>,
    /// `--installed`, when written.
    pub(crate) installed: Option<bool>,
    remaining: Vec<Selector>,
}

impl Plan {
    pub(crate) fn of(query: &Query) -> Self {
        let mut plan = Self {
            named: None,
            search: None,
            installed: match query.option_value("installed") {
                Some(Value::Bool(wanted)) => Some(*wanted),
                _ => None,
            },
            remaining: Vec::new(),
        };
        for selector in query.selectors() {
            // A name is pushed down — dpkg-query is asked for that one package — and still
            // applied to what comes back: a manager asked for one name and answering with its
            // whole listing must not turn the selector into a no-op.
            match selector {
                Selector::Field { name, value } if name == "name" && plan.named.is_none() => {
                    if let Ok(text) = value.as_str() {
                        plan.named = Some(text.to_owned());
                    }
                    plan.remaining.push(selector.clone());
                }
                Selector::Field { name, value } if name == "query" && plan.search.is_none() => {
                    match value.as_str() {
                        Ok(text) => plan.search = Some(text.to_owned()),
                        Err(_) => plan.remaining.push(selector.clone()),
                    }
                }
                Selector::Identity(id) if plan.named.is_none() => {
                    if let Ok(name) = package_name(id) {
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
        let installed_matches = match self.installed {
            Some(wanted) => record.get("installed") == Some(&Value::Bool(wanted)),
            None => true,
        };
        installed_matches
            && self
                .remaining
                .iter()
                .all(|selector| selector.matches(record))
    }
}

/// The package name an identity refers to: `ono.package/1` is `provider + name`, and a creating
/// verb names a package by its selector alone (ADR-0098 §1), so a one-value identity is the
/// name without a provider.
pub(crate) fn package_name(id: &ObjectId) -> Result<&str, ErrorValue> {
    let expected = SchemaId::new("ono.package", 1);
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
                format!("`{id}` does not name a package"),
            )
            .with_help(
                "a package action needs an `ono.package/1` identity of `provider` and `name`",
            )
        })
}

/// The records a plan produces, in the order the manager listed them.
async fn answer(dpkg: &Dpkg, plan: &Plan) -> Result<Vec<RecordValue>, ErrorValue> {
    if let Some(term) = &plan.search {
        let Some(apt_cache) = &dpkg.apt_cache else {
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                "no `apt-cache` is on PATH, so the repositories cannot be searched",
            )
            .with_help("`get package` still lists what dpkg has installed"));
        };
        let arguments = vec!["search".to_owned(), term.clone()];
        let found = run(apt_cache, &arguments).await?;
        if found.status != Some(0) {
            return Err(ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                format!(
                    "`apt-cache search` failed: {}",
                    String::from_utf8_lossy(&found.stderr).trim()
                ),
            ));
        }
        let hits = parse_search(&found.stdout)?;
        // What dpkg has of them decides `installed` and `version`; a hit dpkg does not list
        // is known to the repositories and not installed here.
        let names: Vec<String> = hits.iter().map(|(name, _)| name.clone()).collect();
        let listed = if names.is_empty() {
            Vec::new()
        } else {
            installed(dpkg, &names).await?
        };
        return hits
            .iter()
            .map(|(name, description)| {
                let entry = listed.iter().find(|entry| &entry.name == name);
                package_record(
                    name,
                    entry.and_then(|entry| entry.version.as_deref()),
                    Some(entry.is_some_and(|entry| entry.installed)),
                    Some(description),
                    DPKG,
                    PACKAGE_PROVIDER_ID,
                    "apt-cache search",
                )
            })
            .collect();
    }

    let names: Vec<String> = plan.named.iter().cloned().collect();
    if let Some(name) = &plan.named
        && !is_package_name(name)
    {
        // dpkg would only complain; an impossible name has no package and needs no process.
        return Ok(Vec::new());
    }
    installed(dpkg, &names)
        .await?
        .iter()
        .map(|entry| {
            package_record(
                &entry.name,
                entry.version.as_deref(),
                Some(entry.installed),
                None,
                DPKG,
                PACKAGE_PROVIDER_ID,
                "dpkg-query -W -f",
            )
        })
        .collect()
}

/// The manager invocations a `package` action asks for (ADR-0115 §5).
pub(crate) struct Mutation {
    /// Each `(program, arguments)`, run in order; the action succeeds when all did.
    pub(crate) commands: Vec<(PathBuf, Vec<String>)>,
    /// What is being asked, for a dry run's answer and the refusal's wording.
    pub(crate) described: String,
    /// Whether the outcome can be read from the database's version afterwards; a hold cannot.
    pub(crate) versioned: bool,
}

impl Mutation {
    fn of(action: &Action, dpkg: &Dpkg, name: &str) -> Result<Self, ErrorValue> {
        let unsupported = |message: String, help: &str| {
            Err(ErrorValue::new(ErrorCode::ProviderUnsupported, message).with_help(help))
        };
        let apt_get = || {
            dpkg.apt_get.clone().ok_or_else(|| {
                ErrorValue::new(
                    ErrorCode::ProviderUnsupported,
                    "no `apt-get` is on PATH, so packages cannot be changed here",
                )
                .with_help("`get package` still lists what dpkg has installed")
            })
        };
        let text = |option: &str| match action.argument(option) {
            Some(Value::String(text)) => Some(text.to_string()),
            Some(other) => Some(other.to_string()),
            None => None,
        };
        let flag = |option: &str| match action.argument(option) {
            Some(Value::Bool(wanted)) => Some(*wanted),
            _ => None,
        };
        match action.operation() {
            "add" => {
                let spec = match text("version") {
                    Some(version) => format!("{name}={version}"),
                    None => name.to_owned(),
                };
                Ok(Self {
                    commands: vec![(
                        apt_get()?,
                        vec!["install".to_owned(), "-y".to_owned(), spec.clone()],
                    )],
                    described: format!("install `{spec}`"),
                    versioned: true,
                })
            }
            "remove" => {
                let verb = if flag("purge") == Some(true) {
                    "purge"
                } else {
                    "remove"
                };
                Ok(Self {
                    commands: vec![(
                        apt_get()?,
                        vec![verb.to_owned(), "-y".to_owned(), name.to_owned()],
                    )],
                    described: format!("{verb} `{name}`"),
                    versioned: true,
                })
            }
            "set" => {
                let mut commands = Vec::new();
                let mut described = Vec::new();
                let mut versioned = false;
                if let Some(version) = text("version") {
                    commands.push((
                        apt_get()?,
                        vec![
                            "install".to_owned(),
                            "-y".to_owned(),
                            format!("{name}={version}"),
                        ],
                    ));
                    described.push(format!("move `{name}` to {version}"));
                    versioned = true;
                }
                if let Some(hold) = flag("hold") {
                    let Some(apt_mark) = dpkg.apt_mark.clone() else {
                        return unsupported(
                            "no `apt-mark` is on PATH, so a package cannot be held".to_owned(),
                            "`--version` still works through apt-get",
                        );
                    };
                    let mark = if hold { "hold" } else { "unhold" };
                    commands.push((apt_mark, vec![mark.to_owned(), name.to_owned()]));
                    described.push(format!("{mark} `{name}`"));
                }
                if commands.is_empty() {
                    return unsupported(
                        "the package provider changes `version` and `hold`, and `set` named \
                         neither"
                            .to_owned(),
                        "write `--version 1.24.0` or `--hold true`",
                    );
                }
                Ok(Self {
                    commands,
                    described: described.join(" and "),
                    versioned,
                })
            }
            other => unsupported(
                format!("the package provider has no operation `{other}`"),
                "it can add and remove a package, and set `--version` and `--hold`",
            ),
        }
    }
}

/// The version dpkg has installed for `name`, or `None`.
async fn installed_version(dpkg: &Dpkg, name: &str) -> Result<Option<String>, ErrorValue> {
    Ok(installed(dpkg, &[name.to_owned()])
        .await?
        .into_iter()
        .find(|entry| entry.name == name && entry.installed)
        .and_then(|entry| entry.version))
}

/// The error a failed manager run is: apt's "cannot locate" is `io.not_found`, anything else —
/// including the words dnf and zypper use for the same thing — is the manager's exit with its own
/// message.
pub(crate) fn manager_failure(program: &Path, answer: &Answer) -> ErrorValue {
    let said = String::from_utf8_lossy(&answer.stderr).trim().to_owned();
    let code = if said.contains("Unable to locate package")
        || said.contains("is not installed, so not removed")
    {
        ErrorCode::IoNotFound
    } else {
        ErrorCode::ExternalExitNonzero
    };
    ErrorValue::new(
        code,
        format!(
            "`{}` exited with status {}: {said}",
            program.display(),
            answer
                .status
                .map_or("signal".to_owned(), |status| status.to_string())
        ),
    )
}

#[async_trait::async_trait]
impl Provider for PackageProvider {
    fn identity_token(&self) -> Option<&str> {
        Some(DPKG)
    }

    fn id(&self) -> &str {
        PACKAGE_PROVIDER_ID
    }

    fn targets(&self) -> &[&str] {
        &["package", package_sources::TARGET]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        vec![package_schema(), package_sources::package_source_schema()]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("package.list", Risk::Read),
            Capability::new("package.search", Risk::Read),
            // `docs/contracts/capabilities.yaml` gives `package.manage` elevation `required`: dpkg's
            // database is root's, and the provider says so before it runs anything
            // (ADR-0115 §5).
            Capability::new("package.manage", Risk::Mutate).needing_elevation(),
            Capability::new("package-source.list", Risk::Read),
            // apt's lists are root's too (ADR-0565).
            Capability::new("package-source.refresh", Risk::Mutate).needing_elevation(),
        ]
    }

    fn availability(&self) -> Availability {
        match &self.manager {
            Some(_) => Availability::Available,
            None => Availability::unavailable(unavailable_reason()),
        }
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        if query.target_name() == package_sources::TARGET {
            return self.snapshot_sources(query);
        }
        let dpkg = self.manager()?;
        let plan = Plan::of(query);
        let limit = query.max();
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                let records = match answer(&dpkg, &plan).await {
                    Ok(records) => records,
                    Err(error) => {
                        let _ = sink.fail(error).await;
                        return;
                    }
                };
                let mut emitted = 0usize;
                for record in records {
                    if limit.is_some_and(|limit| emitted >= limit) {
                        return;
                    }
                    if !plan.keeps(&record) {
                        continue;
                    }
                    emitted += 1;
                    if sink.send(record.into_value()).await.is_err() {
                        return;
                    }
                }
            },
        ))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        if package_sources::is_source_selector(selector) {
            return self.resolve_source(selector).await;
        }
        let query = Query::target("package").with(selector.clone());
        let collected = self.snapshot(&query)?.collect().await;
        if let Some(error) = collected.errors().first()
            && collected.values().is_empty()
        {
            return Err(error.clone());
        }
        let found: Vec<ObjectRef> = collected
            .values()
            .iter()
            .filter_map(|value| match value {
                Value::Record(record) => ObjectRef::of(record),
                _ => None,
            })
            .collect();
        if !found.is_empty() {
            return Ok(found);
        }
        // A well-formed name dpkg does not list is still a package identity — the one `add
        // package <name>` is about to install. It resolves to a package that is known not to be
        // installed; whether the repositories carry it is the manager's to say when asked.
        if let Selector::Field { name, value } = selector
            && name == "name"
            && let Ok(text) = value.as_str()
            && is_package_name(text)
        {
            let record = package_record(
                text,
                None,
                Some(false),
                None,
                DPKG,
                PACKAGE_PROVIDER_ID,
                "dpkg-query -W -f",
            )?;
            return Ok(ObjectRef::of(&record).into_iter().collect());
        }
        Ok(Vec::new())
    }
    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        if action.target_name() == package_sources::TARGET {
            return self.refresh_source(action).await;
        }
        let dpkg = self.manager()?;
        let name = package_name(action.target())?.to_owned();
        let mutation = Mutation::of(action, &dpkg, &name)?;
        if action.is_dry_run() {
            return Ok(ActionOutcome::skipped(
                action,
                format!("would {}", mutation.described),
            ));
        }
        // Spec §17.2: elevation is explicit. dpkg's database is root's, and the outcome of
        // asking apt-get as anyone else is known before it runs (ADR-0115 §5).
        let uid = nix::unistd::geteuid();
        if !uid.is_root() {
            return Ok(ActionOutcome::failed(
                action,
                ErrorValue::new(
                    ErrorCode::IoPermissionDenied,
                    format!(
                        "to {} needs root, and this shell runs as uid {uid}",
                        mutation.described
                    ),
                )
                .with_help(
                    "run it as root — `sudo ono -c '…'` — after `explain` has shown you the \
                     privilege it needs (spec §17.2)",
                ),
            ));
        }
        let before = installed_version(&dpkg, &name).await?;
        for (program, arguments) in &mutation.commands {
            let answer = run(program, arguments).await?;
            if answer.status != Some(0) {
                return Ok(ActionOutcome::failed(
                    action,
                    manager_failure(program, &answer),
                ));
            }
        }
        // What changed is dpkg's to say, not apt's prose: the version before against after.
        let changed = if mutation.versioned {
            installed_version(&dpkg, &name).await? != before
        } else {
            true
        };
        Ok(ActionOutcome::succeeded(action, changed))
    }
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
    fn should_read_the_tab_separated_listing() {
        let listed = parse_listing(
            b"curl\t8.5.0-2\tinstall ok installed\nold\t\tdeinstall ok config-files\n",
        )
        .unwrap();
        assert_eq!(
            listed,
            [
                Listed {
                    name: "curl".into(),
                    version: Some("8.5.0-2".into()),
                    installed: true
                },
                Listed {
                    name: "old".into(),
                    version: None,
                    installed: false
                },
            ]
        );
    }

    #[test]
    fn should_refuse_a_listing_that_is_not_in_the_machine_format() {
        for garbage in [
            &b"\xff\xfe not a listing at all ~~~\n"[..],
            b"curl 8.5.0-2 install ok installed\n",
            b"Not A Name\t1\tinstall ok installed\n",
        ] {
            let error = parse_listing(garbage).unwrap_err();
            assert_eq!(
                error.code(),
                ErrorCode::ProviderSchemaViolation,
                "{garbage:?}"
            );
        }
    }

    #[test]
    fn should_read_search_hits_as_name_and_description() {
        let hits =
            parse_search(b"curl - command line tool\nlibcurl4 - easy-to-use - library\n").unwrap();
        assert_eq!(hits[0], ("curl".into(), "command line tool".into()));
        assert_eq!(hits[1], ("libcurl4".into(), "easy-to-use - library".into()));
        assert!(parse_search(b"no separator here\n").is_err());
    }
}
