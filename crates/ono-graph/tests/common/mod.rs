//! Fixtures the graph tests share: a `/proc` tree, providers over records a test states in full,
//! and a resolver that answers from a table rather than from the network.
//!
//! Everything faked here is the *outside world* — the kernel's files, the object providers a
//! `trace` reads through, DNS. Nothing fakes a layer of this crate (AGENTS.md §11).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    dead_code,
    reason = "a shared test fixture states its preconditions the way a test does, and not every \
              test file uses every helper"
)]

use std::collections::HashMap;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ono_graph::{Graph, Node, RelationshipProvider, Resolver, TraceOptions, Tracer};
use ono_pipeline::ValueStream;
use ono_provider_api::{Capability, ObjectRef, Provider, ProviderRegistry, Query, Risk, Selector};
use ono_value::{
    ErrorValue, MapValue, Provenance, RecordValue, Schema, SchemaId, Value, builtin_schemas,
};

/// The instant every fixture record claims to have been observed at, so an identity that
/// includes a start time is an exact expectation rather than a moving one.
pub const FIXTURE_INSTANT: i64 = 1_700_000_000;

/// A `/proc` tree a test owns. The root is the directory that *contains* `proc`, which is what
/// every provider in this workspace takes.
pub struct ProcFixture {
    scratch: ono_testkit::Scratch,
}

impl ProcFixture {
    pub fn new() -> Self {
        let scratch = ono_testkit::scratch();
        fs::create_dir_all(scratch.path().join("proc")).expect("the proc directory");
        Self { scratch }
    }

    /// The root to hand a relationship provider.
    pub fn root(&self) -> &Path {
        self.scratch.path()
    }

    fn proc(&self) -> PathBuf {
        self.scratch.path().join("proc")
    }

    /// Starts a process directory.
    pub fn process(&self, pid: i64) -> ProcessFixture {
        let dir = self.proc().join(pid.to_string());
        fs::create_dir_all(&dir).expect("the process directory");
        ProcessFixture { dir }
    }
}

/// One process inside a [`ProcFixture`].
pub struct ProcessFixture {
    dir: PathBuf,
}

impl ProcessFixture {
    /// Puts a file descriptor into the process's `fd` table, the way the kernel does: a symlink
    /// to the open file, with the open flags beside it in `fdinfo`.
    pub fn fd(self, fd: u32, target: &str, flags: u32) -> Self {
        let fds = self.dir.join("fd");
        fs::create_dir_all(&fds).expect("the fd directory");
        std::os::unix::fs::symlink(target, fds.join(fd.to_string())).expect("the fd link");
        let info = self.dir.join("fdinfo");
        fs::create_dir_all(&info).expect("the fdinfo directory");
        fs::write(
            info.join(fd.to_string()),
            format!("pos:\t0\nflags:\t0{flags:o}\nmnt_id:\t24\n"),
        )
        .expect("the fdinfo file");
        self
    }

    /// A descriptor whose open flags cannot be read, which is what an `fdinfo` owned by another
    /// user looks like.
    pub fn fd_without_flags(self, fd: u32, target: &str) -> Self {
        let fds = self.dir.join("fd");
        fs::create_dir_all(&fds).expect("the fd directory");
        std::os::unix::fs::symlink(target, fds.join(fd.to_string())).expect("the fd link");
        self
    }

    /// Puts a regular file where the `fd` directory belongs, so reading it fails for *everyone*.
    ///
    /// The permission fixture below cannot restrain root, and a test that quietly proves nothing
    /// when the suite happens to run as root — in a container, in CI — is worse than no test.
    /// This one exercises the same contract, that a source which cannot be read becomes a failure
    /// attributed to the object rather than a missing edge, and it holds for every user.
    pub fn fds_not_a_directory(self) -> Self {
        fs::write(self.dir.join("fd"), "not a directory\n").expect("the fd placeholder");
        self
    }

    /// Makes the process's `fd` directory unreadable, which is what another user's process looks
    /// like to an unprivileged shell.
    pub fn unreadable_fds(self) -> Self {
        use std::os::unix::fs::PermissionsExt as _;
        let fds = self.dir.join("fd");
        fs::create_dir_all(&fds).expect("the fd directory");
        fs::set_permissions(&fds, fs::Permissions::from_mode(0o000)).expect("the mode");
        self
    }

    /// Writes `/proc/<pid>/cgroup`.
    pub fn cgroup(self, text: &str) -> Self {
        fs::write(self.dir.join("cgroup"), format!("{text}\n")).expect("the cgroup file");
        self
    }
}

/// Restores the permissions of every unreadable fd directory, so the scratch tree can be removed.
pub fn make_readable(root: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    let proc = root.join("proc");
    let Ok(entries) = fs::read_dir(&proc) else {
        return;
    };
    for entry in entries.flatten() {
        let fds = entry.path().join("fd");
        if fds.is_dir() {
            let _ = fs::set_permissions(&fds, fs::Permissions::from_mode(0o755));
        }
    }
}

/// A provider answering for one or more targets from records a test states in full.
#[derive(Debug)]
pub struct FixtureProvider {
    id: String,
    targets: Vec<&'static str>,
    records: Vec<RecordValue>,
}

impl FixtureProvider {
    pub fn new(id: &str, targets: &[&'static str], records: Vec<RecordValue>) -> Self {
        Self {
            id: id.to_owned(),
            targets: targets.to_vec(),
            records,
        }
    }
}

#[async_trait::async_trait]
impl Provider for FixtureProvider {
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
        vec![Capability::new("fixture.list", Risk::Read)]
    }

    fn snapshot(&self, query: &Query) -> Result<ValueStream, ErrorValue> {
        let values: Vec<Value> = self
            .records
            .iter()
            .filter(|record| query.matches(record))
            .map(|record| record.clone().into_value())
            .collect();
        Ok(ValueStream::from_values(values))
    }

    async fn resolve(&self, _selector: &Selector) -> Result<Vec<ObjectRef>, ErrorValue> {
        Ok(self.records.iter().filter_map(ObjectRef::of).collect())
    }
}

/// A registry over the given providers.
pub fn registry(providers: Vec<Arc<dyn Provider>>) -> Arc<ProviderRegistry> {
    let mut registry = ProviderRegistry::new();
    for provider in providers {
        registry.register(provider);
    }
    Arc::new(registry)
}

fn schema(name: &str) -> Arc<Schema> {
    builtin_schemas()
        .get(&SchemaId::new(name, 1))
        .unwrap_or_else(|| panic!("{name} is a built-in schema"))
}

fn provenance(name: &str) -> Provenance {
    Provenance::local("fixture", SchemaId::new(name, 1))
        .observed_at(jiff::Timestamp::new(FIXTURE_INSTANT, 0).expect("a representable instant"))
}

/// A process record with the fields a relationship provider reads.
pub fn process(pid: i64, ppid: Option<i64>, name: &str) -> RecordValue {
    RecordValue::builder(schema("ono.process"), provenance("ono.process"))
        .set("pid", Value::Int(i128::from(pid)))
        .and_then(|builder| {
            builder.set(
                "ppid",
                ppid.map_or(Value::Null, |ppid| Value::Int(i128::from(ppid))),
            )
        })
        .and_then(|builder| builder.set("name", Value::String(name.into())))
        .and_then(|builder| builder.set("state", Value::String("sleeping".into())))
        .and_then(|builder| {
            builder.set(
                "started",
                Value::Timestamp(
                    jiff::Timestamp::new(FIXTURE_INSTANT, 0).expect("a representable instant"),
                ),
            )
        })
        .expect("the fixture process record")
        .build()
}

/// An endpoint sub-record.
pub fn endpoint(address: Option<&str>, port: Option<u16>) -> Value {
    RecordValue::builder(schema("ono.endpoint"), provenance("ono.endpoint"))
        .set(
            "address",
            address.map_or(Value::Null, |text| {
                Value::Ip(text.parse().expect("an address"))
            }),
        )
        .and_then(|builder| builder.set("port", port.map_or(Value::Null, Value::Port)))
        .expect("the fixture endpoint record")
        .build()
        .into_value()
}

/// A socket record.
pub fn socket(inode: i64, protocol: &str, local: Value, remote: Value, state: &str) -> RecordValue {
    RecordValue::builder(schema("ono.socket"), provenance("ono.socket"))
        .set("protocol", Value::String(protocol.into()))
        .and_then(|builder| builder.set("family", Value::String("inet".into())))
        .and_then(|builder| builder.set("local", local))
        .and_then(|builder| builder.set("remote", remote))
        .and_then(|builder| builder.set("state", Value::String(state.into())))
        .and_then(|builder| builder.set("inode", Value::Int(i128::from(inode))))
        .expect("the fixture socket record")
        .build()
}

/// A service record.
pub fn service(name: &str, pid: Option<i64>) -> RecordValue {
    RecordValue::builder(schema("ono.service"), provenance("ono.service"))
        .set("name", Value::String(name.into()))
        .and_then(|builder| builder.set("state", Value::String("active".into())))
        .and_then(|builder| builder.set("provider", Value::String("systemd".into())))
        .and_then(|builder| {
            builder.set(
                "pid",
                pid.map_or(Value::Null, |pid| Value::Int(i128::from(pid))),
            )
        })
        .expect("the fixture service record")
        .build()
}

/// A file record, as a file provider would answer for one path.
pub fn file(path: &str, kind: &str, device: i64, inode: i64) -> RecordValue {
    let name = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(path);
    RecordValue::builder(schema("ono.file"), provenance("ono.file"))
        .set("path", Value::Path(Arc::from(Path::new(path))))
        .and_then(|builder| builder.set("name", Value::String(name.into())))
        .and_then(|builder| builder.set("kind", Value::String(kind.into())))
        .and_then(|builder| builder.set("device", Value::Int(i128::from(device))))
        .and_then(|builder| builder.set("inode", Value::Int(i128::from(inode))))
        .expect("the fixture file record")
        .build()
}

/// A mount record.
pub fn mount(source: &str, target: &str, filesystem: &str) -> RecordValue {
    RecordValue::builder(schema("ono.mount"), provenance("ono.mount"))
        .set("source", Value::String(source.into()))
        .and_then(|builder| builder.set("target", Value::Path(Arc::from(Path::new(target)))))
        .and_then(|builder| builder.set("filesystem", Value::String(filesystem.into())))
        .and_then(|builder| builder.set("options", Value::List(Arc::from([]))))
        .and_then(|builder| builder.set("read_only", Value::Bool(false)))
        .expect("the fixture mount record")
        .build()
}

/// The node for a record, which is what a trace starts from.
pub fn node(record: &RecordValue) -> Node {
    Node::of(record).expect("the fixture record has an identity")
}

/// Traces from one root with one provider, with a deadline so a hung walk fails the test rather
/// than the suite.
pub async fn trace_with(
    providers: Vec<Arc<dyn RelationshipProvider>>,
    root: Node,
    options: TraceOptions,
) -> Graph {
    let mut tracer = Tracer::new().with_options(options);
    for provider in providers {
        tracer = tracer.with(provider);
    }
    tokio::time::timeout(Duration::from_secs(10), tracer.trace([root]))
        .await
        .expect("the trace finished within ten seconds")
}

/// Every edge as `relation -> target label`, in the order the graph holds them.
pub fn edges(graph: &Graph) -> Vec<String> {
    graph
        .edges()
        .iter()
        .map(|edge| {
            let label = graph
                .node(edge.to())
                .map_or_else(|| edge.to().to_string(), |node| node.label().to_owned());
            format!("{} -> {label}", edge.relation())
        })
        .collect()
}

/// A resolver answering from a table, so an inference is deterministic and offline.
#[derive(Debug, Default)]
pub struct TableResolver {
    hosts: HashMap<IpAddr, String>,
}

impl TableResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, address: &str, host: &str) -> Self {
        self.hosts
            .insert(address.parse().expect("an address"), host.to_owned());
        self
    }
}

#[async_trait::async_trait]
impl Resolver for TableResolver {
    fn id(&self) -> &str {
        "fixture.resolver"
    }

    async fn reverse(&self, address: IpAddr) -> Result<Option<String>, ErrorValue> {
        Ok(self.hosts.get(&address).cloned())
    }
}

/// A relationship a test states directly, for the relations a core provider cannot know — a
/// systemd unit dependency, or anything a KUANG/11 package would contribute (spec §31.26).
#[derive(Debug)]
pub struct StatedRelationships {
    id: String,
    subjects: Vec<&'static str>,
    relations: Vec<&'static str>,
    stated: Vec<(String, String, Node, bool)>,
}

impl StatedRelationships {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            subjects: Vec::new(),
            relations: Vec::new(),
            stated: Vec::new(),
        }
    }

    /// States that every subject whose label is `from` has `relation` to `target`.
    pub fn exact(mut self, from: &str, relation: &'static str, target: Node) -> Self {
        self.relations.push(relation);
        self.stated
            .push((from.to_owned(), relation.to_owned(), target, true));
        self
    }

    /// The same, as an inference.
    pub fn inferred(mut self, from: &str, relation: &'static str, target: Node) -> Self {
        self.relations.push(relation);
        self.stated
            .push((from.to_owned(), relation.to_owned(), target, false));
        self
    }

    pub fn about(mut self, schema: &'static str) -> Self {
        self.subjects.push(schema);
        self
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for StatedRelationships {
    fn id(&self) -> &str {
        &self.id
    }

    fn subjects(&self) -> &[&str] {
        &self.subjects
    }

    fn relations(&self) -> &[&str] {
        &self.relations
    }

    async fn relationships(&self, subject: &Node) -> ono_graph::Relationships {
        let mut found = ono_graph::Relationships::new();
        for (from, relation, target, exact) in &self.stated {
            if subject.label() != from {
                continue;
            }
            found.push(if *exact {
                ono_graph::Relationship::exact(subject, target.clone(), relation, &self.id)
            } else {
                ono_graph::Relationship::inferred(
                    subject,
                    target.clone(),
                    relation,
                    &self.id,
                    "stated by the fixture",
                )
            });
        }
        found
    }
}

/// A map with one entry, for a fixture node's identity or summary.
pub fn map(entries: &[(&str, Value)]) -> MapValue {
    let mut map = MapValue::new();
    for (key, value) in entries {
        map.insert((*key).into(), value.clone());
    }
    map
}
