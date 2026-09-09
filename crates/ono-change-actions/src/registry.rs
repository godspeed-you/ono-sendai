//! The operations v0.6 can plan, read from `docs/contracts/change/actions.yaml` (spec §6.1).
//!
//! §6.1 makes an operation plannable only where Ono can resolve it to a provider contract that
//! declares nine things. Six of them are already stated somewhere else and are not restated
//! here: `docs/contracts/commands/*.yaml` declares the target schema, the required provider
//! capability, the privilege and the arguments; `docs/contracts/verbs.yaml` says which verbs
//! mutate; [`ono_provider_api::Provider::act`] is the execution method. The three that cannot be
//! derived from any of those — the expected direct effects, the idempotency class and the
//! recovery semantics — are facts about the operation rather than about its spelling, so they
//! are data, and this module is what reads them.
//!
//! **An operation with no row is not plannable.** That is §6.2's answer for an arbitrary command,
//! and it is the same answer for a mutating command nobody has written a contract for yet:
//! [`OperationRegistry::get`] returns `None` and the caller refuses. Silence is never a claim
//! that a change is safe.
//!
//! The document is embedded at build time as JSON, the way `ono-command` and `ono-value` embed
//! theirs. §52.1 budgets plan creation at under 150 ms, and a YAML parse on the way to the first
//! plan would spend a visible part of it on text that cannot change between builds (ADR-0571).

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use ono_change_core::{
    ActionRole, EffectConfidence, EffectDomain, EffectKind, Idempotency, PreconditionKind,
    VerificationClass, error,
};
use ono_value::ErrorValue;
use serde::Deserialize;

/// The registry document, as `build.rs` transcoded it from YAML.
const ACTIONS: &str = include_str!(concat!(env!("OUT_DIR"), "/actions.json"));

static EMBEDDED: OnceLock<Result<OperationRegistry, String>> = OnceLock::new();

/// Whether an operation has a reboot dimension at all, and who answers it (§30.5).
///
/// §30.5 requires a provider to distinguish a reboot *requirement* from a *recommendation*.
/// [`RebootDimension::ProviderReported`] marks the operations where that question is meaningful;
/// [`RebootDimension::None`] says the operation has no reboot dimension, which is a different
/// statement from a provider that has one and cannot answer it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebootDimension {
    /// The operation cannot cause a reboot to be needed, so nothing is asked.
    None,
    /// The provider answers, and its answer distinguishes requirement from recommendation.
    ProviderReported,
}

/// What a provider says about a reboot after one operation (§30.5).
///
/// `Unknown` is the answer of a provider that has the dimension and cannot say, and it is
/// deliberately distinct from [`RebootDimension::None`]. §2.4 forbids promoting unknown to
/// anything, and a package manager that does not track reboot flags has not thereby told anyone
/// that no reboot is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RebootRequirement {
    /// The provider says the change is not fully in effect until the machine reboots.
    Required,
    /// The provider recommends a reboot and does not require one.
    Recommended,
    /// The provider says no reboot is involved.
    NotNeeded,
    /// The provider has the dimension and cannot answer it.
    #[default]
    Unknown,
}

impl RebootRequirement {
    /// The spelling a rendered plan and a metadata field carry.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            RebootRequirement::Required => "required",
            RebootRequirement::Recommended => "recommended",
            RebootRequirement::NotNeeded => "not-needed",
            RebootRequirement::Unknown => "unknown",
        }
    }

    /// The confidence an effect describing this answer carries (§8.1).
    ///
    /// A requirement is `EXPECTED`: the provider has strong semantics for it and the machine is
    /// still free to behave otherwise. A recommendation is `POSSIBLE`, because the provider has
    /// documented the effect as possible and cannot assert it. An answer the provider does not
    /// have is `UNKNOWN`, which §2.4 keeps visible rather than reading as "no reboot".
    #[must_use]
    pub const fn confidence(self) -> Option<EffectConfidence> {
        match self {
            RebootRequirement::Required => Some(EffectConfidence::Expected),
            RebootRequirement::Recommended => Some(EffectConfidence::Possible),
            RebootRequirement::NotNeeded => None,
            RebootRequirement::Unknown => Some(EffectConfidence::Unknown),
        }
    }

    /// The sentence a person reads beside the effect.
    #[must_use]
    pub const fn explanation(self) -> &'static str {
        match self {
            RebootRequirement::Required => {
                "§30.5: the provider requires a reboot before the change is fully in effect"
            }
            RebootRequirement::Recommended => {
                "§30.5: the provider recommends a reboot and does not require one"
            }
            RebootRequirement::NotNeeded => "the provider says no reboot is involved",
            RebootRequirement::Unknown => {
                "§30.5: the provider cannot say whether a reboot is needed, which stays unknown \
                 rather than being read as no (§2.4)"
            }
        }
    }
}

/// One expected direct effect of an operation, as §8.2 shapes it.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectSpec {
    object: Option<Arc<str>>,
    domain: EffectDomain,
    kind: EffectKind,
    confidence: EffectConfidence,
    irreversible: bool,
    compensation: Option<Arc<str>>,
    explanation: Arc<str>,
}

impl EffectSpec {
    /// Declares one effect, for an adapter stating its own contract (§6.2's second sentence).
    ///
    /// `irreversible` starts as [`EffectKind::is_inherently_irreversible`], so an `emit` is
    /// irreversible without anybody remembering to say so (§35.1).
    #[must_use]
    pub fn new(
        domain: EffectDomain,
        kind: EffectKind,
        confidence: EffectConfidence,
        explanation: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            object: None,
            domain,
            kind,
            confidence,
            irreversible: kind.is_inherently_irreversible(),
            compensation: None,
            explanation: explanation.into(),
        }
    }

    /// Names the selector whose value the effect lands on, where it is not the action's target.
    ///
    /// `copy file <source> to <destination>` acts on the source and creates the destination, and
    /// §8.2's `object` is the thing that changed. Without this the plan would show the effect
    /// against the file it read.
    #[must_use]
    pub fn on_selector(mut self, selector: impl Into<Arc<str>>) -> Self {
        self.object = Some(selector.into());
        self
    }

    /// The selector whose value the effect lands on, where the row names one.
    #[must_use]
    pub fn object_selector(&self) -> Option<&str> {
        self.object.as_deref()
    }

    /// Marks the effect as one no recovery asset undoes (§2.13).
    #[must_use]
    pub const fn irreversible(mut self) -> Self {
        self.irreversible = true;
        self
    }

    /// Names the inverse action that restores an acceptable semantic state (§27.4).
    #[must_use]
    pub fn compensated_by(mut self, action: impl Into<Arc<str>>) -> Self {
        self.compensation = Some(action.into());
        self
    }

    /// The domain the effect acts in (Appendix A.1).
    #[must_use]
    pub const fn domain(&self) -> EffectDomain {
        self.domain
    }

    /// What the effect does to its object.
    #[must_use]
    pub const fn kind(&self) -> EffectKind {
        self.kind
    }

    /// How strongly Ono asserts it (§8.1).
    #[must_use]
    pub const fn confidence(&self) -> EffectConfidence {
        self.confidence
    }

    /// Whether no recovery asset undoes it (§2.13).
    #[must_use]
    pub const fn is_irreversible(&self) -> bool {
        self.irreversible
    }

    /// The inverse action that restores an acceptable semantic state (§27.4).
    #[must_use]
    pub fn compensation(&self) -> Option<&str> {
        self.compensation.as_deref()
    }

    /// The sentence a person reads (§8.2).
    #[must_use]
    pub fn explanation(&self) -> &str {
        &self.explanation
    }
}

/// One verification contract the operation always carries (§23.1).
#[derive(Debug, Clone, PartialEq)]
pub struct VerificationSpec {
    class: VerificationClass,
    subject: Arc<str>,
    expression: Arc<str>,
}

impl VerificationSpec {
    /// Declares one contract, for an adapter stating its own verification options (§6.1).
    #[must_use]
    pub fn new(
        class: VerificationClass,
        subject: impl Into<Arc<str>>,
        expression: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            class,
            subject: subject.into(),
            expression: expression.into(),
        }
    }

    /// How much a failure says about the plan (§23.2).
    #[must_use]
    pub const fn class(&self) -> VerificationClass {
        self.class
    }

    /// The noun a person reads, where the plan names no target.
    #[must_use]
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// The condition, as a person and a machine both read it.
    ///
    /// `{option:<name>}` is filled from the resolved argument of that name, and a contract whose
    /// substitution has no value is not emitted: §23.1 asks for checks that can be answered, and
    /// a check against an argument nobody gave is not one.
    #[must_use]
    pub fn expression(&self) -> &str {
        &self.expression
    }
}

/// A field whose movement leaves the plan valid (§7.4).
#[derive(Debug, Clone, PartialEq)]
pub struct Tolerance {
    field: Arc<str>,
    doc: Arc<str>,
}

impl Tolerance {
    /// The field that may move.
    #[must_use]
    pub fn field(&self) -> &str {
        &self.field
    }

    /// Why its movement does not invalidate the plan.
    #[must_use]
    pub fn doc(&self) -> &str {
        &self.doc
    }
}

/// An argument that changes the operation's idempotency class (§41.1).
///
/// Appending to a file twice is not appending to it once, and §41.2 forbids blindly rerunning an
/// action whose class does not permit it. The class therefore has to be able to depend on the
/// argument that changes the semantics, rather than on the command's name alone.
#[derive(Debug, Clone, PartialEq)]
pub struct IdempotencyOverride {
    option: Arc<str>,
    value: bool,
    idempotency: Idempotency,
    doc: Arc<str>,
}

impl IdempotencyOverride {
    /// The option that decides it.
    #[must_use]
    pub fn option(&self) -> &str {
        &self.option
    }

    /// The value the option must hold for the override to apply.
    #[must_use]
    pub const fn value(&self) -> bool {
        self.value
    }

    /// The class that applies then.
    #[must_use]
    pub const fn idempotency(&self) -> Idempotency {
        self.idempotency
    }

    /// Why the class differs.
    #[must_use]
    pub fn doc(&self) -> &str {
        &self.doc
    }
}

/// One plannable operation: §6.1's list, as data.
#[derive(Debug, Clone, PartialEq)]
pub struct Operation {
    id: Arc<str>,
    role: ActionRole,
    idempotency: Idempotency,
    idempotency_when: Vec<IdempotencyOverride>,
    reboot: RebootDimension,
    recovery_semantics: Arc<str>,
    preconditions: Vec<PreconditionKind>,
    tolerances: Vec<Tolerance>,
    effects: Vec<EffectSpec>,
    verification: Vec<VerificationSpec>,
}

impl Operation {
    /// The command id this row is about, as `docs/contracts/commands/` spells it.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// What the action is for (§3.3).
    #[must_use]
    pub const fn role(&self) -> ActionRole {
        self.role
    }

    /// The idempotency class when no argument overrides it (§41.1).
    #[must_use]
    pub const fn idempotency(&self) -> Idempotency {
        self.idempotency
    }

    /// The idempotency class for the arguments actually given (§41.1).
    ///
    /// `arguments` is looked up by name; an override applies when the option is present with the
    /// value the row names. Where several apply, the weakest wins, because a class that permits
    /// a blind rerun has to be earned by every argument rather than by one of them (§41.2).
    #[must_use]
    pub fn idempotency_for(&self, arguments: &[(Arc<str>, ono_value::Value)]) -> Idempotency {
        let mut class = self.idempotency;
        for declared in &self.idempotency_when {
            let matched = arguments.iter().any(|(name, value)| {
                name.as_ref() == declared.option.as_ref()
                    && matches!(value, ono_value::Value::Bool(flag) if *flag == declared.value)
            });
            if matched && retry_rank(declared.idempotency) > retry_rank(class) {
                class = declared.idempotency;
            }
        }
        class
    }

    /// Every declared idempotency override.
    #[must_use]
    pub fn idempotency_overrides(&self) -> &[IdempotencyOverride] {
        &self.idempotency_when
    }

    /// Whether the operation has a reboot dimension, and who answers it (§30.5).
    #[must_use]
    pub const fn reboot(&self) -> RebootDimension {
        self.reboot
    }

    /// The recovery semantics, or the explicit statement that there are none (§6.1).
    #[must_use]
    pub fn recovery_semantics(&self) -> &str {
        &self.recovery_semantics
    }

    /// The facts §7.2 needs to detect material drift.
    #[must_use]
    pub fn preconditions(&self) -> &[PreconditionKind] {
        &self.preconditions
    }

    /// The fields whose movement §7.4 tolerates.
    #[must_use]
    pub fn tolerances(&self) -> &[Tolerance] {
        &self.tolerances
    }

    /// The expected direct effects (§6.1, §8.2).
    #[must_use]
    pub fn effects(&self) -> &[EffectSpec] {
        &self.effects
    }

    /// The verification options §23.1 requires the plan to carry.
    #[must_use]
    pub fn verification(&self) -> &[VerificationSpec] {
        &self.verification
    }
}

/// Every operation v0.6 can plan (§6.1).
#[derive(Debug, Clone, PartialEq)]
pub struct OperationRegistry {
    operations: Vec<Operation>,
    by_id: BTreeMap<String, usize>,
}

impl OperationRegistry {
    /// The registry compiled into this binary.
    ///
    /// Parsed once, on first use. A malformed embedded document is a build defect rather than a
    /// runtime condition, so it is reported rather than papered over with an empty registry —
    /// an empty registry would make every operation unplannable, which reads as a spec-conformant
    /// refusal and is in fact a broken build.
    ///
    /// # Errors
    ///
    /// `change.record_malformed` when the embedded document does not typecheck against the
    /// vocabularies of `ono-change-core`.
    pub fn embedded() -> Result<&'static Self, ErrorValue> {
        match EMBEDDED.get_or_init(|| Self::load(ACTIONS).map_err(|e| e.message().to_owned())) {
            Ok(registry) => Ok(registry),
            Err(message) => Err(error::record_malformed(
                "plannable_operations",
                &format!(
                    "{message}. The operation registry is compiled into the binary; this is a \
                     build defect"
                ),
            )),
        }
    }

    /// Parses one registry document.
    ///
    /// # Errors
    ///
    /// `change.record_malformed` when the document is not the shape `plannable_operations:`
    /// declares, or names a word outside one of v0.6's closed vocabularies.
    pub fn load(document: &str) -> Result<Self, ErrorValue> {
        let raw: RawDocument = serde_json::from_str(document)
            .map_err(|error| error::record_malformed("plannable_operations", &error.to_string()))?;
        let mut operations = Vec::with_capacity(raw.plannable_operations.len());
        let mut by_id = BTreeMap::new();
        for row in raw.plannable_operations {
            let operation = row.into_operation()?;
            if by_id
                .insert(operation.id().to_owned(), operations.len())
                .is_some()
            {
                return Err(error::record_malformed(
                    "plannable_operations",
                    &format!(
                        "`{}` is declared twice, and two contracts for one operation is two \
                         answers to §6.1's question",
                        operation.id()
                    ),
                ));
            }
            operations.push(operation);
        }
        Ok(Self { operations, by_id })
    }

    /// Every operation, in declaration order.
    #[must_use]
    pub fn operations(&self) -> &[Operation] {
        &self.operations
    }

    /// How many operations v0.6 can plan.
    #[must_use]
    pub fn len(&self) -> usize {
        self.operations.len()
    }

    /// Whether nothing is plannable, which only a broken build produces.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.operations.is_empty()
    }

    /// One operation by its command id, or `None` where §6.1 has no contract for it.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Operation> {
        self.by_id.get(id).map(|index| &self.operations[*index])
    }

    /// Whether `id` is plannable at all (§6.1, §6.2).
    #[must_use]
    pub fn is_plannable(&self, id: &str) -> bool {
        self.by_id.contains_key(id)
    }
}

/// How little a class permits, so two of them can be combined without a strengthening step.
///
/// §41.2 forbids blindly rerunning an unknown or non-idempotent action, so composing two
/// statements about one action can only ever weaken the class — the same one-directional shape
/// [`ono_change_core::EffectConfidence::weakest_of`] has for §8.1.
const fn retry_rank(class: Idempotency) -> u8 {
    match class {
        Idempotency::Idempotent => 0,
        Idempotency::RetrySafeWithToken => 1,
        Idempotency::NonIdempotent => 2,
        Idempotency::Unknown => 3,
    }
}

#[derive(Debug, Deserialize)]
struct RawDocument {
    #[serde(default)]
    plannable_operations: Vec<RawOperation>,
}

#[derive(Debug, Deserialize)]
struct RawOperation {
    id: String,
    role: String,
    idempotency: String,
    #[serde(default)]
    idempotency_when: Vec<RawOverride>,
    #[serde(default)]
    reboot: Option<String>,
    recovery_semantics: String,
    #[serde(default)]
    preconditions: Vec<String>,
    #[serde(default)]
    tolerances: Vec<RawTolerance>,
    #[serde(default)]
    effects: Vec<RawEffect>,
    #[serde(default)]
    verification: Vec<RawVerification>,
}

#[derive(Debug, Deserialize)]
struct RawOverride {
    option: String,
    value: bool,
    idempotency: String,
    doc: String,
}

#[derive(Debug, Deserialize)]
struct RawTolerance {
    field: String,
    doc: String,
}

#[derive(Debug, Deserialize)]
struct RawEffect {
    #[serde(default)]
    object: Option<String>,
    domain: String,
    kind: String,
    confidence: String,
    irreversible: bool,
    #[serde(default)]
    compensation: Option<String>,
    explanation: String,
}

#[derive(Debug, Deserialize)]
struct RawVerification {
    class: String,
    subject: String,
    expression: String,
}

/// Reads one word of a closed vocabulary, naming the row and the field when it is not in the list.
fn word<T>(
    operation: &str,
    field: &str,
    name: &str,
    read: impl Fn(&str) -> Option<T>,
) -> Result<T, ErrorValue> {
    read(name).ok_or_else(|| {
        error::record_malformed(
            field,
            &format!("`{operation}` names `{name}`, which is not a word v0.6 defines for it"),
        )
    })
}

impl RawOperation {
    fn into_operation(self) -> Result<Operation, ErrorValue> {
        let id = self.id;
        let role = word(&id, "role", &self.role, ActionRole::from_name)?;
        if !role.mutates_target() {
            return Err(error::record_malformed(
                "role",
                &format!(
                    "`{id}` is declared in role `{role}`, and §6.1's list is about operations \
                     that change the system"
                ),
            ));
        }
        let idempotency = word(
            &id,
            "idempotency",
            &self.idempotency,
            Idempotency::from_name,
        )?;
        let reboot = match self.reboot.as_deref() {
            None => RebootDimension::None,
            Some("provider-reported") => RebootDimension::ProviderReported,
            Some(other) => {
                return Err(error::record_malformed(
                    "reboot",
                    &format!(
                        "`{id}` names `{other}`; §30.5's dimension is either absent or reported \
                         by the provider"
                    ),
                ));
            }
        };
        let mut idempotency_when = Vec::with_capacity(self.idempotency_when.len());
        for raw in self.idempotency_when {
            idempotency_when.push(IdempotencyOverride {
                option: Arc::from(raw.option.as_str()),
                value: raw.value,
                idempotency: word(
                    &id,
                    "idempotency_when",
                    &raw.idempotency,
                    Idempotency::from_name,
                )?,
                doc: Arc::from(raw.doc.as_str()),
            });
        }
        let mut preconditions = Vec::with_capacity(self.preconditions.len());
        for name in &self.preconditions {
            preconditions.push(word(
                &id,
                "preconditions",
                name,
                PreconditionKind::from_name,
            )?);
        }
        let mut effects = Vec::with_capacity(self.effects.len());
        for raw in self.effects {
            effects.push(EffectSpec {
                object: raw.object.map(|name| Arc::from(name.as_str())),
                domain: word(&id, "effects.domain", &raw.domain, EffectDomain::from_name)?,
                kind: word(&id, "effects.kind", &raw.kind, EffectKind::from_name)?,
                confidence: word(
                    &id,
                    "effects.confidence",
                    &raw.confidence,
                    EffectConfidence::from_name,
                )?,
                irreversible: raw.irreversible,
                compensation: raw.compensation.map(|text| Arc::from(text.as_str())),
                explanation: Arc::from(raw.explanation.as_str()),
            });
        }
        if effects.is_empty() {
            return Err(error::record_malformed(
                "effects",
                &format!(
                    "`{id}` declares no effects, and §6.1 makes expected direct effects part of \
                     what a plannable operation states"
                ),
            ));
        }
        let mut verification = Vec::with_capacity(self.verification.len());
        for raw in self.verification {
            verification.push(VerificationSpec {
                class: word(
                    &id,
                    "verification.class",
                    &raw.class,
                    VerificationClass::from_name,
                )?,
                subject: Arc::from(raw.subject.as_str()),
                expression: Arc::from(raw.expression.as_str()),
            });
        }
        if verification.is_empty() {
            return Err(error::record_malformed(
                "verification",
                &format!(
                    "`{id}` declares no verification, and §23.1 requires every plan that mutates \
                     to carry at least one contract"
                ),
            ));
        }
        Ok(Operation {
            id: Arc::from(id.as_str()),
            role,
            idempotency,
            idempotency_when,
            reboot,
            recovery_semantics: Arc::from(self.recovery_semantics.trim()),
            preconditions,
            tolerances: self
                .tolerances
                .into_iter()
                .map(|raw| Tolerance {
                    field: Arc::from(raw.field.as_str()),
                    doc: Arc::from(raw.doc.as_str()),
                })
                .collect(),
            effects,
            verification,
        })
    }
}
