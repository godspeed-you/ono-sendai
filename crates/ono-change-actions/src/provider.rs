//! The bridge from an intent to the provider that carries it out (spec §51).
//!
//! §51 declares four questions and one prohibition. The questions are what an intent means
//! (`supports`), what it becomes (`resolve`), whether it still holds (`revalidate`) and what
//! happened (`execute`, `verify`). The prohibition is the one that makes §2.1 hold for the whole
//! planner rather than only for the code that calls it: *providers MUST NOT mutate state during
//! `supports` or `resolve`.*
//!
//! Nothing in this module can break that by accident. `supports` and `resolve` reach the outside
//! world through exactly one thing — an [`Observer`], which answers questions and has no method
//! that changes anything — and the [`Bridge`] that can call
//! [`ono_provider_api::Provider::act`] is used by `execute` alone.
//!
//! What a resolved action carries comes from three places, and none of them is invention:
//!
//! - `docs/contracts/commands/` gives the target schema, the required provider capability, the
//!   privilege and the arguments (§6.1);
//! - `plannable_operations:` gives the expected direct effects, the idempotency class and the
//!   recovery semantics (§6.1, [`crate::registry`]);
//! - the [`Observer`] gives the values §7.2's preconditions freeze, and §30.5's reboot answer.
//!
//! An intent naming an operation with no row is not plannable, and `resolve` refuses with
//! `change.action_not_plannable` (§6.1, §6.2).

use std::str::FromStr as _;
use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{
    ChangeCapability, ChangeProvider, DriftFinding, EffectDomain, EffectKind, Execution,
    FrozenTarget, Intent, PlanAction, PlanFragment, PlanId, PreconditionKind, ProposedEffect,
    ProviderCapabilities, Support, VerificationClass, VerificationContract, VerificationResult,
    VerificationStatus, error,
};
use ono_command::{CommandContract, CommandRegistry, Privilege};
use ono_provider_api::{Action, ObjectId, Provider, Query, Selector};
use ono_value::{ErrorValue, SchemaId, Value};

use crate::bridge::Bridge;
use crate::intent::{self, Statement};
use crate::opaque::AdapterAction;
use crate::preconditions::{self, Mechanism, Observer};
use crate::registry::{Operation, OperationRegistry, RebootDimension};

/// The contract every effect and every idempotency class in a resolved action is cited against.
const CONTRACT: &str = "docs/contracts/change/actions.yaml#plannable_operations";

/// Turns an intent into plan actions over one Ono provider, and carries them out (§51).
///
/// One of these wraps one [`Provider`]. The registries it resolves against are the shell's own,
/// so a command that exists is a command a plan can name, and a command that does not is a
/// command §6.2 refuses.
#[derive(Debug, Clone)]
pub struct ProviderChangeProvider {
    id: Arc<str>,
    version: Arc<str>,
    provider: Arc<dyn Provider>,
    commands: &'static CommandRegistry,
    operations: &'static OperationRegistry,
    observer: Arc<dyn Observer>,
    bridge: Arc<dyn Bridge>,
    session: Arc<str>,
    now: Timestamp,
    plan: PlanId,
    adapters: Vec<AdapterAction>,
}

impl ProviderChangeProvider {
    /// Wraps `provider`, resolving against the embedded registries.
    ///
    /// `now` is a parameter rather than a reading of the clock, because §4.4 makes a plan built
    /// twice from the same intent, session and instant the same plan — an identity that is only
    /// checkable when the instant is the caller's to state.
    ///
    /// # Errors
    ///
    /// A structured error when an embedded registry does not typecheck, which is a build defect.
    pub fn new(
        provider: Arc<dyn Provider>,
        observer: Arc<dyn Observer>,
        bridge: Arc<dyn Bridge>,
        session: impl Into<Arc<str>>,
        now: Timestamp,
    ) -> Result<Self, ErrorValue> {
        let session = session.into();
        Ok(Self {
            id: Arc::from(provider.id()),
            version: Arc::from("0"),
            provider,
            commands: CommandRegistry::embedded()?,
            operations: OperationRegistry::embedded()?,
            observer,
            bridge,
            plan: PlanId::of(&session, &now.to_string(), ""),
            session,
            now,
            adapters: Vec::new(),
        })
    }

    /// Records the provider version the plan is resolved against (§4.4, Appendix G.4).
    #[must_use]
    pub fn at_version(mut self, version: impl Into<Arc<str>>) -> Self {
        self.version = version.into();
        self
    }

    /// Binds the provider to the plan whose contracts [`ChangeProvider::verify`] observes (§23.3).
    #[must_use]
    pub fn for_plan(mut self, plan: PlanId) -> Self {
        self.plan = plan;
        self
    }

    /// Accepts a plannable action a v0.3 external-command adapter declares (§6.2).
    #[must_use]
    pub fn with_adapter_action(mut self, action: AdapterAction) -> Self {
        self.adapters.push(action);
        self
    }

    /// The plan identity an intent resolved in this session at this instant carries (§4.4).
    #[must_use]
    pub fn plan_id(&self, intent: &Intent) -> PlanId {
        PlanId::of(&self.session, &self.now.to_string(), intent.text())
    }

    /// The plannable actions the adapters declared (§6.2).
    #[must_use]
    pub fn adapter_actions(&self) -> &[AdapterAction] {
        &self.adapters
    }

    /// Whether this provider answers about the target `contract` names.
    fn serves(&self, contract: &CommandContract) -> bool {
        contract
            .target()
            .is_some_and(|target| self.provider.targets().contains(&target))
    }

    /// The operation row for a statement, where §6.1 has one.
    fn operation(&self, contract: &CommandContract) -> Option<&'static Operation> {
        self.operations.get(contract.id())
    }

    /// The targets one statement acts on: the ones the caller froze, or the ones it names itself.
    ///
    /// §4.3 freezes the target set at resolution and §2.6 forbids adding to it later, so a caller
    /// that resolved the set passes it in and this filters it to the schema the statement acts on.
    /// A statement whose schema nothing in that set matches names its own object, which is what a
    /// plan block's `copy file ./a to /etc/b` does.
    fn targets_for(
        &self,
        mutation: &intent::Mutation<'_>,
        frozen: &[FrozenTarget],
    ) -> Result<Vec<FrozenTarget>, ErrorValue> {
        let contract = mutation.contract;
        let word = contract.target().unwrap_or_default();
        let schema = self
            .commands
            .target(word)
            .and_then(ono_command::TargetSpec::schema)
            .unwrap_or(word);
        let matching: Vec<FrozenTarget> = frozen
            .iter()
            .filter(|target| target.schema() == schema)
            .cloned()
            .collect();
        if !matching.is_empty() {
            return Ok(matching);
        }
        let (_, identity) = mutation.object_selector().ok_or_else(|| {
            error::target_unresolved(
                &mutation.text,
                "§7.1 stores stable object identities, and this statement names none. Write the \
                 object the operation acts on, or resolve the target set before planning.",
            )
        })?;
        let label =
            ono_value::canonical_text(identity).unwrap_or_else(|_| identity.type_name().to_owned());
        Ok(vec![
            FrozenTarget::new(schema, label.clone(), label).resolved_from(mutation.text.as_ref()),
        ])
    }

    /// One action of the fragment: §6.1's nine facts, over one frozen target.
    fn action_for(
        &self,
        plan: &PlanId,
        ordinal: usize,
        mutation: &intent::Mutation<'_>,
        operation: &Operation,
        target: &FrozenTarget,
    ) -> PlanAction {
        let contract = mutation.contract;
        let mut arguments: Vec<(Arc<str>, Value)> = mutation.selectors.clone();
        arguments.extend(mutation.options.iter().cloned());
        let summary = format!("{} {}", contract.spelling(), target.label());
        let execution = Execution::ProviderAction {
            provider: Arc::clone(&self.id),
            operation: Arc::from(contract.id()),
            arguments: arguments.clone(),
        };
        let mut action = PlanAction::new(plan, ordinal, operation.role(), summary, execution)
            .on(target.identity())
            .with_idempotency(operation.idempotency_for(&arguments))
            .recovery_semantics(operation.recovery_semantics());
        if contract.privilege() == Privilege::Elevated {
            action = action.privileged();
        }
        for precondition in preconditions::for_target(
            operation,
            target,
            &self.id,
            contract.provider_capability(),
            self.observer.as_ref(),
        ) {
            action = action.requiring(precondition);
        }
        for spec in operation.effects() {
            let object = spec
                .object_selector()
                .and_then(|name| mutation.selector(name))
                .and_then(|value| ono_value::canonical_text(value).ok())
                .unwrap_or_else(|| target.label().to_owned());
            let identity = action.id().clone();
            let mut effect = ProposedEffect::new(
                identity,
                spec.domain(),
                spec.kind(),
                spec.confidence(),
                spec.explanation(),
            )
            .on(object)
            .citing(format!("{CONTRACT}/{}", operation.id()));
            if spec.is_irreversible() {
                effect = effect.irreversible();
            }
            if let Some(compensation) = spec.compensation() {
                effect = effect.compensated_by(compensation);
            }
            action = action.effecting(effect);
        }
        if operation.reboot() == RebootDimension::ProviderReported {
            let reported = self.observer.reboot(target.identity(), contract.id());
            if let Some(confidence) = reported.confidence() {
                let effect = ProposedEffect::new(
                    action.id().clone(),
                    EffectDomain::KernelRuntime,
                    EffectKind::Modify,
                    confidence,
                    reported.explanation(),
                )
                .on(format!("reboot: {}", reported.as_str()))
                .citing(format!("{CONTRACT}/{}", operation.id()));
                action = action.effecting(effect);
            }
        }
        action
    }

    /// The verification contracts one action carries (§23.1, §30.4).
    ///
    /// A contract whose `{option:…}` substitution has no value is not emitted: §23.1 asks for
    /// checks that can be answered, and a check against an argument nobody gave is not one.
    fn contracts_for(
        &self,
        plan: &PlanId,
        mutation: &intent::Mutation<'_>,
        operation: &Operation,
        target: &FrozenTarget,
    ) -> Vec<VerificationContract> {
        let word = mutation.contract.target().unwrap_or_default();
        let subject = format!("{word} {}", target.label());
        operation
            .verification()
            .iter()
            .filter_map(|spec| {
                substitute(spec.expression(), &mutation.options).map(|expression| {
                    VerificationContract::new(plan, spec.class(), subject.clone(), expression)
                })
            })
            .collect()
    }

    /// Reads one object back through the provider's own snapshot (§23.1).
    ///
    /// The three answers are kept apart on purpose. An object that is not there is a fact about
    /// the world; a field a record does not carry is a fact about the provider, and §23.3 makes
    /// the second an `UNKNOWN` rather than a failure — the check ran and could not answer.
    fn observe(&self, subject: &str, field: Option<&str>) -> Result<Observed, ErrorValue> {
        let (word, identity) = split_subject(subject);
        let query = self.query_for(word, identity)?;
        let records = self.bridge.snapshot(self.provider.as_ref(), &query)?;
        let Some(first) = records
            .iter()
            .find(|value| identity.is_empty() || shows(value, identity))
        else {
            return Ok(Observed::Missing);
        };
        let Some(field) = field else {
            return Ok(Observed::Present(Some(first.clone())));
        };
        Ok(Observed::Present(
            first
                .as_record()
                .ok()
                .and_then(|record| record.get(field).cloned()),
        ))
    }

    /// The query that reads one object back, narrowed by the selector its `get` command declares.
    fn query_for(&self, word: &str, identity: &str) -> Result<Query, ErrorValue> {
        let mut query = Query::target(word);
        if identity.is_empty() {
            return Ok(query);
        }
        if let Some(spec) = self
            .commands
            .find("get", Some(word))
            .and_then(|contract| contract.selectors().first())
        {
            let text = identity.strip_prefix(':').unwrap_or(identity);
            if let Ok(value) = spec.declared_type().coerce(text) {
                query = query.with(Selector::field(spec.name(), value));
            }
        }
        Ok(query)
    }
}

impl ChangeProvider for ProviderChangeProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::new(Arc::clone(&self.id))
            .changing(ChangeCapability::PlanRead)
            .changing(ChangeCapability::PlanContribute)
            .changing(ChangeCapability::ActionExecute)
            .changing(ChangeCapability::VerificationObserve)
    }

    fn supports(&self, intent: &Intent) -> Support {
        let mut planned = 0_usize;
        let mut refused: Vec<String> = Vec::new();
        for line in intent::statements(intent.text()) {
            match intent::parse(self.commands, line) {
                Ok(Statement::Check(_)) => planned += 1,
                Ok(Statement::Mutation(mutation)) => {
                    if self.operation(mutation.contract).is_some() && self.serves(mutation.contract)
                    {
                        planned += 1;
                    } else {
                        refused.push(line.to_owned());
                    }
                }
                Err(_) => refused.push(line.to_owned()),
            }
        }
        if planned == 0 {
            return Support::None;
        }
        if let Some(reason) = self.provider.availability().reason() {
            return Support::Partial(Arc::from(reason));
        }
        if refused.is_empty() {
            Support::Full
        } else {
            Support::Partial(Arc::from(
                format!("this provider cannot plan: {}", refused.join("; ")).as_str(),
            ))
        }
    }

    fn resolve(
        &self,
        intent: &Intent,
        targets: &[FrozenTarget],
    ) -> Result<PlanFragment, ErrorValue> {
        let plan = self.plan_id(intent);
        let mut fragment = PlanFragment::empty().at_version(Arc::clone(&self.version));
        let mut ordinal = 0_usize;
        let mut wants_provider = false;
        for line in intent::statements(intent.text()) {
            match intent::parse(self.commands, line)? {
                Statement::Check(check) => {
                    fragment = fragment.verifying(VerificationContract::new(
                        &plan,
                        VerificationClass::Required,
                        check.subject,
                        check.expression,
                    ));
                }
                Statement::Mutation(mutation) => {
                    let operation = self.operation(mutation.contract).ok_or_else(|| {
                        error::action_not_plannable(
                            line,
                            "`docs/contracts/change/actions.yaml` declares no \
                             `plannable_operations:` row for it, so nothing states its effects, \
                             its idempotency class or its recovery semantics. §6.2's second \
                             sentence is the way to change that: an external-command adapter \
                             whose contract is explicit.",
                        )
                    })?;
                    for target in self.targets_for(&mutation, targets)? {
                        let action = self.action_for(&plan, ordinal, &mutation, operation, &target);
                        for contract in self.contracts_for(&plan, &mutation, operation, &target) {
                            fragment = fragment.verifying(contract);
                        }
                        fragment = fragment.acting(action);
                        ordinal += 1;
                    }
                    wants_provider |= operation
                        .preconditions()
                        .contains(&PreconditionKind::ProviderAvailable);
                }
            }
        }
        if ordinal == 0 && fragment.verification().is_empty() {
            return Err(error::action_not_plannable(
                intent.text(),
                "the intent names no statement this shell can resolve.",
            ));
        }
        if wants_provider {
            fragment = fragment.requiring(
                ono_change_core::Precondition::new(
                    PreconditionKind::ProviderAvailable,
                    Arc::clone(&self.id),
                    preconditions::field_of(PreconditionKind::ProviderAvailable),
                    Value::Bool(true),
                )
                .explained(
                    "§7.2: the provider this fragment depends on is still available at apply time",
                ),
            );
        }
        Ok(fragment)
    }

    fn revalidate(&self, action: &PlanAction) -> Result<Vec<DriftFinding>, ErrorValue> {
        Ok(preconditions::revalidate(
            action,
            self.observer.as_ref(),
            &Declared(self.provider.as_ref()),
        ))
    }

    fn execute(&self, action: &PlanAction) -> Result<ono_value::ActionResult, ErrorValue> {
        let Execution::ProviderAction {
            operation,
            arguments,
            ..
        } = action.execution()
        else {
            return Err(error::action_not_plannable(
                action.summary(),
                "this change provider carries provider actions. A program runs through §12.3's \
                 tool runner and an opaque action through §6.3's escape, neither of which is here.",
            ));
        };
        let contract = self.commands.get(operation).ok_or_else(|| {
            error::action_not_plannable(
                operation,
                "the plan names a command this shell does not declare, which §7.5 makes a reason \
                 to rebase rather than to run.",
            )
        })?;
        let word = contract.target().unwrap_or_default();
        let identity = action.target().unwrap_or_default();
        let schema = self
            .commands
            .target(word)
            .and_then(ono_command::TargetSpec::schema)
            .unwrap_or(word);
        let schema = SchemaId::from_str(schema)?;
        let object = ObjectId::new(schema, [Value::string(identity)]);
        let mut request = Action::new(word, contract.verb(), object)
            .with_source(identity)
            .labelled(identity);
        let carried = mutation_arguments(contract, arguments);
        for (name, value) in carried {
            request = request.with(name.as_ref(), value.clone());
        }
        let started = std::time::Instant::now();
        let outcome = self.bridge.act(self.provider.as_ref(), &request)?;
        let elapsed = ono_value::Duration::from_nanoseconds(
            i128::try_from(started.elapsed().as_nanos()).unwrap_or(i128::MAX),
        );
        Ok(outcome.into_record(elapsed).with_operation(operation))
    }

    fn verify(&self, contract: &VerificationContract) -> Result<VerificationResult, ErrorValue> {
        let expression = contract.expression();
        let condition = Condition::read(expression);
        let observed = self.observe(contract.subject(), condition.field())?;
        let status = condition.judge(&observed);
        let mut result = VerificationResult::new(self.plan.clone(), contract, status, self.now)
            .citing(format!(
                "provider:{} snapshot of `{}`",
                self.id,
                contract.subject()
            ));
        match &observed {
            Observed::Missing => {
                result = result.explained(format!(
                    "§23.1: `{}` was read back through the provider and is not there",
                    contract.subject()
                ));
            }
            Observed::Present(Some(value)) => result = result.observing(value.clone()),
            Observed::Present(None) => {}
        }
        if status == VerificationStatus::Unknown {
            result = result.explained(format!(
                "§23.3: the check ran and could not answer. `{expression}` is either a condition \
                 this provider does not observe, or a field the object does not carry; the \
                 conditions it reads are `exists`, `exists == <bool>`, `<field> == <value>` and \
                 `<field> != <value>`"
            ));
        }
        Ok(result)
    }
}

/// The provider, asked about itself (§7.2, §43.2).
///
/// A provider's availability and the capabilities it advertises are its own to state, and asking
/// an observer of the world for them would be asking the wrong witness.
struct Declared<'p>(&'p dyn Provider);

impl Mechanism for Declared<'_> {
    fn is_available(&self, provider: &str) -> bool {
        self.0.id() == provider && self.0.availability().is_available()
    }

    fn holds(&self, provider: &str, capability: &str) -> bool {
        self.0.id() == provider
            && self
                .0
                .capabilities()
                .iter()
                .any(|held| held.id() == capability)
    }
}

/// What a provider's snapshot said about one object (§23.1, §23.3).
#[derive(Debug, PartialEq)]
enum Observed {
    /// The provider answered, and the object is not there.
    Missing,
    /// The object is there; the value is the field that was asked for, where it carries one.
    Present(Option<Value>),
}

/// What one verification expression asks, in the vocabulary a snapshot can answer.
///
/// §23.1's floor is "provider-level state acknowledgement", which is exactly `exists`: the object
/// is read back through [`Provider::snapshot`] and the answer is whether it is there. Everything
/// beyond that is a field compared against a value. An expression outside both is `UNKNOWN` —
/// §23.3's check that ran and could not answer — rather than a pass.
#[derive(Debug, PartialEq, Eq)]
enum Condition<'e> {
    /// The object is there.
    Exists(bool),
    /// A field equals a value.
    Equals { field: &'e str, value: &'e str },
    /// A field does not equal a value.
    Differs { field: &'e str, value: &'e str },
    /// Nothing this provider can observe.
    Unreadable,
}

impl<'e> Condition<'e> {
    fn read(expression: &'e str) -> Self {
        let text = expression.trim();
        if let Some((left, right)) = split_once_operator(text, "==") {
            return match (left, unquote(right)) {
                ("exists", "true") => Condition::Exists(true),
                ("exists", "false") => Condition::Exists(false),
                ("exists", _) => Condition::Unreadable,
                (field, value) => Condition::Equals { field, value },
            };
        }
        if let Some((left, right)) = split_once_operator(text, "!=") {
            return match left {
                "exists" => Condition::Unreadable,
                field => Condition::Differs {
                    field,
                    value: unquote(right),
                },
            };
        }
        if text == "exists" {
            return Condition::Exists(true);
        }
        Condition::Unreadable
    }

    /// The field a snapshot has to read, where the condition names one.
    const fn field(&self) -> Option<&str> {
        match self {
            Condition::Exists(_) | Condition::Unreadable => None,
            Condition::Equals { field, .. } | Condition::Differs { field, .. } => Some(field),
        }
    }

    /// The verdict for what was observed (§23.3).
    fn judge(&self, observed: &Observed) -> VerificationStatus {
        match (self, observed) {
            (Condition::Unreadable, _) => VerificationStatus::Unknown,
            (Condition::Exists(wanted), _) => {
                let present = matches!(observed, Observed::Present(_));
                if present == *wanted {
                    VerificationStatus::Passed
                } else {
                    VerificationStatus::Failed
                }
            }
            // The object is not there, so the field it would have carried is not equal to
            // anything. That is an answer rather than an absence of one.
            (_, Observed::Missing) => VerificationStatus::Failed,
            // The object is there and the provider does not report the field. §23.3: the check
            // ran and could not answer, which §23.5 forbids treating as success.
            (_, Observed::Present(None)) => VerificationStatus::Unknown,
            (
                Condition::Equals { value, .. } | Condition::Differs { value, .. },
                Observed::Present(Some(seen)),
            ) => {
                let Ok(text) = ono_value::canonical_text(seen) else {
                    return VerificationStatus::Unknown;
                };
                let equal = text == *value;
                let wanted = matches!(self, Condition::Equals { .. });
                if equal == wanted {
                    VerificationStatus::Passed
                } else {
                    VerificationStatus::Failed
                }
            }
        }
    }
}

/// Splits an expression on an operator that stands as its own word or between two operands.
fn split_once_operator<'e>(text: &'e str, operator: &str) -> Option<(&'e str, &'e str)> {
    let (left, right) = text.split_once(operator)?;
    Some((left.trim(), right.trim()))
}

/// Strips the quotes a plan block may put round a literal.
fn unquote(text: &str) -> &str {
    text.strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            text.strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })
        .unwrap_or(text)
}

/// Splits `service nginx` into the target word and the object's label.
fn split_subject(subject: &str) -> (&str, &str) {
    match subject.split_once(char::is_whitespace) {
        Some((word, identity)) => (word, identity.trim()),
        None => (subject, ""),
    }
}

/// Whether `value` is the record naming `identity` in one of its fields.
fn shows(value: &Value, identity: &str) -> bool {
    let wanted = identity.strip_prefix(':').unwrap_or(identity);
    value.as_record().is_ok_and(|record| {
        record.schema().identity_for(record).iter().any(|field| {
            record
                .get(field)
                .and_then(|held| ono_value::canonical_text(held).ok())
                .is_some_and(|text| text == wanted || text == identity)
        }) || record
            .get("name")
            .and_then(|held| ono_value::canonical_text(held).ok())
            .is_some_and(|text| text == wanted)
    })
}

/// The arguments that travel with a provider action, by the convention the rest of the shell uses.
///
/// A creating verb's selectors describe the object and the provider needs them by name — `mount
/// filesystem <source> <target>` is a source and a target, not two anonymous identity values. For
/// every other verb, the selector that supplied the object travels as the object and the rest
/// travel under their own names (ADR-0082 §2, ADR-0098 §1).
fn mutation_arguments<'a>(
    contract: &CommandContract,
    arguments: &'a [(Arc<str>, Value)],
) -> Vec<&'a (Arc<str>, Value)> {
    let creating = matches!(contract.verb(), "add" | "mount");
    let object = contract.selectors().iter().find_map(|spec| {
        arguments
            .iter()
            .find(|(name, _)| name.as_ref() == spec.name())
            .map(|(name, _)| Arc::clone(name))
    });
    arguments
        .iter()
        .filter(|(name, _)| {
            creating || object.as_deref().map(str::to_owned) != Some(name.as_ref().to_owned())
        })
        .collect()
}

/// Fills `{option:<name>}` from the arguments, or answers `None` when nobody gave one.
fn substitute(expression: &str, options: &[(Arc<str>, Value)]) -> Option<String> {
    let mut filled = expression.to_owned();
    while let Some(start) = filled.find("{option:") {
        let end = filled[start..].find('}')? + start;
        let name = &filled[start + "{option:".len()..end];
        let value = options
            .iter()
            .find(|(declared, _)| declared.as_ref() == name)
            .map(|(_, value)| value)?;
        let text = ono_value::canonical_text(value).ok()?;
        filled.replace_range(start..=end, &text);
    }
    Some(filled)
}
