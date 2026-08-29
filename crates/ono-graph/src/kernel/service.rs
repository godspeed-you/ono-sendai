//! A unit's processes: the one it declares as its main process, and the ones sharing its control
//! group (spec §22.2).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_provider_api::{Availability, Capability, ProviderRegistry, Risk};
use ono_value::{ErrorValue, Value};

use crate::graph::Node;
use crate::kernel::process::{missing_field, proc_availability};
use crate::kernel::procfs;
use crate::provider::{Relationship, RelationshipProvider, Relationships};

/// The processes belonging to a service unit.
///
/// Both relationships are exact: the main process id comes from the service manager itself, and
/// membership of the unit's control group is read from `/proc/<pid>/cgroup`, which is the
/// kernel's own answer to the question rather than an inference from process names.
#[derive(Debug)]
pub struct ServiceProcesses {
    registry: Arc<ProviderRegistry>,
    root: PathBuf,
    snapshots: Arc<super::lookup::SharedSnapshots>,
}

impl ServiceProcesses {
    pub(crate) fn sharing(mut self, snapshots: Arc<super::lookup::SharedSnapshots>) -> Self {
        self.snapshots = snapshots;
        self
    }

    /// A provider reading the running system's `/proc`.
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self {
            registry,
            root: PathBuf::from("/"),
            snapshots: Arc::default(),
        }
    }

    /// Reads `<root>/proc` instead.
    #[must_use]
    pub fn rooted(mut self, root: impl AsRef<Path>) -> Self {
        self.root = root.as_ref().to_path_buf();
        self
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for ServiceProcesses {
    fn id(&self) -> &str {
        "linux.service-processes"
    }

    fn subjects(&self) -> &[&str] {
        &["ono.service/1"]
    }

    fn relations(&self) -> &[&str] {
        &["owns", "contains"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("service.trace", Risk::Read)]
    }

    fn availability(&self) -> Availability {
        proc_availability(&self.root)
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(unit) = subject.text("name") else {
            return Relationships::failed(missing_field(subject, "name"));
        };
        let main = subject.integer("pid");
        let proc = procfs::proc_dir(&self.root);

        let (records, stream_failures) = match self.snapshots.all(&self.registry, "process").await {
            Ok(found) => found,
            Err(error) => return Relationships::failed(error),
        };
        let mut processes: Vec<(i64, Node)> = records
            .iter()
            .filter_map(|record| {
                let node = Node::of(record)?;
                Some((node.integer("pid")?, node))
            })
            .collect();
        processes.sort_by_key(|(pid, _)| *pid);

        let mut found = Relationships::new();
        for failure in stream_failures {
            found.fail(failure);
        }
        if let Some(main) = main {
            match processes
                .iter()
                .find(|(pid, _)| *pid == main)
                .map(|(_, node)| node.clone())
            {
                Some(node) => found.push(
                    Relationship::exact(subject, node, "owns", self.id())
                        .with_metadata("pid", Value::Int(i128::from(main))),
                ),
                None => found.fail(ErrorValue::new(
                    ErrorCode::IoNotFound,
                    format!("{unit} names process {main} as its main process, but no process provider knows it"),
                )),
            }
        }

        let mut hidden = 0usize;
        for (pid, node) in &processes {
            if Some(*pid) == main {
                continue;
            }
            match procfs::cgroups(&proc, *pid) {
                Ok(paths) => {
                    if let Some(path) = paths.iter().find(|path| belongs_to(path, &unit)) {
                        found.push(
                            Relationship::exact(subject, node.clone(), "contains", self.id())
                                .with_metadata("cgroup", Value::String(path.as_str().into())),
                        );
                    }
                }
                Err(error) if error.code() == ErrorCode::IoPermissionDenied => hidden += 1,
                // A process that exited mid-scan is not a member and not a failure.
                Err(_) => continue,
            }
        }
        if hidden > 0 {
            found.fail(ErrorValue::new(
                ErrorCode::IoPermissionDenied,
                format!("{hidden} process(es) did not let their control group be read"),
            ));
        }
        found
    }
}

/// Whether a control group path is the unit's own.
///
/// The unit owns the leaf of its path — `/system.slice/nginx.service` — and everything below it,
/// which is where a unit's workers and its `Delegate=yes` sub-groups live.
fn belongs_to(cgroup: &str, unit: &str) -> bool {
    let leaf = format!("/{unit}");
    cgroup.ends_with(&leaf) || cgroup.contains(&format!("{leaf}/"))
}

/// The units a service requires: `service.depends_on` (spec v0.4 §13, ADR-0239).
///
/// The service manager declares them and the provider carries them on the record, so this
/// composes rather than observes (§2.16): each name is matched against the units the same trace
/// already enumerated. A dependency naming a unit the manager does not currently hold draws no
/// edge — an edge to a node that is not there would be the dangling reference §42.3 forbids —
/// and it is not a failure either, because a unit file may name a unit that was never installed.
#[derive(Debug)]
pub struct ServiceDependencies {
    registry: Arc<ProviderRegistry>,
    snapshots: Arc<super::lookup::SharedSnapshots>,
}

impl ServiceDependencies {
    /// A provider resolving units through `registry`'s service provider.
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self {
            registry,
            snapshots: Arc::default(),
        }
    }

    pub(crate) fn sharing(mut self, snapshots: Arc<super::lookup::SharedSnapshots>) -> Self {
        self.snapshots = snapshots;
        self
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for ServiceDependencies {
    fn id(&self) -> &str {
        "linux.service-dependencies"
    }

    fn subjects(&self) -> &[&str] {
        &["ono.service/1"]
    }

    fn relations(&self) -> &[&str] {
        &["depends-on"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("service.trace", Risk::Read)]
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(unit) = subject.text("name") else {
            return Relationships::failed(missing_field(subject, "name"));
        };
        let wanted: Vec<String> = match subject.field("dependencies") {
            Some(Value::List(units)) => units
                .iter()
                .filter_map(|value| value.as_str().ok().map(str::to_owned))
                .collect(),
            // A provider with no notion of dependencies says `null`, and there is nothing to
            // draw — not a failure (spec §35.3).
            _ => return Relationships::new(),
        };
        if wanted.is_empty() {
            return Relationships::new();
        }
        let (services, stream_failures) = match self.snapshots.all(&self.registry, "service").await
        {
            Ok(found) => found,
            Err(error) => return Relationships::failed(error),
        };
        let mut found = Relationships::new();
        for failure in stream_failures {
            found.fail(failure);
        }
        for name in wanted {
            if name == unit {
                continue;
            }
            let Some(record) = services
                .iter()
                .find(|record| record.get("name") == Some(&Value::String(name.as_str().into())))
            else {
                continue;
            };
            if let Some(node) = Node::of(record) {
                found.push(Relationship::exact(subject, node, "depends-on", self.id()));
            }
        }
        found
    }
}
