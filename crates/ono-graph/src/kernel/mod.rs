//! The relationship providers that read the kernel (spec §22.2, §23).

use std::path::Path;
use std::sync::Arc;

use ono_provider_api::ProviderRegistry;

use crate::provider::RelationshipProvider;

mod dns;
mod identity;
mod lookup;
mod mount;
mod process;
mod procfs;
mod service;

pub use dns::{RemoteHosts, Resolver};
pub use identity::{UserGroups, UserProcesses};
pub use mount::{MountDevices, MountFilesystems, MountUsers};
pub use process::{OpenFiles, ProcessSockets, ProcessTree, SocketOwners};
pub use service::ServiceProcesses;

/// Every exact relationship provider, reading the running system.
///
/// This is what a shell registers: the relationships of spec §22.2, in the order a trace should
/// show them — a unit's processes before a process's sockets before its files, so the drawing
/// comes out as §22.4 draws it. Anything inferred, such as [`RemoteHosts`], is added on top by
/// whoever decides that a resolver may be consulted.
#[must_use]
pub fn kernel_relationships(registry: Arc<ProviderRegistry>) -> Vec<Arc<dyn RelationshipProvider>> {
    rooted_relationships(registry, "/")
}

/// The same, reading `<root>/proc` — how a test, a container fixture or a mounted image is
/// traced.
#[must_use]
pub fn rooted_relationships(
    registry: Arc<ProviderRegistry>,
    root: impl AsRef<Path>,
) -> Vec<Arc<dyn RelationshipProvider>> {
    let root = root.as_ref();
    // One trace is one observation: the expansions share one snapshot per target for exactly
    // the lifetime of this provider set (`lookup::SharedSnapshots`).
    let snapshots = Arc::new(lookup::SharedSnapshots::default());
    vec![
        Arc::new(
            ServiceProcesses::new(Arc::clone(&registry))
                .rooted(root)
                .sharing(Arc::clone(&snapshots)),
        ),
        Arc::new(ProcessTree::new(Arc::clone(&registry)).sharing(Arc::clone(&snapshots))),
        Arc::new(
            ProcessSockets::new(Arc::clone(&registry))
                .rooted(root)
                .sharing(Arc::clone(&snapshots)),
        ),
        Arc::new(OpenFiles::new(Arc::clone(&registry)).rooted(root)),
        Arc::new(SocketOwners::new(Arc::clone(&registry)).rooted(root)),
        Arc::new(MountDevices::new(Arc::clone(&registry))),
        Arc::new(MountFilesystems::new(Arc::clone(&registry)).sharing(Arc::clone(&snapshots))),
        Arc::new(
            MountUsers::new(Arc::clone(&registry))
                .rooted(root)
                .sharing(Arc::clone(&snapshots)),
        ),
        Arc::new(UserProcesses::new(Arc::clone(&registry)).sharing(Arc::clone(&snapshots))),
        Arc::new(UserGroups::new(registry).sharing(snapshots)),
    ]
}
