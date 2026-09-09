//! Frozen targets, preconditions and drift (spec v0.6 §7).
//!
//! §2.6 and §4.3 make target membership a property of the sealed plan rather than of the world at
//! apply time: *"If four services match at resolution time and a fifth fails before execution, the
//! fifth service MUST NOT be added silently."* A [`FrozenTarget`] is therefore an identity, not a
//! selector, and the selector that produced it is kept only so `explain` can show where the set
//! came from.
//!
//! §7.2 asks each action to declare preconditions "sufficient to detect material drift", and §7.4
//! makes tolerance a contract rather than a guess: a [`Precondition`] is material by default, and
//! a provider marks the ones that are not.

use std::sync::Arc;

use ono_value::Value;

use crate::vocab::vocabulary;

vocabulary! {
    /// What a precondition is about, so a refusal can say which fact moved (§7.2).
    PreconditionKind {
        Existence => "existence", "§7.2: the object still exists and is still this object.";
        ContentDigest => "content-digest", "§7.2: the file's hash still equals what was resolved.";
        Generation => "generation", "§7.2: the service's generation or state still equals what was resolved.";
        Version => "version", "§7.2: the installed version still equals what was resolved.";
        PersistenceDomain => "persistence-domain", "§7.2: the path still resolves to the same dataset or subvolume (Appendix B).";
        ProviderAvailable => "provider-available", "§7.2: the recovery provider is still available.";
        Capability => "capability", "§43.2: the capability the action needs is still held.";
        Field => "field", "§7.2: some other declared field still holds its resolved value.";
    }
}

/// One object a plan will act on, frozen at resolution (§7.1).
#[derive(Debug, Clone, PartialEq)]
pub struct FrozenTarget {
    schema: Arc<str>,
    identity: Arc<str>,
    label: Arc<str>,
    spatial_id: Option<Arc<str>>,
    host: Option<Arc<str>>,
    selector: Option<Arc<str>>,
    persistence_domain: Option<Arc<str>>,
}

impl FrozenTarget {
    /// Freezes the object `identity` of type `schema`, shown to a person as `label`.
    #[must_use]
    pub fn new(
        schema: impl Into<Arc<str>>,
        identity: impl Into<Arc<str>>,
        label: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            schema: schema.into(),
            identity: identity.into(),
            label: label.into(),
            spatial_id: None,
            host: None,
            selector: None,
            persistence_domain: None,
        }
    }

    /// Records the v0.4 spatial identity of the same object, where it has one.
    #[must_use]
    pub fn at_place(mut self, spatial_id: impl Into<Arc<str>>) -> Self {
        self.spatial_id = Some(spatial_id.into());
        self
    }

    /// Records the host the target lives on, which is part of a remote target (§7.1).
    #[must_use]
    pub fn on_host(mut self, host: impl Into<Arc<str>>) -> Self {
        self.host = Some(host.into());
        self
    }

    /// Records the selector this target was resolved from, for `explain` (§4.3).
    #[must_use]
    pub fn resolved_from(mut self, selector: impl Into<Arc<str>>) -> Self {
        self.selector = Some(selector.into());
        self
    }

    /// Records the persistence domain the target's state actually lives in (Appendix B).
    #[must_use]
    pub fn in_domain(mut self, domain: impl Into<Arc<str>>) -> Self {
        self.persistence_domain = Some(domain.into());
        self
    }

    /// The schema of the object.
    #[must_use]
    pub fn schema(&self) -> &str {
        &self.schema
    }

    /// The identity, rendered canonically.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// The label a person reads.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The spatial identity, where the object has one.
    #[must_use]
    pub fn spatial_id(&self) -> Option<&str> {
        self.spatial_id.as_deref()
    }

    /// The host, for a remote target.
    #[must_use]
    pub fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }

    /// The selector the target came from.
    #[must_use]
    pub fn selector(&self) -> Option<&str> {
        self.selector.as_deref()
    }

    /// The persistence domain the target's state lives in (Appendix B).
    #[must_use]
    pub fn persistence_domain(&self) -> Option<&str> {
        self.persistence_domain.as_deref()
    }

    /// The canonical text this target contributes to a plan digest (§4.4).
    #[must_use]
    pub fn digest_text(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}",
            self.schema,
            self.identity,
            self.host.as_deref().unwrap_or("")
        )
    }
}

/// One fact that must still hold at apply time (§7.2).
#[derive(Debug, Clone, PartialEq)]
pub struct Precondition {
    kind: PreconditionKind,
    subject: Arc<str>,
    field: Arc<str>,
    expected: Value,
    material: bool,
    detail: Arc<str>,
}

impl Precondition {
    /// Declares that `field` of `subject` must still equal `expected`.
    ///
    /// Material by default. §7.4 requires a provider to declare the fields that do *not*
    /// invalidate a plan, so the safe reading is the one you get by saying nothing.
    #[must_use]
    pub fn new(
        kind: PreconditionKind,
        subject: impl Into<Arc<str>>,
        field: impl Into<Arc<str>>,
        expected: Value,
    ) -> Self {
        Self {
            kind,
            subject: subject.into(),
            field: field.into(),
            expected,
            material: true,
            detail: Arc::from(""),
        }
    }

    /// Declares that a change to this field does not invalidate the plan (§7.4).
    #[must_use]
    pub const fn tolerant(mut self) -> Self {
        self.material = false;
        self
    }

    /// Adds the sentence a refusal shows.
    #[must_use]
    pub fn explained(mut self, detail: impl Into<Arc<str>>) -> Self {
        self.detail = detail.into();
        self
    }

    /// What the precondition is about.
    #[must_use]
    pub const fn kind(&self) -> PreconditionKind {
        self.kind
    }

    /// The object the fact is about.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// The field.
    #[must_use]
    pub fn field(&self) -> &str {
        &self.field
    }

    /// The value resolution froze.
    #[must_use]
    pub const fn expected(&self) -> &Value {
        &self.expected
    }

    /// Whether a change here stops the apply (§7.3).
    #[must_use]
    pub const fn is_material(&self) -> bool {
        self.material
    }

    /// The sentence a refusal shows.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }

    /// Compares `observed` against what was frozen (§7.3).
    ///
    /// An observation Ono could not make is [`DriftVerdict::Unknown`], never a pass: §2.4 forbids
    /// promoting unknown to expected, and a precondition that cannot be checked has not held.
    #[must_use]
    pub fn check(&self, observed: Option<&Value>) -> DriftVerdict {
        match observed {
            None => DriftVerdict::Unknown,
            Some(value) if value.same_data(&self.expected) => DriftVerdict::Unchanged,
            Some(_) if self.material => DriftVerdict::Material,
            Some(_) => DriftVerdict::Tolerated,
        }
    }

    /// The canonical text this precondition contributes to a plan digest (§4.4).
    #[must_use]
    pub fn digest_text(&self) -> String {
        format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.kind.as_str(),
            self.subject,
            self.field,
            crate::digest::value_text(&self.expected),
            u8::from(self.material),
        )
    }
}

/// What revalidation found about one precondition (§7.3, §7.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftVerdict {
    /// The fact still holds.
    Unchanged,
    /// The fact moved, and the contract declared the move harmless (§7.4).
    Tolerated,
    /// The fact moved materially, and §7.3 stops the apply.
    Material,
    /// The fact could not be observed, which is not a pass (§2.4).
    Unknown,
}

impl DriftVerdict {
    /// Whether this verdict prevents the plan from proceeding (§7.3).
    #[must_use]
    pub const fn blocks_apply(self) -> bool {
        matches!(self, DriftVerdict::Material | DriftVerdict::Unknown)
    }
}

/// One precondition that did not hold, and what was seen instead (§7.3).
#[derive(Debug, Clone, PartialEq)]
pub struct DriftFinding {
    subject: Arc<str>,
    field: Arc<str>,
    kind: PreconditionKind,
    verdict: DriftVerdict,
    expected: Value,
    observed: Option<Value>,
}

impl DriftFinding {
    /// Records what revalidation found for one precondition.
    #[must_use]
    pub fn new(
        precondition: &Precondition,
        verdict: DriftVerdict,
        observed: Option<Value>,
    ) -> Self {
        Self {
            subject: Arc::from(precondition.subject()),
            field: Arc::from(precondition.field()),
            kind: precondition.kind(),
            verdict,
            expected: precondition.expected().clone(),
            observed,
        }
    }

    /// The object the finding is about.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// The field that was checked.
    #[must_use]
    pub fn field(&self) -> &str {
        &self.field
    }

    /// What the precondition was about.
    #[must_use]
    pub const fn kind(&self) -> PreconditionKind {
        self.kind
    }

    /// What revalidation found.
    #[must_use]
    pub const fn verdict(&self) -> DriftVerdict {
        self.verdict
    }

    /// The value the plan froze.
    #[must_use]
    pub const fn expected(&self) -> &Value {
        &self.expected
    }

    /// The value revalidation saw, where it saw one.
    #[must_use]
    pub const fn observed(&self) -> Option<&Value> {
        self.observed.as_ref()
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    fn hash_precondition() -> Precondition {
        Precondition::new(
            PreconditionKind::ContentDigest,
            "/etc/nginx/nginx.conf",
            "sha256",
            Value::string("abc123"),
        )
    }

    #[test]
    fn should_pass_when_the_frozen_fact_still_holds() {
        assert_eq!(
            hash_precondition().check(Some(&Value::string("abc123"))),
            DriftVerdict::Unchanged
        );
    }

    #[test]
    fn should_stop_the_apply_when_a_material_fact_moved() {
        let verdict = hash_precondition().check(Some(&Value::string("def456")));
        assert_eq!(
            verdict,
            DriftVerdict::Material,
            "§55.2 case 6: a file changed after seal means apply refuses"
        );
        assert!(verdict.blocks_apply());
    }

    #[test]
    fn should_let_a_declared_tolerance_through() {
        // §7.4's own example: CPU usage moves while a service restart is planned.
        let cpu = Precondition::new(
            PreconditionKind::Field,
            "nginx.service",
            "cpu",
            Value::Float(2.0),
        )
        .tolerant();
        let verdict = cpu.check(Some(&Value::Float(41.0)));
        assert_eq!(
            verdict,
            DriftVerdict::Tolerated,
            "§55.2 case 8: non-material CPU drift leaves the plan valid"
        );
        assert!(!verdict.blocks_apply());
    }

    #[test]
    fn should_treat_an_unobservable_precondition_as_blocking_rather_than_as_a_pass() {
        let verdict = hash_precondition().check(None);
        assert_eq!(
            verdict,
            DriftVerdict::Unknown,
            "§2.4: unknown MUST NOT be silently promoted to expected"
        );
        assert!(
            verdict.blocks_apply(),
            "a precondition nobody could check has not held"
        );
    }

    #[test]
    fn should_default_a_precondition_to_material() {
        assert!(
            hash_precondition().is_material(),
            "§7.4: tolerance must be contract-declared, not guessed"
        );
    }

    #[test]
    fn should_make_a_target_on_another_host_a_different_target() {
        let here = FrozenTarget::new("ono.service/1", "nginx.service", "nginx");
        let there = here.clone().on_host("api-04");
        assert_ne!(
            here.digest_text(),
            there.digest_text(),
            "§7.1: host identity is part of a remote target"
        );
    }

    #[test]
    fn should_keep_the_selector_a_target_came_from_without_re_running_it() {
        let target = FrozenTarget::new("ono.service/1", "nginx.service", "nginx")
            .resolved_from("get service | where state == failed");
        assert_eq!(
            target.selector(),
            Some("get service | where state == failed"),
            "§4.3 freezes the set; the selector survives only as provenance"
        );
        assert!(
            !target.digest_text().contains("where"),
            "the seal is over identities, so re-spelling the selector is not a new plan"
        );
    }
}
