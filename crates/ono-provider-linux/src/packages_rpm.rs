//! The `package` provider for rpm-based systems: Red Hat's `dnf`/`yum` and SUSE's `zypper`,
//! over the one database both families keep.
//!
//! Spec §31.58 and AGENTS.md §6 fix the rule — a provider asks a tool for an explicit
//! machine-readable mode and never parses its human listing — and every tool here has one:
//! `rpm --queryformat` prints exactly the fields a format string names, tab-separated, one
//! package per line; `dnf repoquery --queryformat` does the same for the repositories; and
//! `zypper --xmlout` is the machine interface zypper(8) documents, whose `solvable` elements
//! are the search result. A listing that is not in the declared format is a provider defect —
//! `provider.schema_violation` — and never a source of records named after whatever the bytes
//! happened to say (spec §35.3, ADR-0422).
//!
//! # One database, three front ends
//!
//! What a package *is* on Fedora, RHEL, openSUSE and SLES is one thing: an entry in the rpm
//! database. What can reach a repository differs, and so does nothing else — which is why this
//! is one provider whose front end varies, rather than one provider per distribution. Records
//! name `rpm` as their provider on all of them, because that is the database their identity
//! belongs to (`ono.package/1` is `provider + name`).
//!
//! zypper decides when both front ends are present: Fedora and RHEL never ship it, while dnf
//! installs anywhere, so a machine that has zypper is a SUSE machine (ADR-0422 §2).

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, ValueStream};
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, ObjectRef, Provider, Query, Risk, Selector,
};
use ono_value::{ErrorValue, RecordValue, Schema, Value};
use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};

use crate::packages::{
    Listed, Mutation, Plan, manager_failure, on_path, package_name, package_record, package_schema,
    run,
};

/// The id this provider signs its records with.
pub const RPM_PROVIDER_ID: &str = "linux.packages.rpm";

/// The `provider` field of a record the rpm database answered, and the first half of its
/// identity. It is `rpm` on Red Hat and on SUSE alike: one database, one namespace of names.
const RPM: &str = "rpm";

/// The format `rpm -q --queryformat` is asked for: name, then version-release, tab-separated.
const LISTING_FORMAT: &str = "%{NAME}\\t%{VERSION}-%{RELEASE}\\n";

/// The format `dnf repoquery --queryformat` is asked for: name, then summary.
const REPOQUERY_FORMAT: &str = "%{name}\\t%{summary}\\n";

/// zypper's exit status for "nothing matched" (zypper(8), `ZYPPER_EXIT_INF_CAP_NOT_FOUND`).
const ZYPPER_NOTHING_MATCHED: i32 = 104;

/// What can reach a repository beside the rpm database.
#[derive(Debug, Clone)]
enum Frontend {
    /// `dnf`, or `yum` where that is the name the distribution installs it under.
    RedHat(PathBuf),
    /// `zypper`.
    Suse(PathBuf),
}

/// The rpm family of tools, as found on `PATH`.
#[derive(Debug, Clone)]
struct Rpm {
    rpm: PathBuf,
    frontend: Option<Frontend>,
}

impl Rpm {
    /// The front end, or the refusal that names what was looked for.
    fn frontend(&self) -> Result<&Frontend, ErrorValue> {
        self.frontend.as_ref().ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ProviderUnsupported,
                "the rpm database is here and nothing that can reach a repository is: looked \
                 for `zypper`, `dnf` and `yum` on PATH",
            )
            .with_help("`get package` still lists what rpm has installed")
        })
    }
}

/// The package provider for rpm-based systems: `ono.package/1` records from rpm, dnf and zypper.
///
/// ```
/// use ono_provider_api::Provider;
/// use ono_provider_linux::RpmPackageProvider;
///
/// let provider = RpmPackageProvider::with_path(Some("/nonexistent".into()));
/// let reason = provider.availability().reason().map(str::to_owned);
/// assert!(reason.is_some_and(|reason| reason.contains("rpm")));
/// ```
#[derive(Debug)]
pub struct RpmPackageProvider {
    manager: Option<Rpm>,
}

impl Default for RpmPackageProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl RpmPackageProvider {
    /// A provider over the managers found on this process's `PATH`.
    #[must_use]
    pub fn new() -> Self {
        Self::with_path(std::env::var_os("PATH"))
    }

    /// A provider over the managers found on `path`, or none.
    #[must_use]
    pub fn with_path(path: Option<OsString>) -> Self {
        let find = |name: &str| on_path(path.as_ref(), name);
        let manager = find("rpm").map(|rpm| Rpm {
            rpm,
            // zypper first: a machine that carries it is a SUSE machine, whatever else it has
            // installed, and dnf is what Red Hat's `yum` has been for two major releases.
            frontend: find("zypper")
                .map(Frontend::Suse)
                .or_else(|| find("dnf").map(Frontend::RedHat))
                .or_else(|| find("yum").map(Frontend::RedHat)),
        });
        Self { manager }
    }

    fn manager(&self) -> Result<Rpm, ErrorValue> {
        self.manager.clone().ok_or_else(|| {
            ErrorValue::new(ErrorCode::ProviderUnavailable, unavailable_reason()).with_help(
                "`package` needs a package manager the shell can ask in a machine format. \
                 Having none is not the same as having no packages, so this is a refusal to \
                 answer rather than an empty answer.",
            )
        })
    }
}

fn unavailable_reason() -> String {
    "no supported package manager is on PATH: looked for `rpm` (rpm/dnf/yum/zypper)".to_owned()
}

/// Whether `name` is a package name rpm would accept: alphanumerics and `+-._~^`, starting with
/// an alphanumeric. Unlike a Debian name it may carry capitals and be one character long —
/// `NetworkManager` and `R` are both real packages.
fn is_rpm_package_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.' | '_' | '~' | '^'))
}

/// A schema violation naming the tool that produced it.
fn violation(tool: &str, detail: &str) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::ProviderSchemaViolation,
        format!("`{tool}` did not answer in its documented format: {detail}"),
    )
    .with_help(
        "this is a defect at the package-manager boundary, not in your pipeline, and no record \
         was made from it",
    )
}

/// Reads the listing `rpm -q --queryformat` printed.
///
/// Everything rpm's database answers for is installed — it holds no record of a package that is
/// not — and a package installed for two architectures is listed once per architecture and is
/// still one object, because `ono.package/1` is identified by `provider + name`.
///
/// # Errors
///
/// `provider.schema_violation` when the bytes are not the declared format: not text, or a line
/// that is not `NAME\tVERSION-RELEASE` with a well-formed name.
fn parse_listing(bytes: &[u8]) -> Result<Vec<Listed>, ErrorValue> {
    let tool = "rpm -q --queryformat";
    let text =
        std::str::from_utf8(bytes).map_err(|_| violation(tool, "the output is not UTF-8 text"))?;
    let mut listed = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let [name, version] = fields[..] else {
            return Err(violation(
                tool,
                &format!(
                    "line {} has {} tab-separated fields, not 2",
                    index + 1,
                    fields.len()
                ),
            ));
        };
        if !is_rpm_package_name(name) {
            return Err(violation(
                tool,
                &format!(
                    "line {} names {name:?}, which is not a package name",
                    index + 1
                ),
            ));
        }
        listed.push(Listed {
            name: name.to_owned(),
            version: Some(version).filter(|v| !v.is_empty()).map(str::to_owned),
            installed: true,
        });
    }
    let mut seen = HashSet::new();
    listed.retain(|entry| seen.insert(entry.name.clone()));
    Ok(listed)
}

/// Reads the hits `dnf repoquery --queryformat` printed: `name<TAB>summary` per line.
///
/// A repository carries a package in several versions and architectures and repoquery lists
/// each; the first line for a name is the record, for the same identity reason as above.
///
/// # Errors
///
/// `provider.schema_violation` when a line is not in that shape.
fn parse_repoquery(bytes: &[u8]) -> Result<Vec<(String, Option<String>)>, ErrorValue> {
    let tool = "dnf repoquery --queryformat";
    let text =
        std::str::from_utf8(bytes).map_err(|_| violation(tool, "the output is not UTF-8 text"))?;
    let mut hits: Vec<(String, Option<String>)> = Vec::new();
    let mut seen = HashSet::new();
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let Some((name, summary)) = line.split_once('\t') else {
            return Err(violation(
                tool,
                &format!("line {} is not `name<TAB>summary`", index + 1),
            ));
        };
        if !is_rpm_package_name(name) {
            return Err(violation(
                tool,
                &format!(
                    "line {} names {name:?}, which is not a package name",
                    index + 1
                ),
            ));
        }
        if seen.insert(name.to_owned()) {
            hits.push((
                name.to_owned(),
                Some(summary).filter(|s| !s.is_empty()).map(str::to_owned),
            ));
        }
    }
    Ok(hits)
}

/// Reads the packages out of the document `zypper --xmlout search` printed.
///
/// The answer is a `solvable` per hit; a `srcpackage` is a source package and not a package, and
/// a search result carries no summary, so the description of a hit is null rather than invented
/// (spec §35.3).
///
/// # Errors
///
/// `provider.schema_violation` when the bytes are not zypper's XML, or when a `solvable` carries
/// no name or a name that is not one.
fn parse_zypper_search(bytes: &[u8]) -> Result<Vec<String>, ErrorValue> {
    let tool = "zypper --xmlout search";
    let mut reader = Reader::from_reader(bytes);
    let mut buffer = Vec::new();
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    let mut is_a_stream = false;
    loop {
        let element = match reader.read_event_into(&mut buffer) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(element)) if element.name().as_ref() == "stream" => {
                is_a_stream = true;
                buffer.clear();
                continue;
            }
            Ok(Event::Start(element) | Event::Empty(element))
                if element.name().as_ref() == "solvable" =>
            {
                element.into_owned()
            }
            Ok(_) => {
                buffer.clear();
                continue;
            }
            Err(error) => return Err(violation(tool, &format!("{error}"))),
        };
        buffer.clear();

        let mut name = None;
        let mut kind = None;
        for attribute in element.attributes() {
            let attribute = attribute
                .map_err(|error| violation(tool, &format!("a `solvable` attribute: {error}")))?;
            let value = attribute
                .normalized_value(XmlVersion::Explicit1_0)
                .map_err(|error| violation(tool, &format!("a `solvable` attribute: {error}")))?
                .into_owned();
            match attribute.key.as_ref() {
                "name" => name = Some(value),
                "kind" => kind = Some(value),
                _ => {}
            }
        }
        if kind.as_deref() != Some("package") {
            continue;
        }
        let Some(name) = name else {
            return Err(violation(tool, "a `solvable` element carries no `name`"));
        };
        if !is_rpm_package_name(&name) {
            return Err(violation(
                tool,
                &format!("a `solvable` names {name:?}, which is not a package name"),
            ));
        }
        if seen.insert(name.clone()) {
            names.push(name);
        }
    }
    if !is_a_stream {
        return Err(violation(tool, "the document has no `stream` element"));
    }
    Ok(names)
}

/// The installed set, or one package's entry, from `rpm --queryformat`.
async fn installed(rpm: &Rpm, name: Option<&str>) -> Result<Vec<Listed>, ErrorValue> {
    let mut arguments = vec![
        if name.is_some() { "-q" } else { "-qa" }.to_owned(),
        "--queryformat".to_owned(),
        LISTING_FORMAT.to_owned(),
    ];
    arguments.extend(name.map(str::to_owned));
    let answer = run(&rpm.rpm, &arguments).await?;
    match answer.status {
        Some(0) => parse_listing(&answer.stdout),
        // `rpm -q <name>` exits non-zero for a package the database does not have, and says so
        // on stdout. That is rpm's documented answer to the question and an ordinary empty
        // result here — while anything that reached stderr is a failure and is reported.
        _ if name.is_some() && answer.stderr.is_empty() => Ok(Vec::new()),
        status => Err(ErrorValue::new(
            ErrorCode::ProviderUnavailable,
            format!(
                "`rpm -q` failed with status {}: {}",
                status.map_or("signal".to_owned(), |code| code.to_string()),
                String::from_utf8_lossy(&answer.stderr).trim()
            ),
        )),
    }
}

/// The version rpm has installed for `name`, or `None`.
async fn installed_version(rpm: &Rpm, name: &str) -> Result<Option<String>, ErrorValue> {
    Ok(installed(rpm, Some(name))
        .await?
        .into_iter()
        .find(|entry| entry.name == name)
        .and_then(|entry| entry.version))
}

/// The repositories' hits for `term`, and the invocation that answered.
async fn search(
    rpm: &Rpm,
    term: &str,
) -> Result<(Vec<(String, Option<String>)>, &'static str), ErrorValue> {
    let failed = |program: &Path, answer: &crate::packages::Answer| {
        ErrorValue::new(
            ErrorCode::ProviderUnavailable,
            format!(
                "`{}` failed with status {}: {}",
                program.display(),
                answer
                    .status
                    .map_or("signal".to_owned(), |code| code.to_string()),
                String::from_utf8_lossy(&answer.stderr).trim()
            ),
        )
    };
    match rpm.frontend()? {
        Frontend::RedHat(program) => {
            let arguments = vec![
                "--quiet".to_owned(),
                "repoquery".to_owned(),
                "--queryformat".to_owned(),
                REPOQUERY_FORMAT.to_owned(),
                format!("*{term}*"),
            ];
            let found = run(program, &arguments).await?;
            if found.status != Some(0) {
                return Err(failed(program, &found));
            }
            Ok((
                parse_repoquery(&found.stdout)?,
                "dnf repoquery --queryformat",
            ))
        }
        Frontend::Suse(program) => {
            let arguments = vec![
                "--xmlout".to_owned(),
                "--non-interactive".to_owned(),
                "--no-refresh".to_owned(),
                "search".to_owned(),
                "--type".to_owned(),
                "package".to_owned(),
                term.to_owned(),
            ];
            let found = run(program, &arguments).await?;
            match found.status {
                Some(0) => Ok((
                    parse_zypper_search(&found.stdout)?
                        .into_iter()
                        .map(|name| (name, None))
                        .collect(),
                    "zypper --xmlout search",
                )),
                Some(ZYPPER_NOTHING_MATCHED) => Ok((Vec::new(), "zypper --xmlout search")),
                _ => Err(failed(program, &found)),
            }
        }
    }
}

/// The records a plan produces, in the order the manager listed them.
async fn answer(rpm: &Rpm, plan: &Plan) -> Result<Vec<RecordValue>, ErrorValue> {
    if let Some(term) = &plan.search {
        let (hits, source) = search(rpm, term).await?;
        // What rpm has of them decides `installed` and `version`; one listing answers for every
        // hit, where one `rpm -q` per hit would be one process per hit.
        let listed = if hits.is_empty() {
            Vec::new()
        } else {
            installed(rpm, None).await?
        };
        return hits
            .iter()
            .map(|(name, description)| {
                let entry = listed.iter().find(|entry| &entry.name == name);
                package_record(
                    name,
                    entry.and_then(|entry| entry.version.as_deref()),
                    Some(entry.is_some()),
                    description.as_deref(),
                    RPM,
                    RPM_PROVIDER_ID,
                    source,
                )
            })
            .collect();
    }

    if let Some(name) = &plan.named
        && !is_rpm_package_name(name)
    {
        // rpm would only complain; an impossible name has no package and needs no process.
        return Ok(Vec::new());
    }
    installed(rpm, plan.named.as_deref())
        .await?
        .iter()
        .map(|entry| {
            package_record(
                &entry.name,
                entry.version.as_deref(),
                Some(entry.installed),
                None,
                RPM,
                RPM_PROVIDER_ID,
                "rpm -q --queryformat",
            )
        })
        .collect()
}

/// The front end invocations a `package` action asks for.
fn mutation_of(action: &Action, rpm: &Rpm, name: &str) -> Result<Mutation, ErrorValue> {
    let frontend = rpm.frontend()?;
    let unsupported = |message: String, help: &str| {
        Err(ErrorValue::new(ErrorCode::ProviderUnsupported, message).with_help(help))
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
    // `--non-interactive` is zypper's documented way of being run by a program, and `-y` is
    // dnf's. Neither may ever be left off: a package manager waiting for an answer at a pipe
    // that has nobody behind it is a hung shell.
    let install = |version: Option<&str>| match frontend {
        Frontend::RedHat(program) => (
            program.clone(),
            vec![
                "install".to_owned(),
                "-y".to_owned(),
                version.map_or_else(|| name.to_owned(), |version| format!("{name}-{version}")),
            ],
        ),
        Frontend::Suse(program) => {
            let mut arguments = vec!["--non-interactive".to_owned(), "install".to_owned()];
            // Moving to a named version is as often a step back as forward, and zypper refuses
            // to go back unless it is told that is what was meant.
            if version.is_some() {
                arguments.push("--oldpackage".to_owned());
            }
            arguments.push(
                version.map_or_else(|| name.to_owned(), |version| format!("{name}={version}")),
            );
            (program.clone(), arguments)
        }
    };

    match action.operation() {
        "add" => {
            let version = text("version");
            Ok(Mutation {
                commands: vec![install(version.as_deref())],
                described: match &version {
                    Some(version) => format!("install `{name}` at {version}"),
                    None => format!("install `{name}`"),
                },
                versioned: true,
            })
        }
        "remove" => {
            if flag("purge") == Some(true) {
                return unsupported(
                    "rpm has no purge: a removal leaves a configuration file the administrator \
                     changed behind as `.rpmsave`"
                        .to_owned(),
                    "`remove package <name>` removes the package; what rpm kept is a file, and \
                     deleting it is yours to do",
                );
            }
            let command = match frontend {
                Frontend::RedHat(program) => (
                    program.clone(),
                    vec!["remove".to_owned(), "-y".to_owned(), name.to_owned()],
                ),
                Frontend::Suse(program) => (
                    program.clone(),
                    vec![
                        "--non-interactive".to_owned(),
                        "remove".to_owned(),
                        name.to_owned(),
                    ],
                ),
            };
            Ok(Mutation {
                commands: vec![command],
                described: format!("remove `{name}`"),
                versioned: true,
            })
        }
        "set" => {
            let mut commands = Vec::new();
            let mut described = Vec::new();
            let mut versioned = false;
            if let Some(version) = text("version") {
                commands.push(install(Some(&version)));
                described.push(format!("move `{name}` to {version}"));
                versioned = true;
            }
            if let Some(hold) = flag("hold") {
                commands.push(match frontend {
                    // The version lock is a dnf plugin. Where it is not installed, dnf says so
                    // itself and that is the outcome of the action.
                    Frontend::RedHat(program) => (
                        program.clone(),
                        vec![
                            "versionlock".to_owned(),
                            if hold { "add" } else { "delete" }.to_owned(),
                            name.to_owned(),
                        ],
                    ),
                    Frontend::Suse(program) => (
                        program.clone(),
                        vec![
                            "--non-interactive".to_owned(),
                            if hold { "addlock" } else { "removelock" }.to_owned(),
                            name.to_owned(),
                        ],
                    ),
                });
                described.push(format!("{} `{name}`", if hold { "hold" } else { "unhold" }));
            }
            if commands.is_empty() {
                return unsupported(
                    "the package provider changes `version` and `hold`, and `set` named neither"
                        .to_owned(),
                    "write `--version 1.24.0` or `--hold true`",
                );
            }
            Ok(Mutation {
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

#[async_trait::async_trait]
impl Provider for RpmPackageProvider {
    fn id(&self) -> &str {
        RPM_PROVIDER_ID
    }

    fn identity_token(&self) -> Option<&str> {
        Some(RPM)
    }

    fn targets(&self) -> &[&str] {
        &["package"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        vec![package_schema()]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::new("package.list", Risk::Read),
            Capability::new("package.search", Risk::Read),
            // `docs/spec/capabilities.yaml` gives `package.manage` elevation `required`: the rpm
            // database is root's, and the provider says so before it runs anything.
            Capability::new("package.manage", Risk::Mutate).needing_elevation(),
        ]
    }

    fn availability(&self) -> Availability {
        match &self.manager {
            Some(_) => Availability::Available,
            None => Availability::unavailable(unavailable_reason()),
        }
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let rpm = self.manager()?;
        let plan = Plan::of(query);
        let limit = query.max();
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            Boundedness::Bounded,
            move |sink| async move {
                let records = match answer(&rpm, &plan).await {
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
        // A well-formed name rpm does not have is still a package identity — the one `add
        // package <name>` is about to install. Whether the repositories carry it is the front
        // end's to say when asked (ADR-0115 §3).
        if let Selector::Field { name, value } = selector
            && name == "name"
            && let Ok(text) = value.as_str()
            && is_rpm_package_name(text)
        {
            let record = package_record(
                text,
                None,
                Some(false),
                None,
                RPM,
                RPM_PROVIDER_ID,
                "rpm -q --queryformat",
            )?;
            return Ok(ObjectRef::of(&record).into_iter().collect());
        }
        Ok(Vec::new())
    }

    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        let rpm = self.manager()?;
        let name = package_name(action.target())?.to_owned();
        let mutation = mutation_of(action, &rpm, &name)?;
        if action.is_dry_run() {
            return Ok(ActionOutcome::skipped(
                action,
                format!("would {}", mutation.described),
            ));
        }
        // Spec §17.2: elevation is explicit. The rpm database is root's, and the outcome of
        // asking dnf or zypper as anyone else is known before it runs.
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
        let before = installed_version(&rpm, &name).await?;
        for (program, arguments) in &mutation.commands {
            let answer = run(program, arguments).await?;
            if answer.status != Some(0) {
                return Ok(ActionOutcome::failed(
                    action,
                    manager_failure(program, &answer),
                ));
            }
        }
        // What changed is rpm's to say, not the front end's prose: the version before against
        // after.
        let changed = if mutation.versioned {
            installed_version(&rpm, &name).await? != before
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
        let listed = parse_listing(b"curl\t8.6.0-8.fc40\nNetworkManager\t1.46.0-1.fc40\n").unwrap();
        assert_eq!(
            listed,
            [
                Listed {
                    name: "curl".into(),
                    version: Some("8.6.0-8.fc40".into()),
                    installed: true
                },
                Listed {
                    name: "NetworkManager".into(),
                    version: Some("1.46.0-1.fc40".into()),
                    installed: true
                },
            ]
        );
    }

    #[test]
    fn should_answer_once_for_a_package_installed_for_two_architectures() {
        let listed = parse_listing(b"glibc\t2.39-5.fc40\nglibc\t2.39-5.fc40\n").unwrap();
        assert_eq!(listed.len(), 1, "one name is one object, got {listed:?}");
    }

    #[test]
    fn should_refuse_a_listing_that_is_not_in_the_machine_format() {
        for garbage in [
            &b"\xff\xfe not a listing at all ~~~\n"[..],
            b"curl 8.6.0-8.fc40\n",
            b"curl\t8.6.0\textra\n",
            b"-nota name\t1\n",
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
    fn should_read_repository_hits_as_name_and_summary() {
        let hits = parse_repoquery(b"curl\tA utility\ncurl\tA utility\nzsh\t\n").unwrap();
        assert_eq!(
            hits,
            [
                ("curl".to_owned(), Some("A utility".to_owned())),
                // Spec §35.3: a summary the repository does not carry is null, not an empty
                // string pretending to be one.
                ("zsh".to_owned(), None),
            ]
        );
    }

    #[test]
    fn should_read_the_packages_out_of_zypper_xml() {
        let document = br#"<?xml version='1.0'?>
<stream>
<search-result version="0.0">
<solvable-list>
<solvable status="not-installed" name="curl" kind="package" edition="8.6.0-1.1"/>
<solvable status="not-installed" name="curl-source" kind="srcpackage" edition="8.6.0-1.1"/>
<solvable status="installed" name="curl" kind="package" edition="8.5.0-1.1"/>
</solvable-list>
</search-result>
</stream>
"#;
        assert_eq!(parse_zypper_search(document).unwrap(), ["curl"]);
    }

    #[test]
    fn should_refuse_a_search_answer_that_is_not_zypper_xml() {
        for garbage in [
            &b"S | Name | Summary | Type\n--+------+---------+-----\n"[..],
            b"",
        ] {
            let error = parse_zypper_search(garbage).unwrap_err();
            assert_eq!(
                error.code(),
                ErrorCode::ProviderSchemaViolation,
                "{garbage:?}"
            );
        }
    }

    #[test]
    fn should_accept_the_names_rpm_accepts() {
        for name in [
            "curl",
            "NetworkManager",
            "R",
            "gcc-c++",
            "python3.12",
            "lib_x~1^2",
        ] {
            assert!(is_rpm_package_name(name), "{name}");
        }
        for name in ["-curl", "", "curl!", "cu rl", "café"] {
            assert!(!is_rpm_package_name(name), "{name}");
        }
    }
}
