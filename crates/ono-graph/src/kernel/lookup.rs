//! Asking the object providers for the objects at the far end of a relationship.
//!
//! A relationship provider never builds an object record of its own: the process at the end of a
//! `parent` edge is whatever the process provider says it is, so a graph cannot disagree with
//! `get process` about the same machine.

use std::path::Path;
use std::sync::Arc;

use ono_core::ErrorCode;
use ono_provider_api::{ProviderRegistry, Query, Selector};
use ono_value::{ErrorValue, RecordValue, Value};

use crate::graph::Node;

/// One trace's shared view of the enumerations it repeats.
///
/// A trace expands one node at a time, and several relationship providers each need "every
/// process" to find their end of an edge. Asking procfs again for every node made a trace of
/// pid 1 cost `nodes × processes` full reads — twenty-seven seconds where spec §34 budgets
/// interactive moments in milliseconds. One trace is one observation of the machine, so its
/// expansions legitimately share one snapshot per target; the cache lives exactly as long as
/// the [`kernel_relationships`](crate::kernel_relationships) call that created it, and the next
/// trace observes afresh.
#[derive(Debug, Default)]
pub(crate) struct SharedSnapshots {
    #[allow(
        clippy::type_complexity,
        reason = "one private field, named right here"
    )]
    cached: std::sync::Mutex<
        std::collections::HashMap<String, (Vec<Arc<RecordValue>>, Vec<ErrorValue>)>,
    >,
}

impl SharedSnapshots {
    /// The one object of `target` whose `field` equals `value`, found in the shared snapshot.
    ///
    /// A pinned query — one pid, one inode — can be cheap for a provider that pushes it down,
    /// but a provider that answers by enumerating makes every such lookup a full dump. The
    /// trace has usually enumerated the target already; finding the one object in that snapshot
    /// costs nothing more.
    ///
    /// # Errors
    ///
    /// Returns the provider's error when the target cannot be answered at all.
    pub(crate) async fn one(
        &self,
        registry: &ProviderRegistry,
        target: &str,
        field: &str,
        value: &Value,
    ) -> Result<Option<Arc<RecordValue>>, ErrorValue> {
        let (records, _) = self.all(registry, target).await?;
        Ok(records
            .into_iter()
            .find(|record| record.get(field) == Some(value)))
    }

    /// Every object of `target`, enumerated once per trace.
    pub(crate) async fn all(
        &self,
        registry: &ProviderRegistry,
        target: &str,
    ) -> Result<(Vec<Arc<RecordValue>>, Vec<ErrorValue>), ErrorValue> {
        if let Some(hit) = self
            .cached
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(target)
        {
            return Ok(hit.clone());
        }
        let found = find(registry, &Query::target(target)).await?;
        self.cached
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(target.to_owned(), found.clone());
        Ok(found)
    }
}

/// Every object matching `query`, in the order its provider produced them.
///
/// # Errors
///
/// Returns the provider's error when the target cannot be answered at all. A failure concerning
/// one object arrives on the stream instead and is returned beside the records.
pub(crate) async fn find(
    registry: &ProviderRegistry,
    query: &Query,
) -> Result<(Vec<Arc<RecordValue>>, Vec<ErrorValue>), ErrorValue> {
    let collected = registry.snapshot(query)?.collect().await;
    let records = collected
        .values()
        .iter()
        .filter_map(|value| match value {
            Value::Record(record) => Some(Arc::clone(record)),
            _ => None,
        })
        // A provider may answer a selector by asking the system for less, or ignore it and leave
        // the filtering to its caller. Correctness must not depend on which it chose.
        .filter(|record| query.matches(record))
        .collect();
    Ok((records, collected.errors().to_vec()))
}

/// The one object matching `query`, or `None` when nothing does.
///
/// # Errors
///
/// See [`find`].
pub(crate) async fn one(
    registry: &ProviderRegistry,
    query: &Query,
) -> Result<Option<Arc<RecordValue>>, ErrorValue> {
    let (records, failures) = find(registry, query).await?;
    match records.into_iter().next() {
        Some(record) => Ok(Some(record)),
        None => match failures.into_iter().next() {
            Some(failure) => Err(failure),
            None => Ok(None),
        },
    }
}

/// The node for the one process with this pid.
///
/// # Errors
///
/// Returns the provider's error, or `io.not_found` when no process has that pid any more — which
/// is the ordinary outcome of asking about a process that exited while the trace was running.
pub(crate) async fn process_node(
    registry: &ProviderRegistry,
    pid: i64,
) -> Result<Node, ErrorValue> {
    let query = Query::target("process").with(Selector::field("pid", Value::Int(i128::from(pid))));
    node_of(one(registry, &query).await?, &format!("process {pid}"))
}

/// The node for the one socket with this inode.
///
/// # Errors
///
/// See [`process_node`].
pub(crate) async fn socket_node(
    registry: &ProviderRegistry,
    snapshots: &SharedSnapshots,
    inode: i64,
) -> Result<Node, ErrorValue> {
    let found = snapshots
        .one(registry, "socket", "inode", &Value::Int(i128::from(inode)))
        .await?;
    node_of(found, &format!("socket {inode}"))
}

/// The node for the file at this path.
///
/// # Errors
///
/// See [`process_node`].
pub(crate) async fn file_node(
    registry: &ProviderRegistry,
    path: &Path,
) -> Result<Node, ErrorValue> {
    let query = Query::target("file").with(Selector::field(
        "path",
        Value::Path(Arc::from(path.to_path_buf())),
    ));
    node_of(one(registry, &query).await?, &path.display().to_string())
}

/// The node for a record that was found, or the error for one that was not.
fn node_of(record: Option<Arc<RecordValue>>, what: &str) -> Result<Node, ErrorValue> {
    let Some(record) = record else {
        return Err(ErrorValue::new(
            ErrorCode::IoNotFound,
            format!("{what} is held open but no provider can describe it"),
        ));
    };
    Node::of(&record).ok_or_else(|| {
        ErrorValue::new(
            ErrorCode::ProviderSchemaViolation,
            format!("{what} has no object identity, so it cannot be a node of a graph"),
        )
    })
}
