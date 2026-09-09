#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    dead_code,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
//! The outside world, faked: one provider, one observer, one bridge.
//!
//! AGENTS.md section 11 permits faking the outside world and forbids mocking internal layers.
//! `FakeProvider` stands in for systemd, procfs and dpkg; `FakeObserver` stands in for the facts
//! §7.2 freezes; `TestBridge` is the join §51 needs between a synchronous change provider and an
//! asynchronous one. Nothing here fakes `ProviderChangeProvider` itself.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use jiff::Timestamp;
use ono_change_actions::ProviderChangeProvider;
use ono_change_actions::bridge::{Bridge, drain};
use ono_change_actions::preconditions::Observer;
use ono_change_actions::registry::RebootRequirement;
use ono_change_core::PreconditionKind;
use ono_pipeline::ValueStream;
use ono_provider_api::{
    Action, ActionOutcome, Availability, Capability, ObjectRef, Provider, Query, Risk, Selector,
};
use ono_value::{ErrorValue, Provenance, RecordValue, Schema, SchemaId, Value};

/// The instant every test resolves at, so §4.4's identity is reproducible without a clock.
pub const AT: &str = "2026-09-09T12:00:00Z";

/// The fixed instant tests plan at.
pub fn at() -> Timestamp {
    AT.parse().expect("a literal timestamp parses")
}

/// One of the schemas the shell ships.
pub fn schema(name: &str) -> Arc<Schema> {
    ono_value::builtin_schemas()
        .get(&SchemaId::new(name, 1))
        .unwrap_or_else(|| panic!("{name}/1 is one of the schemas the shell ships"))
}

/// A record of `name`, with the fields given.
pub fn record(name: &str, fields: &[(&str, Value)]) -> Value {
    let schema = schema(name);
    let mut builder = RecordValue::builder(
        Arc::clone(&schema),
        Provenance::local("test", schema.id().clone()),
    );
    for (field, value) in fields {
        builder = builder
            .set(field, value.clone())
            .unwrap_or_else(|error| panic!("{name}/1 has no field `{field}`: {error}"));
    }
    builder.build().into_value()
}

/// An `ono.service/1` record.
pub fn service(name: &str, state: &str) -> Value {
    record(
        "ono.service",
        &[
            ("provider", Value::string("systemd")),
            ("name", Value::string(name)),
            ("state", Value::string(state)),
        ],
    )
}

/// An `ono.package/1` record.
pub fn package(name: &str, version: &str, installed: bool) -> Value {
    record(
        "ono.package",
        &[
            ("provider", Value::string("dpkg")),
            ("name", Value::string(name)),
            ("version", Value::string(version)),
            ("installed", Value::Bool(installed)),
        ],
    )
}

/// An `ono.file/1` record.
pub fn file(path: &str) -> Value {
    record(
        "ono.file",
        &[
            ("path", Value::Path(Arc::from(std::path::Path::new(path)))),
            ("name", Value::string(path)),
            ("kind", Value::string("file")),
            ("device", Value::Int(64)),
            ("inode", Value::Int(4_711)),
        ],
    )
}

/// An `ono.process/1` record.
pub fn process(pid: i128) -> Value {
    record(
        "ono.process",
        &[
            ("pid", Value::Int(pid)),
            ("started", Value::Timestamp(at())),
            ("name", Value::string("nginx")),
            ("state", Value::string("sleeping")),
        ],
    )
}

/// An `ono.socket/1` record.
pub fn socket(port: u16) -> Value {
    record(
        "ono.socket",
        &[
            ("protocol", Value::string("tcp")),
            ("family", Value::string("inet")),
            ("local", Value::string(&format!(":{port}"))),
            ("state", Value::string("listen")),
            ("inode", Value::Int(i128::from(port))),
        ],
    )
}

/// The provider capabilities every mutating command in `plannable_operations:` names.
///
/// A fake provider advertises all of them, so §43.2's precondition holds unless a test says
/// otherwise.
pub const ALL_CAPABILITIES: &[&str] = &[
    "file.write",
    "file.copy",
    "file.move",
    "file.remove",
    "file.set",
    "service.manage",
    "process.signal",
    "process.set",
    "package.manage",
    "package-source.refresh",
    "route.set",
    "interface.set",
    "socket.close",
    "mount.manage",
    "user.manage",
    "group.manage",
    "container.manage",
];

/// A provider that answers from a fixed set of records and counts what it was asked to do.
#[derive(Debug)]
pub struct FakeProvider {
    id: String,
    targets: Vec<&'static str>,
    records: Vec<Value>,
    availability: Availability,
    capabilities: Vec<&'static str>,
    acted: Mutex<Vec<Action>>,
    fails: bool,
}

impl FakeProvider {
    /// A provider answering about `targets`.
    pub fn new(id: &str, targets: &[&'static str]) -> Self {
        Self {
            id: id.to_owned(),
            targets: targets.to_vec(),
            records: Vec::new(),
            availability: Availability::Available,
            capabilities: ALL_CAPABILITIES.to_vec(),
            acted: Mutex::new(Vec::new()),
            fails: false,
        }
    }

    /// The records `snapshot` answers with.
    #[must_use]
    pub fn holding(mut self, records: Vec<Value>) -> Self {
        self.records = records;
        self
    }

    /// A provider advertising exactly `capabilities` and nothing else.
    #[must_use]
    pub fn advertising(mut self, capabilities: &[&'static str]) -> Self {
        self.capabilities = capabilities.to_vec();
        self
    }

    /// A provider that cannot answer here.
    #[must_use]
    pub fn unavailable(mut self, reason: &str) -> Self {
        self.availability = Availability::unavailable(reason);
        self
    }

    /// A provider whose every action fails.
    #[must_use]
    pub const fn failing(mut self) -> Self {
        self.fails = true;
        self
    }

    /// How many times anything asked this provider to change something.
    pub fn act_count(&self) -> usize {
        self.acted.lock().expect("the log is not poisoned").len()
    }

    /// The actions it was asked to perform, in order.
    pub fn acted(&self) -> Vec<Action> {
        self.acted.lock().expect("the log is not poisoned").clone()
    }
}

#[async_trait::async_trait]
impl Provider for FakeProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn targets(&self) -> &[&str] {
        &self.targets
    }

    fn schemas(&self) -> Vec<Arc<Schema>> {
        Vec::new()
    }

    fn capabilities(&self) -> Vec<Capability> {
        self.capabilities
            .iter()
            .map(|id| Capability::new(*id, Risk::Mutate))
            .collect()
    }

    fn availability(&self) -> Availability {
        self.availability.clone()
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let matching: Vec<Value> = self
            .records
            .iter()
            .filter(|value| {
                value
                    .as_record()
                    .is_ok_and(|record| query.selectors().iter().all(|s| matches(s, record)))
            })
            .cloned()
            .collect();
        Ok(ValueStream::from_values(matching))
    }

    async fn resolve(&self, _selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        Ok(Vec::new())
    }

    async fn act(&self, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        self.acted
            .lock()
            .expect("the log is not poisoned")
            .push(action.clone());
        if self.fails {
            return Ok(ActionOutcome::failed(
                action,
                ErrorValue::new(
                    ono_core::ErrorCode::IoPermissionDenied,
                    "refused by the test",
                ),
            ));
        }
        Ok(ActionOutcome::succeeded(action, true))
    }
}

/// A selector narrows only where the record has the field.
///
/// That is what the real contract permits: "a provider may honour a selector by asking the system
/// for less, or ignore it and let the pipeline filter. Correctness never depends on which it
/// chose." A socket is asked for by port and carries an endpoint, and the fake behaves the way a
/// real provider does rather than the way a database would.
fn matches(selector: &Selector, record: &RecordValue) -> bool {
    match selector {
        Selector::Field { name, value } => match record.get(name) {
            None => true,
            Some(held) => {
                ono_value::canonical_text(held).ok() == ono_value::canonical_text(value).ok()
            }
        },
        other => other.matches(record),
    }
}

/// The join between a synchronous change provider and an asynchronous provider (§51).
///
/// One current-thread runtime per call: a test needs no scheduler, and building one per call
/// keeps every test independent of every other.
#[derive(Debug, Default)]
pub struct TestBridge;

impl TestBridge {
    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a current-thread runtime builds")
    }
}

impl Bridge for TestBridge {
    fn act(&self, provider: &dyn Provider, action: &Action) -> Result<ActionOutcome, ErrorValue> {
        Self::runtime().block_on(provider.act(action))
    }

    fn snapshot(&self, provider: &dyn Provider, query: &Query) -> Result<Vec<Value>, ErrorValue> {
        Self::runtime().block_on(async {
            let stream = provider.snapshot(query)?;
            drain(stream).await
        })
    }
}

/// The facts §7.2 freezes, stated by the test rather than read from the machine.
#[derive(Debug, Default)]
pub struct FakeObserver {
    facts: BTreeMap<(String, String), Value>,
    silent: Vec<(String, String)>,
    reboot: BTreeMap<String, RebootRequirement>,
}

impl FakeObserver {
    /// An observer that has seen nothing, which answers "the object is not there".
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that `field` of `subject` holds `value`.
    #[must_use]
    pub fn seeing(mut self, subject: &str, field: &str, value: Value) -> Self {
        self.facts
            .insert((subject.to_owned(), field.to_owned()), value);
        self
    }

    /// Records that `field` of `subject` could not be observed at all (§2.4).
    #[must_use]
    pub fn blind_to(mut self, subject: &str, field: &str) -> Self {
        self.silent.push((subject.to_owned(), field.to_owned()));
        self
    }

    /// Records what the provider says about a reboot for `subject` (§30.5).
    #[must_use]
    pub fn rebooting(mut self, subject: &str, answer: RebootRequirement) -> Self {
        self.reboot.insert(subject.to_owned(), answer);
        self
    }

    /// The observer a service test uses: nginx exists and is running.
    #[must_use]
    pub fn nginx() -> Self {
        Self::new()
            .seeing("nginx", "identity", Value::string("nginx"))
            .seeing("nginx", "state", Value::string("running"))
            .seeing("nginx", "cpu", Value::Float(2.0))
            .seeing("nginx", "memory", Value::Int(64))
            .seeing("/etc/nginx/nginx.conf", "sha256", Value::string("abc123"))
            .seeing(
                "/etc/nginx/nginx.conf",
                "persistence-domain",
                Value::string("tank/etc"),
            )
            .seeing(
                "/etc/nginx/nginx.conf",
                "identity",
                Value::string("/etc/nginx/nginx.conf"),
            )
    }
}

impl Observer for FakeObserver {
    fn fact(&self, subject: &str, _kind: PreconditionKind, field: &str) -> Option<Value> {
        if self.silent.iter().any(|(s, f)| s == subject && f == field) {
            return None;
        }
        Some(
            self.facts
                .get(&(subject.to_owned(), field.to_owned()))
                .cloned()
                .unwrap_or(Value::Null),
        )
    }

    fn reboot(&self, subject: &str, _operation: &str) -> RebootRequirement {
        self.reboot.get(subject).copied().unwrap_or_default()
    }
}

/// A change provider over `provider`, observing `observer`, at the fixed instant.
pub fn change_provider(
    provider: Arc<FakeProvider>,
    observer: Arc<FakeObserver>,
) -> ProviderChangeProvider {
    ProviderChangeProvider::new(provider, observer, Arc::new(TestBridge), "session-1", at())
        .expect("the embedded registries typecheck")
        .at_version("test-1")
}

/// The change provider every service test uses.
pub fn service_provider() -> (Arc<FakeProvider>, ProviderChangeProvider) {
    let provider = Arc::new(
        FakeProvider::new("linux.systemd", &["service"]).holding(vec![service("nginx", "running")]),
    );
    let change = change_provider(Arc::clone(&provider), Arc::new(FakeObserver::nginx()));
    (provider, change)
}
