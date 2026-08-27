//! A user's processes and groups (spec §22.3, ADR-0078).
//!
//! Both relationships are exact: a process's owner is the uid the kernel reports in
//! `/proc/<pid>/status`, and a group's members are what the account database lists. Neither
//! is inferred from a name, because names are not identity (spec §23.6): a process is related to
//! a user by uid, a user to a group by gid — and to a supplementary group by the membership the
//! group itself declares.

use std::sync::Arc;

use ono_provider_api::{Capability, ProviderRegistry, Risk};
use ono_value::{RecordValue, Value};

use crate::graph::Node;
use crate::kernel::lookup::SharedSnapshots;
use crate::kernel::process::missing_field;
use crate::provider::{Relationship, RelationshipProvider, Relationships};

/// The schema of the objects the identity relationship providers expand.
const USER: &str = "ono.user/1";

/// The processes running as a user, read from the process provider.
#[derive(Debug)]
pub struct UserProcesses {
    registry: Arc<ProviderRegistry>,
    snapshots: Arc<SharedSnapshots>,
}

impl UserProcesses {
    /// A provider reading through `registry`'s process provider.
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self {
            registry,
            snapshots: Arc::default(),
        }
    }

    pub(crate) fn sharing(mut self, snapshots: Arc<SharedSnapshots>) -> Self {
        self.snapshots = snapshots;
        self
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for UserProcesses {
    fn id(&self) -> &str {
        "linux.user-processes"
    }

    fn subjects(&self) -> &[&str] {
        &[USER]
    }

    fn relations(&self) -> &[&str] {
        &["runs"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("process.list", Risk::Read)]
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let Some(uid) = subject.integer("uid") else {
            return Relationships::failed(missing_field(subject, "uid"));
        };
        let (records, failures) = match self.snapshots.all(&self.registry, "process").await {
            Ok(found) => found,
            Err(error) => return Relationships::failed(error),
        };

        let mut owned: Vec<(i64, Node)> = records
            .iter()
            .filter(|record| reference_id(record.get("user"), "uid") == Some(uid))
            .filter_map(|record| {
                let node = Node::of(record)?;
                Some((node.integer("pid")?, node))
            })
            .collect();
        owned.sort_by_key(|(pid, _)| *pid);

        let mut found = Relationships::new();
        for failure in failures {
            found.fail(failure);
        }
        for (_, node) in owned {
            found.push(Relationship::exact(subject, node, "runs", self.id()));
        }
        found
    }
}

/// The groups a user belongs to: the primary group its account names, and every group whose
/// member list names the account.
#[derive(Debug)]
pub struct UserGroups {
    registry: Arc<ProviderRegistry>,
    snapshots: Arc<SharedSnapshots>,
}

impl UserGroups {
    /// A provider reading through `registry`'s group provider.
    #[must_use]
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self {
            registry,
            snapshots: Arc::default(),
        }
    }

    pub(crate) fn sharing(mut self, snapshots: Arc<SharedSnapshots>) -> Self {
        self.snapshots = snapshots;
        self
    }
}

#[async_trait::async_trait]
impl RelationshipProvider for UserGroups {
    fn id(&self) -> &str {
        "linux.user-groups"
    }

    fn subjects(&self) -> &[&str] {
        &[USER]
    }

    fn relations(&self) -> &[&str] {
        &["primary-group", "member-of"]
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::new("group.list", Risk::Read)]
    }

    async fn relationships(&self, subject: &Node) -> Relationships {
        let primary = reference_id(subject.field("primary_group"), "gid");
        let name = subject.text("name");
        let (records, failures) = match self.snapshots.all(&self.registry, "group").await {
            Ok(found) => found,
            Err(error) => return Relationships::failed(error),
        };

        let mut groups: Vec<(i64, &'static str, Node)> = records
            .iter()
            .filter_map(|record| {
                let node = Node::of(record)?;
                let gid = node.integer("gid")?;
                if Some(gid) == primary {
                    Some((gid, "primary-group", node))
                } else if name
                    .as_deref()
                    .is_some_and(|name| lists_member(record, name))
                {
                    Some((gid, "member-of", node))
                } else {
                    None
                }
            })
            .collect();
        groups.sort_by_key(|(gid, _, _)| *gid);

        let mut found = Relationships::new();
        for failure in failures {
            found.fail(failure);
        }
        for (_, relation, node) in groups {
            found.push(Relationship::exact(subject, node, relation, self.id()));
        }
        found
    }
}

/// The numeric id inside a `ref<…>` value such as a process's `user`, or `None` when the
/// reference is unknown, failed or of another shape.
pub(crate) fn reference_id(value: Option<&Value>, field: &str) -> Option<i64> {
    match value? {
        Value::Record(reference) => match reference.get(field)? {
            Value::Int(id) => i64::try_from(*id).ok(),
            _ => None,
        },
        _ => None,
    }
}

/// Whether a group record lists `name` among its members.
fn lists_member(group: &RecordValue, name: &str) -> bool {
    match group.get("members") {
        Some(Value::List(members)) => members
            .iter()
            .any(|member| member.as_str().ok() == Some(name)),
        _ => false,
    }
}
