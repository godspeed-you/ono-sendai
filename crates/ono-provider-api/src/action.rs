//! Asking a provider to change something, and hearing exactly what happened.

use ono_value::{ActionResult, ActionStatus, ErrorValue, Value};

use crate::ObjectId;

/// A request to change one object.
#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    target: String,
    operation: String,
    object: ObjectId,
    label: Option<String>,
    arguments: Vec<(String, Value)>,
    dry_run: bool,
    source: Option<String>,
}

impl Action {
    /// An action on one object.
    #[must_use]
    pub fn new(target: impl Into<String>, operation: impl Into<String>, object: ObjectId) -> Self {
        Self {
            target: target.into(),
            operation: operation.into(),
            object,
            label: None,
            arguments: Vec::new(),
            dry_run: false,
            source: None,
        }
    }

    /// Records where the object was observed: the provenance `source` of the record it came
    /// from, or the path a `path` selector named (ADR-0082 §4).
    ///
    /// An identity says *which* object; for a provider whose identity is not what the system
    /// acts on — a file is `(device, inode)`, and every filesystem call takes a path — the
    /// source is how the object is found again. A provider whose identity is complete ignores it.
    #[must_use]
    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Names the object the way a person knows it — `nginx.service`, `tcp/:443` — so the
    /// outcome can say which object it was without the reader decoding an identity.
    #[must_use]
    pub fn labelled(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// The object's human label, when the caller gave one.
    #[must_use]
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// Adds an argument, such as the signal a `kill` should send.
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, value: Value) -> Self {
        self.arguments.push((name.into(), value));
        self
    }

    /// Asks the provider to report what it *would* do without doing it.
    ///
    /// Spec §11.6 wants scope calculated before a destructive fan-out executes, and §42.2 shows
    /// a plan for a destructive remote operation. Neither is possible unless a provider can be
    /// asked without being obeyed.
    #[must_use]
    pub fn as_dry_run(mut self) -> Self {
        self.dry_run = true;
        self
    }

    /// The target family the action belongs to.
    #[must_use]
    pub fn target(&self) -> &ObjectId {
        &self.object
    }

    /// The target's name, as a command names it.
    #[must_use]
    pub fn target_name(&self) -> &str {
        &self.target
    }

    /// What is being asked for.
    #[must_use]
    pub fn operation(&self) -> &str {
        &self.operation
    }

    /// Every argument, in the order it was written.
    ///
    /// A forwarding provider has to carry the arguments it did not itself declare — a `stop`'s
    /// signal, a `--timeout` — and enumerating them is the only way to carry them all.
    #[must_use]
    pub fn arguments(&self) -> &[(String, Value)] {
        &self.arguments
    }

    /// An argument's value, if it was given.
    #[must_use]
    pub fn argument(&self, name: &str) -> Option<&Value> {
        self.arguments
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value)
    }

    /// Whether the provider was asked to report rather than to act.
    #[must_use]
    pub fn is_dry_run(&self) -> bool {
        self.dry_run
    }

    /// Where the object was observed, when the caller recorded it (ADR-0082 §4).
    #[must_use]
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}

/// What a provider did, per target.
///
/// Spec §11.5 requires a structured result rather than an exit code, and spec §16.5 forbids
/// collapsing `97 succeeded, 3 failed` into one ambiguous answer. Every mutation therefore
/// answers with one of these per object, and the aggregate is derived from them rather than
/// standing in for them.
#[derive(Debug, Clone, PartialEq)]
pub struct ActionOutcome {
    target: ObjectId,
    label: Option<String>,
    operation: String,
    status: ActionStatus,
    changed: bool,
    message: Option<String>,
    error: Option<ErrorValue>,
}

impl ActionOutcome {
    /// The action succeeded.
    #[must_use]
    pub fn succeeded(action: &Action, changed: bool) -> Self {
        Self {
            target: action.target().clone(),
            label: action.label().map(ToOwned::to_owned),
            operation: action.operation().to_owned(),
            status: ActionStatus::Success,
            changed,
            message: None,
            error: None,
        }
    }

    /// The action was not needed: the object was already in the requested state.
    #[must_use]
    pub fn skipped(action: &Action, why: impl Into<String>) -> Self {
        Self {
            target: action.target().clone(),
            label: action.label().map(ToOwned::to_owned),
            operation: action.operation().to_owned(),
            status: ActionStatus::Skipped,
            changed: false,
            message: Some(why.into()),
            error: None,
        }
    }

    /// The action failed, and this is why.
    #[must_use]
    pub fn failed(action: &Action, error: ErrorValue) -> Self {
        Self {
            target: action.target().clone(),
            label: action.label().map(ToOwned::to_owned),
            operation: action.operation().to_owned(),
            status: ActionStatus::Failed,
            changed: false,
            message: Some(error.message().to_owned()),
            error: Some(error),
        }
    }

    /// Which object it was.
    #[must_use]
    pub fn target(&self) -> &ObjectId {
        &self.target
    }

    /// The short human label the object was acted on under, when one was known.
    #[must_use]
    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// What was asked for.
    #[must_use]
    pub fn operation(&self) -> &str {
        &self.operation
    }

    /// How it went.
    #[must_use]
    pub fn status(&self) -> ActionStatus {
        self.status
    }

    /// Whether anything actually changed. A successful action that changed nothing is a
    /// distinction scripts need: running `start service` twice should not report two changes.
    #[must_use]
    pub fn changed(&self) -> bool {
        self.changed
    }

    /// Whether the action succeeded.
    #[must_use]
    pub fn is_success(&self) -> bool {
        matches!(self.status, ActionStatus::Success)
    }

    /// What went wrong, when something did.
    #[must_use]
    pub fn error(&self) -> Option<&ErrorValue> {
        self.error.as_ref()
    }

    /// The outcome as the `ActionResult` record that flows through a pipeline (spec §11.5).
    #[must_use]
    pub fn into_record(self, duration: ono_value::Duration) -> ActionResult {
        // The reference names the identity, which is what `inspect` resolves, and the label a
        // person knows the object by — `ono.socket/1[620332]` says nothing, `tcp/:443` does.
        // A label the identity already shows — `/` on `ono.mount/1[/]` — adds nothing and is
        // left off (ADR-0116 §1).
        let reference = match &self.label {
            Some(label) if !self.target.shows(label) => format!("{} {label}", self.target),
            _ => self.target.to_string(),
        };
        let mut result = ActionResult::new(
            ono_value::ValueRef::name(&reference),
            &self.operation,
            self.status,
        )
        .changed(self.changed)
        .with_duration(duration);
        if let Some(message) = &self.message {
            result = result.with_message(message);
        }
        if let Some(error) = self.error {
            result = result.with_error(error);
        }
        result
    }
}
