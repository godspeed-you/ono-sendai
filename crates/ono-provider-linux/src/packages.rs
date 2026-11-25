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
    Selector, TemporalCapabilities,
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

/// Whether apt could read `name` as something other than the package of that name: a trailing `-`
/// ("remove") or `+` ("install") and a `.` (a regular expression) are instructions to apt only
/// where no package of exactly that name exists (`g++` and `python3.11` are packages). Such a name
/// is let through once the package database knows it exactly ([`exactly_known`]).
pub(crate) fn needs_exact_lookup(name: &str) -> bool {
    name.ends_with(['-', '+']) || name.contains('.')
}

/// Whether apt's package database knows a package of exactly `name` (review finding SEC-C-1).
///
/// `apt-cache pkgnames -- <name>` lists the names that begin with `name`, read-only and without
/// pattern matching; an exact line is the answer. `name` has passed [`is_package_name`] before it
/// gets here, and travels after `--`, so the lookup cannot be read as an option either.
async fn exactly_known(dpkg: &Dpkg, name: &str) -> Result<bool, ErrorValue> {
    let Some(apt_cache) = &dpkg.apt_cache else {
        return Ok(false);
    };
    let answer = run(
        apt_cache,
        &["pkgnames".to_owned(), "--".to_owned(), name.to_owned()],
    )
    .await?;
    Ok(answer.status == Some(0)
        && String::from_utf8_lossy(&answer.stdout)
            .lines()
            .any(|line| line.trim() == name))
}

/// Whether `version` is a version string and nothing else — a package manager is handed it
/// appended to a name, and a glob or a space there would be a different request (SEC-C-1).
pub(crate) fn is_version_text(version: &str) -> bool {
    let mut chars = version.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '~' | ':' | '-' | '_' | '^')
        })
}

/// The refusal of a name or a version a package manager would read as something other than the
/// one package the action is about — an option, a pattern, an instruction (SEC-C-1, ADR-1084).
///
/// Raised before any process starts, for every way a name arrives: a piped record's identity,
/// the selector a creating verb was given, a name `resolve` passed through.
pub(crate) fn not_one_package(what: &str, text: &str, family: &str) -> ErrorValue {
    let shown: String = text.chars().take(64).collect();
    ErrorValue::new(
        ErrorCode::TypeMismatch,
        format!("{shown:?} is not a {what} {family} reads as one package"),
    )
    .with_help(
        "a package is named by its name alone — letters, digits and the punctuation its \
         manager allows — and Ono hands it to the manager only after `--`; patterns, options \
         and globs are refused rather than passed on",
    )
}

/// Refuses an action's name and version before anything runs, where either is not one package.
fn check_apt_request(action: &Action, name: &str) -> Result<(), ErrorValue> {
    let archive = action.operation() == "add"
        && name.starts_with('/')
        && name.ends_with(".deb")
        && !name.chars().any(char::is_control);
    if !archive && !is_package_name(name) {
        return Err(not_one_package("package name", name, "apt"));
    }
    if let Some(Value::String(version)) = action.argument("version")
        && !is_version_text(version)
    {
        return Err(not_one_package("version", version, "apt"));
    }
    Ok(())
}

/// Whether a package request's name and version are one package and one version to every
/// manager this crate drives (review finding R2C-4).
///
/// The plan applies it before anything is sealed, so the plan and the direct command refuse the
/// same requests: a name that is neither a deb-control(5) nor an rpm name (anything beginning with
/// `-`, globs, paths, provides) and a version that is not a version (`*`, `-oX`, a space). The
/// family-specific rules — apt's exact lookup for a trailing `-`/`+` or a `.`, the narrower deb
/// grammar on a dpkg host — are applied where the family is known, by the provider before any
/// process starts.
///
/// # Errors
///
/// `type.mismatch` (E0201) for either.
pub fn check_package_arguments(name: &str, version: Option<&Value>) -> Result<(), ErrorValue> {
    // A local archive is a path the manager is handed after `--` (ADR-0957).
    let archive = (name.starts_with('/') || name.starts_with("./") || name.starts_with("../"))
        && (name.ends_with(".deb") || name.ends_with(".rpm"))
        && !name.chars().any(char::is_control);
    if !archive && !is_package_name(name) && !crate::packages_rpm::is_rpm_package_name(name) {
        return Err(not_one_package("package name", name, "a package manager"));
    }
    if let Some(version) = version {
        let text = match version {
            Value::String(text) => text.to_string(),
            other => other.to_string(),
        };
        if !is_version_text(&text) {
            return Err(not_one_package("version", &text, "a package manager"));
        }
    }
    Ok(())
}

/// Whether a `set package` asks for one transition (ADR-1084, review findings COR-C1, COR-C2).
///
/// `--latest` and `--version` name two destinations for one transition, and a `set` that names
/// neither a destination nor a hold — `--latest false` alone — asks for nothing. Both are a usage
/// error, refused with `type.mismatch` (E0201) wherever the request is made: by the plan before it
/// is sealed, and by the provider before any manager runs. One function, so the plan and the
/// direct command cannot disagree about what a transition is.
///
/// # Errors
///
/// `type.mismatch` for either.
pub fn check_set_request(
    version: Option<&Value>,
    latest: Option<&Value>,
    hold: Option<&Value>,
) -> Result<(), ErrorValue> {
    let latest = matches!(latest, Some(Value::Bool(true)));
    if latest && version.is_some() {
        return Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            "`--latest` and `--version` name two destinations for one transition",
        )
        .with_help("write `--latest` to move to the newest version, or `--version` to name one"));
    }
    if !latest && version.is_none() && !matches!(hold, Some(Value::Bool(_))) {
        return Err(ErrorValue::new(
            ErrorCode::TypeMismatch,
            "`set package` names no transition: neither a version, `--latest` nor a hold",
        )
        .with_help("write `--version 1.24.0`, `--latest` or `--hold true`"));
    }
    Ok(())
}

/// The refusal of `set --latest` for a package that is not installed (review finding COR-C5).
///
/// `apt-get install --only-upgrade` answers "skipping, it is not installed" with status 0, and an
/// action that reported that as success would say a package had moved to its newest version that
/// is not there at all.
pub(crate) fn not_installed(name: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::IoNotFound,
        format!("`{name}` is not installed, so there is no version to move to the newest"),
    )
    .with_help("`add package <name>` installs it; `--latest` moves an installed package")
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
    run_in(program, arguments, None).await
}

/// [`run`], in `directory` when one is given.
pub(crate) async fn run_in(
    program: &Path,
    arguments: &[String],
    directory: Option<&Path>,
) -> Result<Answer, ErrorValue> {
    let mut command = tokio::process::Command::new(program);
    if let Some(directory) = directory {
        command.current_dir(directory);
    }
    let child = command
        .args(arguments)
        .env("LC_ALL", "C")
        .env("DEBIAN_FRONTEND", "noninteractive")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        // A group of its own, so the package tools it starts are reached as well.
        .process_group(0)
        .spawn();
    // Recorded while it runs, so a shell ending on a signal ends it too (ADR-0959).
    let output = async {
        let child = child?;
        let recorded = ono_core::children::Recorded::group_of(child.id());
        let output = child.wait_with_output().await;
        drop(recorded);
        output
    }
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
    let mut arguments = vec![
        "-W".to_owned(),
        "-f".to_owned(),
        LISTING_FORMAT.to_owned(),
        "--".to_owned(),
    ];
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

/// The apt option that turns off apt's reading of a package argument as a regular expression or
/// a glob when no package has that name (`xz.` is every package with `xz` in its name otherwise):
/// patterns only, and only the `?pattern` syntax (review finding SEC-C-1). Honoured by this host's
/// apt 2.8.4; an apt that does not know the key ignores it, as it ignores every unknown `-o` key,
/// and [`exactly_known`] is the defence for that apt.
pub(crate) const APT_PATTERN_ONLY: [&str; 2] = ["-o", "APT::Cmd::Pattern-Only=true"];

impl Mutation {
    /// The invocations an action asks for, each with [`APT_PATTERN_ONLY`] before its `--`.
    fn of(action: &Action, dpkg: &Dpkg, name: &str) -> Result<Self, ErrorValue> {
        let mut mutation = Self::unguarded(action, dpkg, name)?;
        for (_, arguments) in &mut mutation.commands {
            if let Some(at) = arguments.iter().position(|argument| argument == "--") {
                for (offset, option) in APT_PATTERN_ONLY.iter().enumerate() {
                    arguments.insert(at + offset, (*option).to_owned());
                }
            }
        }
        Ok(mutation)
    }

    /// [`Mutation::of`]'s invocations, before the pattern guard.
    fn unguarded(action: &Action, dpkg: &Dpkg, name: &str) -> Result<Self, ErrorValue> {
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
                        vec![
                            "install".to_owned(),
                            "-y".to_owned(),
                            "--no-remove".to_owned(),
                            "--".to_owned(),
                            spec.clone(),
                        ],
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
                        vec![
                            verb.to_owned(),
                            "-y".to_owned(),
                            "--".to_owned(),
                            name.to_owned(),
                        ],
                    )],
                    described: format!("{verb} `{name}`"),
                    versioned: true,
                })
            }
            "set" => {
                check_set_request(
                    action.argument("version"),
                    action.argument("latest"),
                    action.argument("hold"),
                )?;
                let mut commands = Vec::new();
                let mut described = Vec::new();
                let mut versioned = false;
                // v0.6 §3.1's `update package` (ADR-1084): the newest version the repositories
                // offer, for a package that is installed. `--only-upgrade` is apt-get's own way
                // of never installing one that is not.
                if flag("latest") == Some(true) {
                    commands.push((
                        apt_get()?,
                        vec![
                            "install".to_owned(),
                            "--only-upgrade".to_owned(),
                            "-y".to_owned(),
                            "--no-remove".to_owned(),
                            "--".to_owned(),
                            name.to_owned(),
                        ],
                    ));
                    described.push(format!(
                        "move `{name}` to the newest version its repositories offer"
                    ));
                    versioned = true;
                }
                if let Some(version) = text("version") {
                    commands.push((
                        apt_get()?,
                        vec![
                            "install".to_owned(),
                            "-y".to_owned(),
                            "--no-remove".to_owned(),
                            "--".to_owned(),
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
                    commands.push((
                        apt_mark,
                        vec![mark.to_owned(), "--".to_owned(), name.to_owned()],
                    ));
                    described.push(format!("{mark} `{name}`"));
                }
                if commands.is_empty() {
                    return unsupported(
                        "the package provider changes `version` and `hold`, and `set` named \
                         neither"
                            .to_owned(),
                        "write `--version 1.24.0`, `--latest` or `--hold true`",
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

    fn temporal(&self) -> TemporalCapabilities {
        // The package database as it is now. dpkg keeps an install log — `/var/log/dpkg.log` — and this
        // provider reads neither, so it claims no history: the log is a source somebody could
        // add, not one that is already answering (§21.1). A listing is a complete list at the
        // instant it was read, so it checkpoints, and a package manager announces nothing to
        // subscribe to.
        TemporalCapabilities {
            current_snapshot: true,
            live_events: false,
            historical_query: false,
            exhaustive_events: false,
            causal_tokens: false,
            checkpointable: true,
            retained_history: None,
        }
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
        let name = local_package(action, package_name(action.target())?);
        // SEC-C-1: nothing reaches a package manager that it would read as an option, a pattern
        // or an instruction — checked before any process starts, the root check included.
        check_apt_request(action, &name)?;
        let mutation = Mutation::of(action, &dpkg, &name)?;
        let archive = name.starts_with('/');
        let before = if archive {
            None
        } else {
            installed_version(&dpkg, &name).await?
        };
        // A trailing `-`/`+` or a `.` is apt's instruction or pattern only where no package of
        // exactly that name exists: an installed one, or one the package database knows exactly,
        // is that package; anything else is refused before any manager changes anything (SEC-C-1).
        if !archive
            && before.is_none()
            && needs_exact_lookup(&name)
            && !exactly_known(&dpkg, &name).await?
        {
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!(
                    "the package database knows no package called exactly `{name}`, and apt would \
                     read the name as an instruction or a pattern"
                ),
            )
            .with_help(
                "a trailing `-` means \"remove\" and `+` \"install\" to apt, and a `.` a \
                 pattern, wherever no package has exactly that name; name the package as \
                 `apt-cache pkgnames` lists it",
            ));
        }
        if !archive && before.is_none() {
            // COR-C5: an update of a package that is not there is refused, not "skipped" by apt
            // with status 0 and reported as success.
            if action.operation() == "set" && action.argument("latest") == Some(&Value::Bool(true))
            {
                return Err(not_installed(&name));
            }
        }
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
        for (program, arguments) in &mutation.commands {
            // The manager runs where the action's paths mean something: a job's directory
            // (ADR-0957), the process's otherwise.
            let answer = run_in(program, arguments, action.working_directory()).await?;
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

    fn set_action(arguments: &[(&str, Value)]) -> Action {
        arguments.iter().fold(
            Action::new(
                "package",
                "set",
                ObjectId::new(ono_value::SchemaId::new("ono.package", 1), []),
            ),
            |action, (name, value)| action.with(*name, value.clone()),
        )
    }

    fn managers() -> Dpkg {
        Dpkg {
            dpkg_query: PathBuf::from("/fake/dpkg-query"),
            apt_cache: None,
            apt_get: Some(PathBuf::from("/fake/apt-get")),
            apt_mark: None,
            apt_config: None,
        }
    }

    #[test]
    fn should_upgrade_only_an_installed_package_when_an_update_is_asked_for() {
        let mutation = Mutation::of(
            &set_action(&[("latest", Value::Bool(true))]),
            &managers(),
            "curl",
        )
        .expect("an update is a mutation the provider runs");
        assert_eq!(
            mutation.commands,
            [(
                PathBuf::from("/fake/apt-get"),
                vec![
                    "install".to_owned(),
                    "--only-upgrade".to_owned(),
                    "-y".to_owned(),
                    "--no-remove".to_owned(),
                    "-o".to_owned(),
                    "APT::Cmd::Pattern-Only=true".to_owned(),
                    "--".to_owned(),
                    "curl".to_owned()
                ]
            )],
            "ADR-1084: apt-get's upgrade-only install, which never installs a missing package"
        );
        assert!(
            mutation.versioned,
            "an update is read back from dpkg's version"
        );
    }

    #[test]
    fn should_refuse_an_update_that_also_names_a_version() {
        let refused = Mutation::of(
            &set_action(&[
                ("latest", Value::Bool(true)),
                ("version", Value::string("9.0")),
            ]),
            &managers(),
            "curl",
        );
        assert!(
            refused.is_err_and(|error| error.message().contains("--latest")
                && error.code() == ErrorCode::TypeMismatch),
            "ADR-1084: one transition has one destination, refused as a usage error (COR-C2)"
        );
    }

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

/// A package name as the action means it (ADR-0957): a local archive written as a relative path
/// — `./foo.deb`, `../pkgs/foo.deb`, the form apt takes for a file — is joined onto the action's
/// directory; a package name is not a path and travels as written.
fn local_package(action: &Action, name: &str) -> String {
    if name.starts_with("./") || name.starts_with("../") {
        return action
            .resolve_path(Path::new(name))
            .to_string_lossy()
            .into_owned();
    }
    name.to_owned()
}

#[cfg(test)]
mod local_packages {
    use super::local_package;
    use ono_provider_api::{Action, ObjectId};
    use ono_value::SchemaId;

    #[test]
    fn should_anchor_a_relative_archive_and_leave_a_package_name_alone_when_adding_from_a_job() {
        let action = Action::new(
            "package",
            "add",
            ObjectId::new(SchemaId::new("ono.package", 1), []),
        )
        .within(std::sync::Arc::from(std::path::Path::new("/job")));
        assert_eq!(local_package(&action, "./foo.deb"), "/job/./foo.deb");
        assert_eq!(
            local_package(&action, "../pkgs/foo.deb"),
            "/job/../pkgs/foo.deb"
        );
        assert_eq!(local_package(&action, "openssl"), "openssl");
        assert_eq!(local_package(&action, "/srv/foo.deb"), "/srv/foo.deb");
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
mod names_reaching_apt {
    //! Review findings SEC-C-1 and COR-C5: nothing reaches apt-get, apt-mark or dpkg-query that
    //! they would read as an option, a pattern or an instruction, and an update of a package that
    //! is not installed is refused before any manager runs. The managers are fakes on a PATH of
    //! the test's own, which write down every invocation; no real package manager runs.

    use super::*;
    use ono_provider_api::{Action, ObjectId};

    /// The names the fake package database knows: `apt-cache pkgnames -- <prefix>` lists those
    /// that begin with the prefix, as apt-cache does.
    const KNOWN: &str = "g++ g++-13 python3.11 python3.11-venv curl";

    /// A PATH holding a `dpkg-query` that lists `listed`, an `apt-cache` that knows [`KNOWN`] and
    /// fakes of apt-get and apt-mark, all writing their arguments to `calls.log`.
    fn fake_managers(listed: &str) -> (tempfile::TempDir, PackageProvider) {
        let directory = tempfile::tempdir().expect("a scratch PATH");
        let log = directory.path().join("calls.log");
        let body = |name: &str, answer: &str| {
            format!(
                "#!/bin/sh\nprintf '%s' '{name}' >> '{log}'\nfor a in \"$@\"; do printf ' [%s]' \"$a\" >> '{log}'; done\necho >> '{log}'\n{answer}\n",
                log = log.display()
            )
        };
        let listing = if listed.is_empty() {
            "exit 1".to_owned()
        } else {
            format!("printf '{listed}\\n'; exit 0")
        };
        let pkgnames = format!(
            "for n in {KNOWN}; do case \"$n\" in \"$3\"*) echo \"$n\";; esac; done; exit 0"
        );
        for (name, answer) in [
            ("dpkg-query", listing.as_str()),
            ("apt-cache", pkgnames.as_str()),
            ("apt-get", "exit 0"),
            ("apt-mark", "exit 0"),
        ] {
            ono_testkit::executable_script(directory.path(), name, &body(name, answer));
        }
        let provider = PackageProvider::with_path(Some(directory.path().as_os_str().to_owned()));
        (directory, provider)
    }

    fn calls(directory: &tempfile::TempDir) -> String {
        std::fs::read_to_string(directory.path().join("calls.log")).unwrap_or_default()
    }

    fn action(operation: &str, name: &str, arguments: &[(&str, Value)]) -> Action {
        arguments.iter().fold(
            Action::new(
                "package",
                operation,
                ObjectId::new(
                    SchemaId::new("ono.package", 1),
                    [Value::string(DPKG), Value::string(name)],
                ),
            ),
            |action, (option, value)| action.with(*option, value.clone()),
        )
    }

    #[tokio::test]
    async fn should_refuse_a_name_apt_would_read_as_an_option_a_pattern_or_an_instruction() {
        for name in [
            "-oDebug::pkgProblemResolver=1",
            "--auto-remove",
            "*",
            "/usr/bin/curl",
            "pkgconfig(openssl)",
        ] {
            for (operation, arguments) in [
                ("add", vec![]),
                ("remove", vec![]),
                ("set", vec![("latest", Value::Bool(true))]),
                ("set", vec![("hold", Value::Bool(true))]),
            ] {
                let (directory, provider) = fake_managers("");
                let refused = provider
                    .act(&action(operation, name, &arguments))
                    .await
                    .expect_err("SEC-C-1: the name is refused, not handed on");
                assert_eq!(
                    refused.code(),
                    ErrorCode::TypeMismatch,
                    "`{operation} package {name}`: {refused:?}"
                );
                assert_eq!(
                    calls(&directory),
                    "",
                    "`{operation} package {name}`: no manager ran, not even a lookup"
                );
            }
        }
    }

    #[tokio::test]
    async fn should_refuse_a_suffixed_or_dotted_name_the_package_database_does_not_know() {
        // Review finding SEC-C-1: apt reads `curl-` as "remove curl", `curl+` as "install curl"
        // and `xz.` as a pattern over every package — wherever no package has exactly that name.
        // The package database is asked, read-only and after `--`; unknown, the name is refused
        // and no mutation starts.
        for name in ["curl-", "curl+", "xz.", "python3.12"] {
            for (operation, arguments) in [
                ("add", vec![]),
                ("remove", vec![]),
                ("set", vec![("hold", Value::Bool(true))]),
            ] {
                let (directory, provider) = fake_managers("");
                let refused = provider
                    .act(&action(operation, name, &arguments))
                    .await
                    .expect_err("an instruction or a pattern is refused");
                assert_eq!(
                    refused.code(),
                    ErrorCode::TypeMismatch,
                    "`{operation} package {name}`: {refused:?}"
                );
                assert!(
                    refused
                        .message()
                        .contains("knows no package called exactly"),
                    "the refusal says what was looked for: {refused:?}"
                );
                let asked = calls(&directory);
                assert!(
                    asked.contains(&format!("apt-cache [pkgnames] [--] [{name}]")),
                    "the database was asked for exactly that name, after `--`: {asked}"
                );
                assert!(
                    !asked.contains("apt-get") && !asked.contains("apt-mark"),
                    "no mutation started: {asked}"
                );
            }
        }
    }

    #[tokio::test]
    async fn should_take_a_suffixed_or_dotted_name_the_package_database_knows_as_that_package() {
        // `g++` ends in `+` and `python3.11` holds a `.`, and both are packages: a name the
        // database knows exactly is literal to apt, and stopping `add package g++` would be a
        // regression against v0.6.5.
        for name in ["g++", "python3.11"] {
            let (directory, provider) = fake_managers("");
            let outcome = provider
                .act(&action("add", name, &[]).as_dry_run())
                .await
                .expect("a package the database knows is one package");
            assert_eq!(
                outcome.status(),
                ono_value::ActionStatus::Skipped,
                "`add package {name}` is a dry run here: {outcome:?}"
            );
            assert!(
                !calls(&directory).contains("apt-get"),
                "a dry run starts no mutation: {}",
                calls(&directory)
            );
        }
    }

    #[tokio::test]
    async fn should_refuse_a_version_that_is_not_a_version() {
        let (directory, provider) = fake_managers("");
        let refused = provider
            .act(&action(
                "set",
                "curl",
                &[("version", Value::string("8.* foo"))],
            ))
            .await
            .expect_err("SEC-C-1: a glob in a version is a different request");
        assert_eq!(refused.code(), ErrorCode::TypeMismatch, "{refused:?}");
        assert_eq!(calls(&directory), "");
    }

    #[tokio::test]
    async fn should_refuse_an_update_of_a_package_that_is_not_installed_before_apt_runs() {
        // COR-C5: `apt-get install --only-upgrade` skips a missing package with status 0.
        let (directory, provider) = fake_managers("");
        let refused = provider
            .act(&action("set", "curl", &[("latest", Value::Bool(true))]))
            .await
            .expect_err("an update of a package that is not there is refused");
        assert_eq!(refused.code(), ErrorCode::IoNotFound, "{refused:?}");
        assert!(
            !calls(&directory).contains("apt-get"),
            "no manager ran: {}",
            calls(&directory)
        );
        assert!(
            calls(&directory).contains("dpkg-query") && calls(&directory).contains("[--] [curl]"),
            "the database was asked, with the name after `--`: {}",
            calls(&directory)
        );
    }

    #[tokio::test]
    async fn should_let_an_installed_dotted_name_through_as_a_name() {
        let (_directory, provider) = fake_managers("python3.11\\t3.11.2-6\\tinstall ok installed");
        let outcome = provider
            .act(&action("set", "python3.11", &[("latest", Value::Bool(true))]).as_dry_run())
            .await
            .expect("an installed name is one package");
        assert_eq!(
            outcome.status(),
            ono_value::ActionStatus::Skipped,
            "{outcome:?}"
        );
    }

    #[test]
    fn should_put_the_package_after_double_dash_in_every_command_it_builds() {
        let dpkg = Dpkg {
            dpkg_query: PathBuf::from("/fake/dpkg-query"),
            apt_cache: None,
            apt_get: Some(PathBuf::from("/fake/apt-get")),
            apt_mark: Some(PathBuf::from("/fake/apt-mark")),
            apt_config: None,
        };
        for (name, operation, arguments) in [
            ("curl", "add", vec![]),
            ("g++", "add", vec![]),
            (
                "python3.11",
                "add",
                vec![("version", Value::string("3.11.2-6"))],
            ),
            ("curl", "remove", vec![]),
            ("curl", "remove", vec![("purge", Value::Bool(true))]),
            ("g++", "set", vec![("latest", Value::Bool(true))]),
            ("curl", "set", vec![("version", Value::string("8.5.0"))]),
            ("curl", "set", vec![("hold", Value::Bool(true))]),
        ] {
            let mutation = Mutation::of(&action(operation, name, &arguments), &dpkg, name)
                .expect("a mutation");
            for (program, built) in &mutation.commands {
                let at = built
                    .iter()
                    .position(|argument| argument == "--")
                    .unwrap_or_else(|| panic!("{program:?} {built:?} has no `--`"));
                assert_eq!(
                    at,
                    built.len() - 2,
                    "the package is the one argument after `--`: {built:?}"
                );
                assert!(
                    built
                        .last()
                        .is_some_and(|operand| operand.starts_with(name)),
                    "the operand is the package: {built:?}"
                );
                assert_eq!(
                    built[at - 2..at],
                    ["-o".to_owned(), "APT::Cmd::Pattern-Only=true".to_owned()],
                    "apt reads the operand as a name, never as a regex or a glob: {built:?}"
                );
                if program.ends_with("apt-get") && built[0] == "install" {
                    assert!(
                        built.iter().any(|argument| argument == "--no-remove"),
                        "an install never removes anything: {built:?}"
                    );
                }
            }
        }
    }
}
