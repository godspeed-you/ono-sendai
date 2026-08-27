//! The `ActionResult` of spec §11.5: what a mutating command hands back to the pipeline.

use std::fmt;
use std::sync::Arc;

use crate::{Duration, ErrorValue, Provenance, RecordValue, Value, ValueRef, action_result_schema};

/// How an action ended (spec §11.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionStatus {
    /// The action was performed.
    Success,
    /// The action was not needed, so nothing was attempted.
    Skipped,
    /// The action was attempted and did not succeed.
    Failed,
}

impl ActionStatus {
    /// Every status, in the order spec §11.5 lists them.
    pub const ALL: &'static [ActionStatus] = &[
        ActionStatus::Success,
        ActionStatus::Skipped,
        ActionStatus::Failed,
    ];

    /// The status as `where status == "failed"` spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ActionStatus::Success => "success",
            ActionStatus::Skipped => "skipped",
            ActionStatus::Failed => "failed",
        }
    }

    /// Resolves a status from its name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|status| status.as_str() == name)
    }
}

impl fmt::Display for ActionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The structured acknowledgement a mutating consumer returns instead of an exit code alone
/// (spec §11.5).
///
/// One result per target: spec §16.5 forbids collapsing `97 succeeded, 3 failed` into a single
/// boolean, so a bulk operation emits a stream of these rather than one aggregate error.
///
/// ```
/// use ono_value::{ActionResult, ActionStatus, Value, ValueRef};
/// let result = ActionResult::new(ValueRef::name("nginx.service"), "stop", ActionStatus::Success)
///     .changed(true);
/// let record = result.into_record();
/// assert_eq!(record.get("status"), Some(&Value::String("success".into())));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct ActionResult {
    target: ValueRef,
    operation: Arc<str>,
    status: ActionStatus,
    changed: bool,
    message: Option<Arc<str>>,
    error: Option<Arc<ErrorValue>>,
    duration: Duration,
}

impl ActionResult {
    /// Records that `operation` on `target` ended with `status`, changing nothing and taking no
    /// measured time until said otherwise.
    #[must_use]
    pub fn new(target: ValueRef, operation: &str, status: ActionStatus) -> Self {
        Self {
            target,
            operation: operation.into(),
            status,
            changed: false,
            message: None,
            error: None,
            duration: Duration::ZERO,
        }
    }

    /// States whether the action actually changed anything.
    #[must_use]
    pub fn changed(mut self, changed: bool) -> Self {
        self.changed = changed;
        self
    }

    /// Adds the human explanation.
    #[must_use]
    pub fn with_message(mut self, message: &str) -> Self {
        self.message = Some(message.into());
        self
    }

    /// Attaches the structured failure, which also makes the result a failure to read.
    #[must_use]
    pub fn with_error(mut self, error: ErrorValue) -> Self {
        self.error = Some(Arc::new(error));
        self
    }

    /// Records how long the action took.
    #[must_use]
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// What the action was performed on.
    #[must_use]
    pub const fn target(&self) -> &ValueRef {
        &self.target
    }

    /// What was attempted.
    #[must_use]
    pub fn operation(&self) -> &str {
        &self.operation
    }

    /// How the action ended.
    #[must_use]
    pub const fn status(&self) -> ActionStatus {
        self.status
    }

    /// Whether anything actually changed.
    #[must_use]
    pub const fn is_changed(&self) -> bool {
        self.changed
    }

    /// The human explanation, if there is one.
    #[must_use]
    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    /// The structured failure, if the action failed.
    #[must_use]
    pub fn error(&self) -> Option<&ErrorValue> {
        self.error.as_deref()
    }

    /// How long the action took.
    #[must_use]
    pub const fn duration(&self) -> Duration {
        self.duration
    }

    /// The result as a record of `ono.action-result/1`, so it flows through a pipeline like any
    /// other object (spec §11.5).
    #[must_use]
    pub fn into_record(self) -> RecordValue {
        let schema = action_result_schema();
        let provenance = Provenance::local("ono", schema.id().clone());
        let message = self.message.map_or(Value::Null, Value::String);
        let error = self.error.map_or(Value::Null, Value::Error);
        // Every name below is declared by the built-in schema; a name that stopped matching
        // would leave its field unknown, which `validate` reports and the crate's tests fail on.
        RecordValue::builder(schema, provenance)
            .set_known("target", self.target.to_value())
            .set_known("operation", Value::String(self.operation))
            .set_known("status", Value::string(self.status.as_str()))
            .set_known("changed", Value::Bool(self.changed))
            .set_known("message", message)
            .set_known("error", error)
            .set_known("duration", Value::Duration(self.duration))
            .build()
    }

    /// The result as an ordinary value.
    #[must_use]
    pub fn into_value(self) -> Value {
        self.into_record().into_value()
    }
}
