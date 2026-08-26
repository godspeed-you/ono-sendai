//! A unit's processes: the one it declares as its main process, and the ones sharing its control
//! group (spec §22.2).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_provider_api::{Availability, Capability, ProviderRegistry, Query, Risk};
use ono_value::{ErrorValue, Value};

use crate::graph::Node;
use crate::kernel::process::{missing_field, proc_availability};
use crate::kernel::{lookup, procfs};
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
}

impl ServiceProcesses {
    /// A provider reading the running system's `/proc`.
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self {
            registry,
            root: PathBuf::from("/"),
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

        let (records, stream_failures) =
            match lookup::find(&self.registry, &Query::target("process")).await {
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
