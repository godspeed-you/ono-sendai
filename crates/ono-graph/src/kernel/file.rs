//! Who holds a file open (spec §22.3, ADR-0078): the inverse of [`OpenFiles`](super::OpenFiles),
//! found by scanning every process's descriptor table for the file's path.
//!
//! The comparison is by the path the kernel reports for the descriptor — `d_path`, canonical —
//! against the subject's canonicalised path, so `trace file ./held.txt` in a working directory
//! finds the same holders as the absolute spelling. A deleted-but-open file no longer has a
//! path to compare and is not traced (it is not an object any more, spec §10.5).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_provider_api::{Availability, Capability, ProviderRegistry, Risk};
use ono_value::{ErrorValue, Value};

use crate::graph::Node;
use crate::kernel::lookup::SharedSnapshots;
use crate::kernel::process::{missing_field, proc_availability};
use crate::kernel::procfs;
use crate::provider::{Relationship, RelationshipProvider, Relationships};

/// The processes holding a file open, read from every readable `/proc/<pid>/fd`.
#[derive(Debug)]
pub struct FileHolders {
    registry: Arc<ProviderRegistry>,
    root: PathBuf,
    snapshots: Arc<SharedSnapshots>,
}

impl FileHolders {
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

    pub(crate) fn sharing(mut self, snapshots: Arc<SharedSnapshots>) -> Self {
        self.snapshots = snapshots;
        self
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for FileHolders {
    fn id(&self) -> &str {
        "linux.file-holders"
    }

    fn subjects(&self) -> &[&str] {
        &["ono.file/1"]
    }

    fn relations(&self) -> &[&str] {
        &["holder"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("process.trace", Risk::Read)]
    }

    fn availability(&self) -> Availability {
        proc_availability(&self.root)
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(path) = subject.text("path") else {
            return Relationships::failed(missing_field(subject, "path"));
        };
        let path = PathBuf::from(path);
        // The kernel reports a descriptor's file by its canonical path; the subject may have been
        // named relative to the working directory or through a symlink.
        let wanted = std::fs::canonicalize(&path).unwrap_or(path);
        let wanted = wanted.to_string_lossy().into_owned();

        let proc = procfs::proc_dir(&self.root);
        let pids = match procfs::pids(&proc) {
            Ok(pids) => pids,
            Err(error) => return Relationships::failed(error),
        };

        let mut found = Relationships::new();
        let mut hidden = 0usize;
        for pid in pids {
            let descriptors = match procfs::descriptors(&proc, pid) {
                Ok(descriptors) => descriptors,
                Err(error) if error.code() == ErrorCode::IoPermissionDenied => {
                    hidden += 1;
                    continue;
                }
                // A process that exited during the scan holds nothing any more.
                Err(_) => continue,
            };
            for descriptor in descriptors {
                if descriptor.target != wanted {
                    continue;
                }
                match self
                    .snapshots
                    .one(
                        &self.registry,
                        "process",
                        "pid",
                        &Value::Int(i128::from(pid)),
                    )
                    .await
                {
                    Ok(Some(record)) => {
                        if let Some(node) = Node::of(&record) {
                            found.push(
                                Relationship::exact(subject, node, "holder", self.id())
                                    .with_metadata("fd", Value::Int(i128::from(descriptor.fd)))
                                    .with_metadata(
                                        "access",
                                        descriptor.access.map_or(Value::Null, |access| {
                                            Value::String(access.as_str().into())
                                        }),
                                    ),
                            );
                        }
                    }
                    Ok(None) => {}
                    Err(error) => found.fail(error),
                }
            }
        }
        // One line for the whole scan, as `SocketOwners` reports it: a holder this user cannot
        // see is not a holder that does not exist.
        if hidden > 0 {
            found.fail(
                ErrorValue::new(
                    ErrorCode::IoPermissionDenied,
                    format!("{hidden} process(es) did not let their open files be read"),
                )
                .with_help(
                    "holders of this file may be among them; running with more privilege would \
                     show them",
                ),
            );
        }
        found
    }
}
