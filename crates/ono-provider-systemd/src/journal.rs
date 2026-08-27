//! The `journal` and `log` targets: systemd's journal, read as records.
//!
//! The journal has no D-Bus surface. The choices are the binary journal files through
//! `libsystemd`'s `sd-journal` API, or `journalctl --output=json`, whose output is a machine
//! format systemd documents and keeps stable. ADR-0085 takes the second: it is the documented
//! adapter fallback spec §50 allows, it needs no C library, and the v0.3 adapter pack
//! (`docs/spec/adapters/first-party/systemd.yaml`) already decodes exactly that stream into
//! `ono.journal-event/1`. This provider spawns the same invocation and runs the same decoder, so
//! `get journal` and `journalctl` cannot disagree about what a record is.
//!
//! What it never does is parse `journalctl`'s human-readable output, and what it never says is
//! "no entries" when it could not read: a `journalctl` that fails is
//! [`ErrorCode::ProviderUnavailable`] with what it said, not an empty stream (spec §10.5).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use ono_adapter::{Adapter, Decoding, Trace};
use ono_core::ErrorCode;
use ono_pipeline::{Boundedness, PipelineConfig, StreamSink, ValueStream};
use ono_provider_api::{Availability, Capability, ObjectRef, Provider, Query, Risk, Selector};
use ono_value::{ErrorValue, Provenance, RecordValue, Schema, SchemaId, Value, builtin_schemas};
use tokio::io::AsyncReadExt;

/// The id this provider signs its records with.
pub const JOURNAL_PROVIDER_ID: &str = "systemd-journal";

/// The adapter whose decoder this provider runs, by pack and adapter id.
const JOURNALCTL_PACK: &str = "org.ono.compat.systemd";
const JOURNALCTL_ADAPTER: &str = "journalctl";

/// The syslog severities, by priority: `error` is 3 (spec §41.4 writes `level >= error`).
const LEVELS: [&str; 8] = [
    "emerg", "alert", "crit", "error", "warning", "notice", "info", "debug",
];

/// The journal provider: `ono.journal-event/1` records from `journalctl --output=json`.
///
/// ```
/// use ono_provider_api::Provider;
///
/// let provider = ono_provider_systemd::JournalProvider::new();
/// // Without a `journalctl` the provider says so, rather than answering an empty journal.
/// if let Some(reason) = provider.availability().reason() {
///     assert!(reason.contains("journalctl"));
/// }
/// ```
#[derive(Debug)]
pub struct JournalProvider {
    journalctl: Option<PathBuf>,
}

impl Default for JournalProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl JournalProvider {
    /// A provider over the `journalctl` found on this process's `PATH`.
    #[must_use]
    pub fn new() -> Self {
        Self::with_path(std::env::var_os("PATH"))
    }

    /// A provider over the `journalctl` found on `path`, or none.
    #[must_use]
    pub fn with_path(path: Option<OsString>) -> Self {
        let journalctl = path.and_then(|path| {
            std::env::split_paths(&path)
                .map(|directory| directory.join(JOURNALCTL_ADAPTER))
                .find(|candidate| candidate.is_file())
        });
        Self { journalctl }
    }

    fn journalctl(&self) -> Result<&Path, ErrorValue> {
        self.journalctl.as_deref().ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                "no `journalctl` is on PATH, so the journal cannot be read here",
            )
            .with_help(
                "`journal` and `log` read systemd's journal. Having no journal is not the same \
                 as having no log records, so this is a refusal to answer rather than an empty \
                 answer.",
            )
        })
    }
}

/// The `journalctl` adapter of the bundled systemd pack.
fn adapter() -> Result<&'static Adapter, ErrorValue> {
    ono_adapter::first_party()
        .iter()
        .flat_map(ono_adapter::AdapterPack::adapters)
        .find(|adapter| adapter.pack_id() == JOURNALCTL_PACK && adapter.id() == JOURNALCTL_ADAPTER)
        .ok_or_else(|| {
            ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                "the bundled `journalctl` adapter contract is missing, so the journal cannot be \
                 decoded",
            )
        })
}

/// The schema a target's records satisfy.
fn schema_for(target: &str) -> Result<Arc<Schema>, ErrorValue> {
    let id = match target {
        "journal" => SchemaId::new("ono.journal-event", 1),
        "log" => SchemaId::new("ono.log-record", 1),
        other => {
            return Err(ErrorValue::new(
                ErrorCode::ResolveTargetNotFound,
                format!("the journal provider answers `journal` and `log`, not `{other}`"),
            ));
        }
    };
    builtin_schemas().get(&id).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::ProviderUnavailable,
            format!("the schema `{id}` is not built in, so the journal cannot be typed"),
        )
    })
}

/// The `journalctl` arguments a query asks for, after the fixed `--output=json` ones.
///
/// Every option is a declared one of `docs/spec/commands/service.yaml`; a value of the wrong
/// type is refused rather than passed through as text for `journalctl` to guess at.
fn arguments(query: &Query) -> Result<Vec<String>, ErrorValue> {
    let mut argv = vec![
        "--output=json".to_owned(),
        "--no-pager".to_owned(),
        "--quiet".to_owned(),
    ];
    if query.flag("follow") {
        argv.push("--follow".to_owned());
    }
    for (name, value) in query.options() {
        match (name.as_str(), value) {
            ("follow", _) | ("provider", _) => {}
            ("since", Value::Timestamp(at)) => argv.push(format!("--since=@{}", at.as_second())),
            ("until", Value::Timestamp(at)) => argv.push(format!("--until=@{}", at.as_second())),
            ("boot", Value::Int(boot)) => argv.push(format!("--boot={boot}")),
            ("lines", Value::Int(lines)) => argv.push(format!("--lines={lines}")),
            ("service", Value::String(unit)) => argv.push(format!("--unit={unit}")),
            ("level", Value::String(level)) => {
                argv.push(format!("--priority={}", level_name(level)?));
            }
            ("level", Value::Int(priority)) if (0..=7).contains(priority) => {
                argv.push(format!("--priority={priority}"));
            }
            (name, other) => {
                return Err(ErrorValue::new(
                    ErrorCode::TypeMismatch,
                    format!("`--{name}` cannot be a {} here", other.type_name()),
                )
                .with_help("`help get journal` and `help get log` list the option types"));
            }
        }
    }
    Ok(argv)
}

/// The severity name `journalctl --priority` understands, for the spellings a user writes.
fn level_name(level: &str) -> Result<&'static str, ErrorValue> {
    let name = match level.trim().to_ascii_lowercase().as_str() {
        "emerg" | "emergency" | "panic" => "emerg",
        "alert" => "alert",
        "crit" | "critical" => "crit",
        "err" | "error" => "err",
        "warning" | "warn" => "warning",
        "notice" => "notice",
        "info" | "informational" => "info",
        "debug" => "debug",
        _ => {
            return Err(ErrorValue::new(
                ErrorCode::TypeMismatch,
                format!("`{level}` is not a log level"),
            )
            .with_help(format!(
                "`--level` is a minimum severity: {}",
                LEVELS.join(", ")
            )));
        }
    };
    Ok(name)
}

/// A decoded journal event as a record of `schema`, signed by this provider.
///
/// The adapter's decoder signs its records as the adapter; here the observation is the
/// provider's, with the adapter kept in the provenance as the mechanism (spec v0.3 §1.8). The
/// `log` view adds `level`, the severity name of the priority (spec §41.4).
fn reshape(record: &RecordValue, schema: &Arc<Schema>) -> Result<RecordValue, ErrorValue> {
    let mut provenance = Provenance::local(JOURNAL_PROVIDER_ID, schema.id().clone());
    if let Some(source) = record.provenance().source() {
        provenance = provenance.from_source(source);
    }
    if let Some(trace) = record.provenance().adapter() {
        provenance = provenance.adapted_by(trace.clone());
    }
    let mut builder = RecordValue::builder(Arc::clone(schema), provenance);
    for field in schema.fields() {
        let value = match field.name() {
            "level" => record
                .get("priority")
                .and_then(|priority| priority.as_int().ok())
                .and_then(|priority| usize::try_from(priority).ok())
                .and_then(|priority| LEVELS.get(priority))
                .map_or(Value::Null, |level| Value::string(level)),
            name => record.get(name).cloned().unwrap_or(Value::Null),
        };
        builder = builder.set(field.name(), value)?;
    }
    for (key, value) in record.extra().iter() {
        builder = builder.set_extra(key, value.clone());
    }
    Ok(builder.build())
}

/// Runs one `journalctl` and streams its records into `sink` until it ends or the consumer is
/// gone.
async fn run(
    journalctl: PathBuf,
    argv: Vec<String>,
    query: Query,
    schema: Arc<Schema>,
    mut decoding: Decoding<'static>,
    sink: StreamSink,
) {
    let mut command = tokio::process::Command::new(&journalctl);
    command
        .args(&argv)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Its own process group: a Ctrl-C at the terminal reaches the shell's pipeline, which
        // cancels this stream, which stops the child — never the child first.
        .process_group(0)
        .kill_on_drop(true);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = sink
                .fail(
                    ErrorValue::new(
                        ErrorCode::ProviderUnavailable,
                        format!("`{}` could not be run: {error}", journalctl.display()),
                    )
                    .with_help("the journal is read through `journalctl --output=json`"),
                )
                .await;
            return;
        }
    };
    let (Some(mut stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = sink
            .fail(ErrorValue::new(
                ErrorCode::ProviderUnavailable,
                "journalctl's output could not be captured",
            ))
            .await;
        return;
    };
    let stderr = tokio::spawn(async move {
        let mut said = Vec::new();
        let _ = stderr.read_to_end(&mut said).await;
        said
    });

    let mut buffer = vec![0u8; 64 * 1024];
    let mut consumer_gone = false;
    'reading: loop {
        let read = tokio::select! {
            biased;
            () = sink.closed() => {
                consumer_gone = true;
                break 'reading;
            }
            read = stdout.read(&mut buffer) => read,
        };
        let count = match read {
            Ok(0) | Err(_) => break 'reading,
            Ok(count) => count,
        };
        for outcome in decoding.feed(&buffer[..count]) {
            if !deliver(&sink, outcome, &query, &schema).await {
                consumer_gone = true;
                break 'reading;
            }
        }
    }
    if consumer_gone {
        let _ = child.start_kill();
        let _ = child.wait().await;
        return;
    }
    for outcome in decoding.finish() {
        if !deliver(&sink, outcome, &query, &schema).await {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return;
        }
    }
    let status = child.wait().await;
    let said = stderr.await.unwrap_or_default();
    let said = String::from_utf8_lossy(&said);
    let said = said.trim();
    match status {
        Ok(status) if status.success() => {}
        Ok(status) => {
            let _ = sink
                .fail(
                    ErrorValue::new(
                        ErrorCode::ProviderUnavailable,
                        format!(
                            "the journal could not be read: journalctl exited with status {}{}",
                            status.code().unwrap_or(-1),
                            if said.is_empty() {
                                String::new()
                            } else {
                                format!(" — {said}")
                            }
                        ),
                    )
                    .with_help(
                        "a machine without journal files, or a user the journal is not readable \
                         to, has records this shell cannot see; that is not the same as having \
                         none",
                    ),
                )
                .await;
        }
        Err(error) => {
            let _ = sink
                .fail(ErrorValue::new(
                    ErrorCode::ProviderUnavailable,
                    format!("journalctl could not be waited for: {error}"),
                ))
                .await;
        }
    }
}

/// Sends one decoded outcome downstream, in the shape the target promised; `false` once the
/// consumer is gone.
async fn deliver(
    sink: &StreamSink,
    outcome: Result<Value, ErrorValue>,
    query: &Query,
    schema: &Arc<Schema>,
) -> bool {
    match outcome {
        Ok(Value::Record(record)) => {
            if !query.matches(&record) {
                return true;
            }
            match reshape(&record, schema) {
                Ok(record) => sink.send(record.into_value()).await.is_ok(),
                Err(error) => sink.fail(error).await.is_ok(),
            }
        }
        // The decoder only ever produces records; anything else is not a journal event.
        Ok(_) => true,
        Err(error) => sink.fail(error).await.is_ok(),
    }
}

#[async_trait::async_trait]
impl Provider for JournalProvider {
    fn id(&self) -> &str {
        JOURNAL_PROVIDER_ID
    }

    fn targets(&self) -> &[&str] {
        &["journal", "log"]
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        ["journal", "log"]
            .iter()
            .filter_map(|target| schema_for(target).ok())
            .collect()
    }

    fn capabilities(&self) -> Vec<Capability> {
        // `docs/spec/capabilities.yaml`: `log.read`, elevation conditional — what the journal
        // shows depends on the reader's groups, and the journal decides, not the shell.
        vec![Capability::new("log.read", Risk::Read)]
    }

    fn availability(&self) -> Availability {
        match self.journalctl() {
            Ok(_) => Availability::Available,
            Err(error) => Availability::unavailable(error.message().to_owned()),
        }
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let journalctl = self.journalctl()?.to_path_buf();
        let schema = schema_for(query.target_name())?;
        let argv = arguments(query)?;
        let follow = query.flag("follow");
        let spelling = format!(
            "{} {}",
            if follow { "tail" } else { "get" },
            query.target_name()
        );
        let trace = Trace {
            executable: journalctl.clone(),
            version: None,
            user_invocation: vec![spelling],
            actual_invocation: std::iter::once(JOURNALCTL_ADAPTER.to_owned())
                .chain(argv.iter().cloned())
                .collect(),
            host: None,
        };
        let decoding = Decoding::borrowed(adapter()?, trace, builtin_schemas())?;
        let boundedness = if follow {
            Boundedness::Unbounded
        } else {
            Boundedness::Bounded
        };
        let query = query.clone();
        Ok(ValueStream::spawn(
            PipelineConfig::new(),
            boundedness,
            move |sink| run(journalctl, argv, query, schema, decoding, sink),
        ))
    }

    async fn resolve(&self, selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        let query = Query::target("journal").with(selector.clone());
        let collected = self.snapshot(&query)?.collect().await;
        Ok(collected
            .values()
            .iter()
            .filter_map(|value| match value {
                Value::Record(record) => ObjectRef::of(record),
                _ => None,
            })
            .collect())
    }
}
