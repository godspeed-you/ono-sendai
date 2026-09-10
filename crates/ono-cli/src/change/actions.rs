//! Turning what an operator typed into plan actions (spec v0.6 §5.1, §5.2, §6.1, §6.2, §6.3).
//!
//! §6.1 fixes the question this module answers: an operation is plannable when a contract
//! declares its target scope, its effects, its reversibility, the privilege it needs and how it
//! is verified. The contracts this shell has are the ones in `docs/contracts/commands/`, so a
//! statement inside `plan { … }` is resolved through the same [`ono_command::CommandRegistry`]
//! the evaluator resolves a command through (ADR-0813). What that registry cannot answer, this
//! module refuses rather than guesses:
//!
//! - a spelling with a contract that declares no mutation — `get service nginx` — is
//!   `change.action_not_plannable`, naming what would have to be declared;
//! - a spelling with no contract at all that resolves to a program on `PATH` — `sh -c '…'` — is
//!   §6.2's `change.opaque_action_forbidden`, and §6.3's `--opaque` is the only way past it;
//! - a spelling that is neither — `validate config nginx` — is `change.action_not_plannable`,
//!   because §6.3's escape is an explicit request and never a fallback (ADR-0813).
//!
//! `operations()` is the table §6.1 asks the contract to carry beyond what a command contract
//! already says: which mutation domain the operation acts in, at what confidence, whether it can
//! be run twice, and what the plan will check afterwards. It is the shell's stand-in for the
//! `plannable_operations:` registry of `docs/contracts/change/actions.yaml`; when that registry
//! lands, [`operation_of`] is the one function that changes.

use std::sync::Arc;

use ono_change_core::{
    ActionRole, EffectConfidence, EffectDomain, EffectKind, Execution, Idempotency, PlanAction,
    PlanFragment, PlanId, Precondition, PreconditionKind, ProposedEffect, VerificationClass,
    VerificationContract, error,
};
use ono_command::CommandRegistry;
use ono_parser::{Argument, Stage, StageHead};
use ono_value::{ErrorValue, Value};

/// What kind of object an operation mutates, and how its identity is read (§7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetShape {
    /// A service-manager unit, frozen with its provider namespace and its generation (§7.1).
    Service,
    /// A path on this host, frozen with its persistence domain (§7.1, Appendix B).
    File,
    /// An installed package, frozen by name and version.
    Package,
    /// Any other object a provider knows by name — a route, a mount, a container, a user.
    ///
    /// §7.1 asks for a provider namespace, an object identity and the generation a revalidation
    /// compares against, and those three come back from the provider for every target the shell
    /// has. The word is the target the provider is found by.
    Named(&'static str),
}

impl TargetShape {
    /// How the objects of `target` are frozen (`docs/contracts/targets.yaml`).
    #[must_use]
    pub const fn of(target: &'static str) -> Self {
        match target.as_bytes() {
            b"service" => TargetShape::Service,
            b"file" | b"dir" => TargetShape::File,
            b"package" => TargetShape::Package,
            _ => TargetShape::Named(target),
        }
    }

    /// The target word a command contract spells this shape with (`docs/contracts/targets.yaml`).
    ///
    /// It travels into a verification contract's subject because §23.1's check is asked of the
    /// world through a provider, and the provider is found by target: `service nginx` names
    /// which of them to ask, where `nginx` alone would not.
    #[must_use]
    pub const fn target_word(self) -> &'static str {
        match self {
            TargetShape::Service => "service",
            TargetShape::File => "file",
            TargetShape::Package => "package",
            TargetShape::Named(word) => word,
        }
    }
}

/// One operation §6.1 makes plannable, as this shell needs it.
///
/// The semantics — the effects, the idempotency class, the reversibility statement and the
/// verification contracts — come from `docs/contracts/change/actions.yaml` through
/// [`ono_change_actions::OperationRegistry`], which is the one place they are declared. What is
/// added here is what only the shell knows: how the target is frozen (§7.1), which selector names
/// the object, and whether the command needs §43.3's elevation. Those are properties of the
/// command contract rather than of the operation, and they are read from it.
#[derive(Debug, Clone, Copy)]
pub struct PlannableOperation {
    /// The command contract this operation is the mutating half of.
    pub command: &'static str,
    /// What kind of object it acts on (§7.1).
    pub shape: TargetShape,
    /// Which selector of the contract names the object it mutates.
    ///
    /// `copy file ./new to /etc/nginx.conf` writes the *destination*, and freezing the source
    /// would put the wrong path into the plan's recovery domain (Appendix B).
    pub subject: &'static str,
    /// Whether the selector that names the object is a number rather than text (§4.3).
    ///
    /// `process 4211` selects by an `int` field and `service nginx` by a `string` one, and a
    /// provider asked for a string `4211` where it holds an integer answers with nothing. The
    /// declared type is the contract's, so a target that changes its selector type does not need
    /// this to be remembered anywhere else.
    pub subject_is_number: bool,
    /// Whether §43.3's elevated privilege is needed.
    pub privileged: bool,
    /// Whether the operation brings its object into existence (§4.3, §7.2).
    ///
    /// `add user alice` names an object no provider can answer for yet, so §4.3's freeze records
    /// the identity the operator gave rather than refusing, and §7.2's existence precondition is
    /// that the object is *not* there — a plan to create something that already exists is a plan
    /// whose world moved.
    pub creates: bool,
    /// The contract row: effects, idempotency, reversibility and verification (§6.1).
    pub declared: &'static ono_change_actions::Operation,
}

impl PlannableOperation {
    /// Whether the object has to exist for the operation to be planned at all (§4.3, §7.2).
    ///
    /// §7.2's existence precondition is "the object still exists and is still this object". An
    /// operation that declares it and brings nothing into existence — a removal, a move, a
    /// permission change — states a precondition nobody could satisfy when the object is not
    /// there, and sealing it would hand `apply` a plan that can only be refused. `write file`
    /// declares no existence precondition and `copy file` creates its destination, so both still
    /// plan onto a path that does not exist yet.
    #[must_use]
    pub fn requires_existence(&self) -> bool {
        !self.creates
            && self
                .declared
                .preconditions()
                .contains(&PreconditionKind::Existence)
    }
}

/// Every operation §6.1 makes plannable, built once from the registry (§6.2, §47).
///
/// §6.2's default is refusal, and this is where it is decided: an operation with no row in
/// `docs/contracts/change/actions.yaml` is refused with a sentence naming what a contract would
/// have to declare. The list was written out by hand here once, and eleven of the registry's
/// forty-six operations reached an operator as a result — a second copy of a contract is a
/// contract that stops being one.
fn operations() -> &'static [PlannableOperation] {
    static OPERATIONS: std::sync::OnceLock<Vec<PlannableOperation>> = std::sync::OnceLock::new();
    OPERATIONS.get_or_init(|| {
        let Ok(registry) = ono_change_actions::OperationRegistry::embedded() else {
            // A malformed embedded document is a build defect the registry's own tests catch, and
            // an empty list here is the fail-closed answer: nothing is plannable (§6.2, §56.3).
            return Vec::new();
        };
        let Ok(commands) = ono_command::CommandRegistry::load() else {
            return Vec::new();
        };
        registry
            .operations()
            .iter()
            .filter_map(|operation| {
                let contract: &ono_command::CommandContract = commands.get(operation.id())?;
                let target: &'static str = Box::leak(
                    contract
                        .target()
                        .unwrap_or_else(|| contract.verb())
                        .to_owned()
                        .into_boxed_str(),
                );
                let first = contract.selectors().first();
                // Which selector names the object the plan is *about* — the one it freezes and
                // the one a recovery domain is resolved from. `copy file <source> to
                // <destination>` acts on the source and changes the destination, and freezing
                // the source would put the wrong path into Appendix B's resolution. The registry
                // already says so: an effect names the selector it lands on (§8.2), and the
                // first one that does is the object. Otherwise it is the contract's own first
                // selector, which is what `restart service nginx` means.
                let subject: &'static str = Box::leak(
                    operation
                        .effects()
                        .iter()
                        .find_map(ono_change_actions::EffectSpec::object_selector)
                        .or_else(|| first.map(ono_command::ParameterSpec::name))
                        .unwrap_or("name")
                        .to_owned()
                        .into_boxed_str(),
                );
                let subject_is_number = contract
                    .selectors()
                    .iter()
                    .find(|spec| spec.name() == subject)
                    .or(first)
                    .is_some_and(|spec| {
                        matches!(
                            spec.declared_type(),
                            ono_command::DeclaredType::Int
                                | ono_command::DeclaredType::Float
                                | ono_command::DeclaredType::Port
                        )
                    });
                Some(PlannableOperation {
                    command: Box::leak(operation.id().to_owned().into_boxed_str()),
                    shape: TargetShape::of(target),
                    subject,
                    subject_is_number,
                    // §43.3: `conditional` privilege is needed only in some situations, and the
                    // provider says which when it acts. Only `elevated` is privileged up front.
                    privileged: contract.privilege() == ono_command::Privilege::Elevated,
                    creates: operation.effects().iter().all(|effect| {
                        !matches!(
                            effect.kind(),
                            ono_change_core::EffectKind::Remove
                                | ono_change_core::EffectKind::Replace
                        )
                    }) && operation
                        .effects()
                        .iter()
                        .any(|effect| effect.kind() == ono_change_core::EffectKind::Create),
                    declared: operation,
                })
            })
            .collect()
    })
}

/// The operation `command` names, or `None` where this build cannot plan it (§6.1).
#[must_use]
pub fn operation_of(command: &str) -> Option<&'static PlannableOperation> {
    operations()
        .iter()
        .find(|operation| operation.command == command)
}

/// One statement of a `plan { … }` block, or the single action of `plan <mutation>` (§5.1, §5.2).
#[derive(Debug, Clone)]
pub struct Statement {
    /// The head word, as the operator wrote it.
    pub head: String,
    /// The arguments, as the parser read them.
    pub arguments: Vec<Argument>,
    /// The statement rebuilt from its words, which is what a refusal quotes back.
    pub source: String,
}

impl Statement {
    /// Reads one parsed stage as a statement, or `None` where the head is not a command word.
    #[must_use]
    pub fn of_stage(stage: &Stage) -> Option<Self> {
        let StageHead::Command(name) = &stage.head else {
            return None;
        };
        let head = match name.namespace.as_deref() {
            Some(namespace) => format!("{namespace}:{}", name.name),
            None => name.name.clone(),
        };
        let mut source = head.clone();
        for argument in &stage.arguments {
            source.push(' ');
            source.push_str(&spell(argument));
        }
        Some(Self {
            head,
            arguments: stage.arguments.clone(),
            source,
        })
    }

    /// Whether this statement is §23.1's verification contract rather than an action (ADR-0813).
    #[must_use]
    pub fn is_verification(&self) -> bool {
        self.head == "verify"
    }

    /// The words after the head, which is how both a subject and an expression are read.
    #[must_use]
    pub fn words(&self) -> Vec<String> {
        self.arguments.iter().map(spell).collect()
    }
}

/// One argument as a word, for a statement a refusal has to quote and a contract has to resolve.
fn spell(argument: &Argument) -> String {
    match argument {
        Argument::Word(word) => word.text.clone(),
        Argument::Option(option) => match &option.value {
            Some(ono_parser::Expr::Str(literal)) => match literal.literal_text() {
                Some(text) => format!("--{}={text}", option.name),
                None => format!("--{}", option.name),
            },
            _ => format!("--{}", option.name),
        },
        Argument::Value(ono_parser::Expr::Str(literal)) => literal
            .literal_text()
            .map_or_else(|| "…".to_owned(), str::to_owned),
        Argument::Value(_) | Argument::Error(_) => "…".to_owned(),
    }
}

/// Whether §6.3's escape was given for an opaque action (§6.2, §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpaquePermission {
    /// Whether `--opaque` was written.
    pub requested: bool,
    /// Whether `change.allow_opaque_actions` is set (§53).
    pub configured: bool,
}

impl OpaquePermission {
    /// Whether an opaque action may be admitted at all (§6.3).
    ///
    /// Both halves are needed. §6.3 is an explicit per-plan request, and §53's setting is the
    /// administrator's decision that the request may be made on this host at all.
    #[must_use]
    pub const fn is_granted(self) -> bool {
        self.requested && self.configured
    }
}

/// What one statement contributes to a plan (§5.2, §23.1).
#[derive(Debug, Clone)]
pub enum Resolution {
    /// An operation the registry declares and `operations()` can plan.
    Operation {
        /// The contract the spelling resolved to.
        command: &'static str,
        /// What the plan knows about the operation beyond the command contract.
        operation: &'static PlannableOperation,
        /// The objects the action mutates, as the statement or the pipeline named them (§7.1).
        subjects: Vec<String>,
        /// The object identity the provider acts on, where the statement fixed one.
        ///
        /// `copy file <source> to <destination>` acts on the *source* and writes the
        /// destination, so the object a provider is asked about and the object the plan is
        /// protecting are different things and both have to travel (ADR-0082 §1, §2).
        object: Option<String>,
        /// Which selector supplied that object, so execution resolves it the way the shell does.
        object_selector: Arc<str>,
        /// Whether that selector is a path, which is acted on rather than resolved (ADR-0082 §1).
        object_is_path: bool,
        /// The arguments that travel with the action, already typed.
        arguments: Vec<(Arc<str>, Value)>,
        /// The statement, as the operator wrote it.
        source: String,
    },
    /// §23.1's contract: what the plan will ask the world afterwards (ADR-0813).
    Verification {
        /// What the check is about — `service nginx`, `socket :443`.
        subject: String,
        /// The condition, as a person and a machine both read it.
        expression: String,
    },
    /// §6.3's acknowledged escape: an action Ono cannot reason about.
    Opaque {
        /// What the operator said the action does.
        description: String,
        /// The program, where one resolved.
        program: Option<String>,
        /// The argument vector, unquoted and unexpanded (§2.17).
        argv: Vec<Arc<str>>,
    },
}

/// Reads `statement` as the contribution it makes to a plan (§6.1, §6.2, §6.3).
///
/// # Errors
///
/// - `change.action_not_plannable` where no contract declares the operation's semantics, or
///   where the contract that exists declares a query rather than a mutation;
/// - `change.opaque_action_forbidden` where the statement runs an external program and §6.3's
///   escape was not given.
pub fn resolve(
    registry: &CommandRegistry,
    statement: &Statement,
    opaque: OpaquePermission,
    piped: &[Value],
) -> Result<Resolution, ErrorValue> {
    if statement.is_verification() {
        return verification_of(statement);
    }
    let Ok(resolved) = registry.resolve(&statement.head, &statement.arguments) else {
        return Err(unresolvable(statement, opaque));
    };
    let contract = resolved.contract;
    let Some(operation) = operation_of(contract.id()) else {
        // §6.2's refusal, in the words of why it is one. A query has nothing to plan and never
        // will; a mutation with no row has a contract somebody could write, and the difference
        // is the difference between "ask this a different way" and "this is not supported yet".
        let detail = if registry
            .verb(contract.verb())
            .is_some_and(ono_command::VerbSpec::is_mutating)
        {
            format!(
                "§6.1: an operation is plannable only where a contract declares its target \
                 scope, its effects, its reversibility, the privilege it needs and how it is \
                 verified. `{}` mutates and `docs/contracts/change/actions.yaml` declares none \
                 of that for it, so this build knows it as a command and not as a mutation it \
                 can describe.",
                contract.id()
            )
        } else {
            format!(
                "§3.1: a plan describes a proposed *change*, and `{}` changes nothing — it \
                 answers a question. Run it directly, or pipe what it answers into a mutation \
                 and plan that (§5.3).",
                contract.id()
            )
        };
        return Err(error::action_not_plannable(&statement.source, &detail));
    };
    let bound = contract.bind(resolved.arguments)?;
    // §5.3: a pipeline supplies the objects the mutation acts on, and the statement then names
    // none. What was typed still wins over what the pipe carried, exactly as a context frame does.
    let from_pipe = piped_subjects(piped, operation.subject);
    let subjects = match subject_of(&bound, operation) {
        Some(named) => vec![named],
        None if !from_pipe.is_empty() => from_pipe,
        None => {
            return Err(error::target_unresolved(
                &statement.source,
                &format!(
                    "§4.3 turns a selector into a concrete object identity, and `{}` names no                      `{}` — write one, or pipe the objects in (§5.3).",
                    contract.spelling(),
                    operation.subject
                ),
            ));
        }
    };
    // ADR-0082 §1: the object a provider is asked about is the first declared selector that was
    // written, and a `path` selector is acted on rather than resolved. Recording which one it was
    // is what lets execution build the same action the shell's own mutation path builds.
    let identifying = contract
        .selectors()
        .iter()
        .find(|spec| bound.selector(spec.name()).is_some())
        .map_or_else(
            || (operation.subject, false),
            |spec| {
                (
                    spec.name(),
                    matches!(spec.declared_type(), ono_command::DeclaredType::Path),
                )
            },
        );
    let object = text_of(bound.selector(identifying.0));
    let mut arguments: Vec<(Arc<str>, Value)> = vec![
        (
            Arc::from("target"),
            Value::string(contract.target().unwrap_or(contract.verb())),
        ),
        (Arc::from("verb"), Value::string(contract.verb())),
    ];
    for option in contract.options() {
        if let Some(value) = bound.option(option.name()) {
            arguments.push((Arc::from(option.name()), value.clone()));
        }
    }
    // The selectors that did not supply the object travel under their own names, exactly as they
    // do for an ordinary mutation: a `destination` is an argument of the action, not a target.
    for selector in contract.selectors() {
        if selector.name() == identifying.0 {
            continue;
        }
        if let Some(value) = bound.selector(selector.name()) {
            arguments.push((Arc::from(selector.name()), value.clone()));
        }
    }
    Ok(Resolution::Operation {
        command: operation.command,
        operation,
        subjects,
        object,
        object_selector: Arc::from(identifying.0),
        object_is_path: identifying.1,
        arguments,
        source: statement.source.clone(),
    })
}

/// The subjects a pipeline in front of `plan` resolved, read by the field `field` names (§5.3).
///
/// Read from the field the operation's own selector names first — `path` for a file, `pid` for a
/// process, `name` for a service — because that is the field the object is frozen by (§7.1). A
/// stream of `ono.file/1` carries a `name` too, and it is the basename: frozen as the subject, it
/// would be resolved against the working directory and name a different file, or none. The
/// identifying fields follow for a record that lacks the selector's own. §2.6 freezes what this
/// answers: an object that starts matching after this point does not join the plan.
fn piped_subjects(input: &[Value], field: &str) -> Vec<String> {
    let mut subjects = Vec::new();
    for value in input {
        let Ok(record) = value.as_record() else {
            if let Ok(text) = value.as_str() {
                subjects.push(text.to_owned());
            }
            continue;
        };
        let found = [field, "path", "unit", "name"]
            .into_iter()
            .find_map(|name| match record.get(name) {
                Some(Value::String(text)) => Some(text.to_string()),
                Some(Value::Path(path)) => Some(path.display().to_string()),
                Some(Value::Int(number)) => Some(number.to_string()),
                _ => None,
            });
        if let Some(subject) = found {
            subjects.push(subject);
        }
    }
    subjects.sort();
    subjects.dedup();
    subjects
}

/// The refusal for a statement no command contract resolves (§6.2, ADR-0813).
///
/// A head that names a program is §6.2's external command, and §6.3's escape is the only way
/// past it. A head that names nothing at all is not an opaque action — it is a spelling nobody
/// declared, and saying so is more useful than offering to run it blind.
fn unresolvable(statement: &Statement, opaque: OpaquePermission) -> ErrorValue {
    let Some(program) = program_on_path(&statement.head) else {
        return error::action_not_plannable(
            &statement.source,
            &format!(
                "§6.1: `{}` resolves to no command contract and to no program on `PATH`, so \
                 nothing declares what it would change.",
                statement.head
            ),
        );
    };
    if !opaque.requested {
        return error::opaque_action_forbidden(&statement.source);
    }
    if !opaque.configured {
        return error::opaque_action_forbidden(&statement.source).with_help(format!(
            "`{program}` would run as §6.3's opaque action, and `change.allow_opaque_actions` is \
             not set on this host (§53). Setting it permits `--opaque`; the action's impact and \
             reversibility stay unknown and Appendix A.7 caps the plan at partially protected"
        ));
    }
    // The caller asked for the escape and the host permits it, so this is not a refusal at all.
    // It is reported through `resolve_opaque`, which the caller reaches after this check.
    error::opaque_action_forbidden(&statement.source)
}

/// §6.3's acknowledged escape, for a statement [`resolve`] refused and the caller admitted.
///
/// # Errors
///
/// `change.opaque_action_forbidden` where the escape was not granted, so the only path to an
/// opaque action is an explicit one (§6.2).
pub fn resolve_opaque(
    statement: &Statement,
    opaque: OpaquePermission,
) -> Result<Resolution, ErrorValue> {
    if !opaque.is_granted() {
        return Err(error::opaque_action_forbidden(&statement.source));
    }
    Ok(Resolution::Opaque {
        description: statement.source.clone(),
        program: program_on_path(&statement.head),
        argv: statement.words().into_iter().map(Arc::from).collect(),
    })
}

/// `verify <target> <identity> <expression…>` as §23.1's contract (ADR-0813).
fn verification_of(statement: &Statement) -> Result<Resolution, ErrorValue> {
    let words = statement.words();
    if words.len() < 3 {
        return Err(error::action_not_plannable(
            &statement.source,
            "§23.1: a verification line names what it is about and the condition it expects — \
             `verify service nginx state == running`, `verify socket :443 exists`.",
        ));
    }
    let subject = format!("{} {}", words[0], words[1]);
    let expression = words[2..].join(" ");
    Ok(Resolution::Verification {
        subject,
        expression,
    })
}

/// The object a statement mutates, read from the selector [`PlannableOperation::subject`] names.
///
/// Where the contract has no such selector — `copy file <source> to <destination>` writes the
/// destination as an option in some spellings and as a selector in others — every declared
/// selector and option of that name is consulted before the answer is `None`.
fn subject_of(
    bound: &ono_command::BoundArguments,
    operation: &PlannableOperation,
) -> Option<String> {
    text_of(
        bound
            .selector(operation.subject)
            .or_else(|| bound.option(operation.subject)),
    )
}

/// A bound value as the text an identity is written with.
fn text_of(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Null => None,
        Value::String(text) => Some(text.to_string()),
        Value::Path(path) => Some(path.display().to_string()),
        other => ono_value::canonical_text(other).ok(),
    }
}

/// The absolute path of `name` on `PATH`, where it is there (§6.2, §12.3).
///
/// A resolved absolute path rather than a name, because §2.17 puts a program and its argument
/// vector into the execution and leaves nowhere for a command line to live.
fn program_on_path(name: &str) -> Option<String> {
    let candidate = std::path::Path::new(name);
    if candidate.is_absolute() {
        return candidate.is_file().then(|| candidate.display().to_string());
    }
    if name.contains('/') {
        return None;
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
        .map(|candidate| candidate.display().to_string())
}

/// The plan fragment one resolved operation contributes (§51's shape, without a provider crate).
///
/// `provider` is the `ono-provider-api` provider that will carry the action out, so §4.4's
/// provider binding names the mechanism that will act rather than the contract that described it.
#[must_use]
pub fn fragment_for(
    plan: &PlanId,
    ordinal: usize,
    resolution: &Resolution,
    provider: &str,
    target: &ono_change_core::FrozenTarget,
) -> PlanFragment {
    match resolution {
        Resolution::Operation {
            operation,
            object,
            object_selector,
            object_is_path,
            arguments,
            source,
            ..
        } => {
            let action = PlanAction::new(
                plan,
                ordinal,
                ActionRole::Mutate,
                source.clone(),
                Execution::ProviderAction {
                    provider: Arc::from(provider),
                    operation: Arc::from(operation.command),
                    arguments: {
                        let mut carried = arguments.clone();
                        carried.push((
                            Arc::from("object"),
                            Value::string(object.as_deref().unwrap_or_else(|| target.label())),
                        ));
                        carried
                            .push((Arc::from("object_selector"), Value::string(object_selector)));
                        carried.push((Arc::from("object_is_path"), Value::Bool(*object_is_path)));
                        carried
                    },
                },
            )
            .on(target.identity())
            .with_idempotency(operation.declared.idempotency_for(&arguments.clone()))
            .recovery_semantics(operation.declared.recovery_semantics());
            // §7.2's existence precondition is "the object still exists and is still this
            // object", which is a statement about a *resolved* object. A plan that creates its
            // target froze an identity rather than resolving one — `add user alice` names a user
            // no provider can answer for — and §7.2 defines no counterpart for that, so nothing
            // is asserted rather than an inverted assertion invented.
            let mut action = action;
            for precondition in preconditions_of(operation, target) {
                action = action.requiring(precondition);
            }
            let action = if operation.privileged {
                action.privileged()
            } else {
                action
            };
            let id = action.id().clone();
            // Every effect the contract declares, not one: §8.2 asks what the operation does,
            // and `restart service` interrupts what the unit was serving as well as replacing
            // its processes. A plan that showed only the first would understate the change.
            //
            // The effect lands on the object as a person and a mount table both name it, which
            // is the label rather than the frozen identity: §11.2 maps a mutation domain onto a
            // persistence domain by path, and `/etc/nginx.conf@/dev/sda2` is not a path. A row
            // that names its own selector overrides that — `copy file <source> to <destination>`
            // creates the destination and reads the source.
            let mut acted = action;
            for spec in operation.declared.effects() {
                let object = spec
                    .object_selector()
                    .and_then(|selector| {
                        arguments
                            .iter()
                            .find(|(name, _)| name.as_ref() == selector)
                            .and_then(|(_, value)| value.as_str().ok().map(str::to_owned))
                    })
                    .unwrap_or_else(|| target.label().to_owned());
                let mut effect = ProposedEffect::new(
                    id.clone(),
                    spec.domain(),
                    spec.kind(),
                    spec.confidence(),
                    spec.explanation(),
                )
                .on(object)
                .citing(operation.command);
                if spec.is_irreversible() {
                    effect = effect.irreversible();
                }
                if let Some(compensation) = spec.compensation() {
                    effect = effect.compensated_by(compensation);
                }
                acted = acted.effecting(effect);
            }
            PlanFragment::empty()
                .acting(acted)
                .at_version(provider_version(provider))
        }
        Resolution::Opaque {
            description,
            program,
            argv,
        } => {
            let action = PlanAction::new(
                plan,
                ordinal,
                ActionRole::Mutate,
                format!("opaque action: {description}"),
                Execution::Opaque {
                    description: Arc::from(description.as_str()),
                    program: program.as_deref().map(Arc::from),
                    argv: argv.clone(),
                },
            )
            .on(target.identity())
            .with_idempotency(Idempotency::Unknown)
            .recovery_semantics(
                "none declared: §6.3 classifies an opaque action's impact and reversibility as \
                 unknown",
            );
            let id = action.id().clone();
            // §6.3 makes impact and reversibility unknown. The effect says so in its own terms —
            // domain, kind and confidence `unknown` — and is not marked irreversible: §19.4 gates
            // *known* irreversible actions, and that would be a claim nobody can make (ADR-0823).
            // The views list it under `not recoverable` as reversibility unknown. The explanation
            // leads with the command because the effect names no object, and the explanation is
            // what a view prints in its place.
            let effect = ProposedEffect::new(
                id,
                EffectDomain::Unknown,
                EffectKind::Unknown,
                EffectConfidence::Unknown,
                format!(
                    "opaque action `{description}`: the operator acknowledged that Ono cannot \
                     reason about what this command touches (§6.3), so its domain, its scope and \
                     its reversibility are all unknown"
                ),
            );
            PlanFragment::empty().acting(action.effecting(effect))
        }
        Resolution::Verification { .. } => PlanFragment::empty(),
    }
}

/// What §23.1 asks the world after `operation` ran on `target`, or `None` where it declares none.
///
/// It is built beside the fragment rather than inside it because a `verify` line in a §5.2 block
/// may restate the same postcondition, and [`CheckId`](ono_change_core::CheckId) is derived from
/// the subject and the expression — two contracts saying the same thing would share an identity,
/// and a result could then not be attributed to either. The caller holds both lists and keeps one
/// contract per condition (ADR-0813).
#[must_use]
pub fn contracts_for(
    plan: &PlanId,
    operation: &PlannableOperation,
    target: &ono_change_core::FrozenTarget,
    arguments: &[(Arc<str>, Value)],
) -> Vec<VerificationContract> {
    // §8.2's first effect is what the operation is *for*, so it is the domain the check reports
    // equivalence in. An operation with no effect never reaches here: the registry refuses one.
    let domain = operation.declared.effects().first().map_or(
        EffectDomain::Unknown,
        ono_change_actions::EffectSpec::domain,
    );
    operation
        .declared
        .verification()
        .iter()
        .filter_map(|spec| {
            // §23.1 asks for checks that can be answered, and a contract whose `{option:<name>}`
            // has no value is not one — `--version 1.2` is what makes a version check askable,
            // and without it the check is about nothing.
            // A `{digest:<argument>}` needs one file to hash. Where there is none — a recursive copy
            // of a directory — the check that can still be answered is that the object is there.
            let expression = substituted(spec.expression(), arguments).or_else(|| {
                spec.expression()
                    .contains("{digest:")
                    .then(|| "exists".to_owned())
            })?;
            // The registry's `subject` is the noun a person reads — "the service", "the process"
            // — and §23.1's check is asked of the world through a provider, which is found by
            // target. So the subject the contract carries is `<target> <label>`: `service nginx`
            // names which provider to ask, where `the service nginx` names none.
            //
            // The object is named by the value it was frozen by (§7.1). For most targets that is
            // the name the label already shows; a process is frozen by its pid, and `process
            // sleep` would be a check about whichever `sleep` answered first.
            let subject = format!(
                "{} {}",
                operation.shape.target_word(),
                frozen_key(operation, target)
            );
            let mut contract =
                VerificationContract::new(plan, spec.class(), subject, expression.clone())
                    .about(equivalence_of(domain));
            if let Some((_, expected)) = expression.split_once("==") {
                contract = contract.expecting(Value::string(expected.trim()));
            }
            Some(contract)
        })
        .collect()
}

/// The value `target` was frozen by, as a verification subject names it (§7.1, §23.1).
///
/// A target selected by `name` is named by its label. One selected by any other field — a
/// process by `pid`, a container by `id` — is named by the key its identity carries,
/// `<namespace>:<key>[#generation=…]`, because its label is a display name and not an identity.
fn frozen_key<'a>(
    operation: &PlannableOperation,
    target: &'a ono_change_core::FrozenTarget,
) -> &'a str {
    if operation.subject == "name" || !matches!(operation.shape, TargetShape::Named(_)) {
        return target.label();
    }
    target
        .identity()
        .split_once(':')
        .and_then(|(_, rest)| rest.split('#').next())
        .filter(|key| !key.is_empty())
        .unwrap_or_else(|| target.label())
}

/// `expression` with every `{option:<name>}` replaced, or `None` where one has no value (§23.1).
fn substituted(expression: &str, arguments: &[(Arc<str>, Value)]) -> Option<String> {
    let mut filled = String::with_capacity(expression.len());
    let mut rest = expression;
    while let Some(start) = rest.find("{option:") {
        let (before, tail) = rest.split_at(start);
        filled.push_str(before);
        let inner = tail.strip_prefix("{option:")?;
        let (name, tail) = inner.split_once('}')?;
        let value = arguments
            .iter()
            .find(|(argument, _)| argument.as_ref() == name)
            .map(|(_, value)| value)?;
        filled.push_str(&ono_value::canonical_text(value).ok()?);
        rest = tail;
    }
    filled.push_str(rest);
    // `{digest:<argument>}` is the SHA-256 of the file that argument names, taken now: §25.1's
    // persistent-state check for a copy is that the destination holds the bytes the source held
    // when the plan was sealed, and "the destination exists" establishes nothing about them.
    let mut digested = String::with_capacity(filled.len());
    let mut rest = filled.as_str();
    while let Some(start) = rest.find("{digest:") {
        let (before, tail) = rest.split_at(start);
        digested.push_str(before);
        let inner = tail.strip_prefix("{digest:")?;
        let (name, tail) = inner.split_once('}')?;
        // A path argument is a string, and its canonical text is quoted; the file to hash is the
        // string itself.
        let path = arguments
            .iter()
            .find(|(argument, _)| argument.as_ref() == name)
            .and_then(|(_, value)| {
                value
                    .as_str()
                    .ok()
                    .map(str::to_owned)
                    .or_else(|| ono_value::canonical_text(value).ok())
            })?;
        digested.push_str(&digest_of(&path)?);
        rest = tail;
    }
    digested.push_str(rest);
    Some(digested)
}

/// The SHA-256 of the file at `path`, or `None` where there is no file to hash.
fn digest_of(path: &str) -> Option<String> {
    if !path.starts_with('/') {
        return None;
    }
    std::fs::read(path)
        .ok()
        .map(|bytes| ono_recovery_files::manifest::digest_of(&bytes))
}

/// §7.2's preconditions for one action on one frozen target.
///
/// The kinds come from the registry, because §6.1 makes them part of the contract and §7.2 makes
/// them per-operation: `copy file` depends on the destination's bytes and on the persistence
/// domain that holds them, and `restart service` on the unit's generation. The *values* come
/// from the frozen target, which is what §4.3 froze them for.
///
/// A kind whose fact the frozen target does not carry is not emitted. §2.4 is why: a precondition
/// against `unknown` is a check that can never pass, and a plan carrying one would refuse every
/// apply for a fact nobody ever recorded.
pub(super) fn preconditions_of(
    operation: &PlannableOperation,
    target: &ono_change_core::FrozenTarget,
) -> Vec<Precondition> {
    let mut preconditions = Vec::new();
    for kind in operation.declared.preconditions() {
        let precondition = match kind {
            // §7.2's existence is about a *resolved* object. A plan that creates its target
            // froze an identity rather than resolving one, and §7.2 defines no counterpart.
            PreconditionKind::Existence if target.selector().is_some() => Precondition::new(
                PreconditionKind::Existence,
                target.identity(),
                "exists",
                Value::Bool(true),
            )
            .explained(
                "§7.2: the object the plan was resolved against still exists and is still this \
                 object",
            ),
            // §7.2 verbatim: "the file's hash still equals what was resolved". The digest is
            // taken here, at freeze time, and it is kept out of the identity on purpose — every
            // layer that has to recognise the object again reads the identity as a path, and a
            // file whose bytes moved is the same file. That difference is drift, which is what
            // this precondition is.
            PreconditionKind::ContentDigest => match digest_of(target.label()) {
                Some(digest) => Precondition::new(
                    PreconditionKind::ContentDigest,
                    target.identity(),
                    "sha256",
                    Value::string(&digest),
                )
                .explained(
                    "§7.2: the file's bytes are the ones the plan was resolved against, so a \
                     change made after the seal stops the apply rather than being overwritten",
                ),
                // A file the plan is about to create has no bytes to freeze, and §2.4 forbids
                // a check against a fact nobody established.
                None => continue,
            },
            // A unit's generation is part of the identity §4.3 froze, so an object replaced
            // underneath the plan is a different identity and the identity is the check.
            PreconditionKind::Generation if target.identity().contains('#') => Precondition::new(
                PreconditionKind::Generation,
                target.identity(),
                "identity",
                Value::string(target.identity()),
            )
            .explained(
                "§7.2: the object's generation is part of the identity the plan froze, so an \
                 object restarted or replaced underneath it is not this object",
            ),
            // §7.2 verbatim: "package installed version still equals Z". The version is the one
            // §4.3 froze into the package's identity; an object that has none — a package the
            // plan installs — has no version to hold, and §2.4 forbids inventing one.
            PreconditionKind::Version => match installed_version(target) {
                Some(version) => Precondition::new(
                    PreconditionKind::Version,
                    target.identity(),
                    "version",
                    Value::string(version),
                )
                .explained(
                    "§7.2: the installed version is the one the plan was resolved against, so a \
                     package upgraded or removed after the seal stops the apply",
                ),
                None => continue,
            },
            PreconditionKind::PersistenceDomain => match target.persistence_domain() {
                Some(domain) => Precondition::new(
                    PreconditionKind::PersistenceDomain,
                    target.identity(),
                    "persistence_domain",
                    Value::string(domain),
                )
                .explained(
                    "§7.2 and Appendix B: the path still resolves to the dataset or subvolume \
                     the protection was planned against",
                ),
                None => continue,
            },
            // §7.2's provider and capability checks are made by the executor before prepare
            // (§43.2), against the session rather than against the world, so they are not
            // frozen facts and carry no value a drift check could compare.
            PreconditionKind::ProviderAvailable | PreconditionKind::Capability => continue,
            // `field` names no fact by itself — an operation declaring it would have to say
            // which field — and §43.5's path identity below is the one this module freezes.
            PreconditionKind::Field => continue,
            // A guarded kind whose fact this target does not carry: §2.4 again.
            _ => continue,
        };
        preconditions.push(precondition);
    }
    preconditions.extend(path_identities(target));
    preconditions
}

/// The installed version a frozen package carries, where it carries one (§7.2).
fn installed_version(target: &ono_change_core::FrozenTarget) -> Option<&str> {
    if target.schema() != super::world::PACKAGE_SCHEMA {
        return None;
    }
    target
        .identity()
        .split_once('#')
        .map(|(_, version)| version)
        .filter(|version| !version.is_empty())
}

/// §43.5's path identities for a file target: what the path is, read without following it.
///
/// "Paths, symlinks and identities MUST be revalidated using safe filesystem APIs." The content
/// digest reads through a link, so a link swapped in over the path — to an object with the very
/// same bytes — passes every other precondition, and the mutation then writes through it. The
/// file type, device and inode of the path itself do not survive that swap.
///
/// Two paths are frozen where the operator's spelling is not the canonical one: the canonical
/// path the plan resolved, and the path as written — a symlink the action will be carried out
/// through, and that could be re-pointed between the seal and the apply.
fn path_identities(target: &ono_change_core::FrozenTarget) -> Vec<Precondition> {
    // Only a resolved object has an identity to hold: a file the plan creates was not there.
    let Some(spelled) = target.selector() else {
        return Vec::new();
    };
    if target.schema() != ono_change_plan::freeze::FILE_SCHEMA {
        return Vec::new();
    }
    let canonical = std::path::Path::new(target.label());
    let mut paths = vec![(target.identity().to_owned(), canonical.to_path_buf())];
    if let Ok(spelled) = std::path::absolute(spelled)
        && spelled != canonical
    {
        paths.push((spelled.display().to_string(), spelled));
    }
    paths
        .into_iter()
        .filter_map(|(subject, path)| {
            let found = super::world::lstat_identity(&path).ok()?;
            Some(
                Precondition::new(
                    PreconditionKind::Field,
                    subject,
                    super::world::LSTAT_FIELD,
                    Value::string(&found),
                )
                .explained(
                    "§43.5: the path is still the object the plan was resolved against — the same \
                     type, device and inode, read without following a symlink",
                ),
            )
        })
        .collect()
}

/// The one thing §23.1 lets a plan say about an opaque action (§6.3, §23.3).
///
/// §23.1 requires every mutating plan to carry a contract "even where the minimum is
/// provider-level state acknowledgement", and §6.3 has already said that Ono cannot reason about
/// what an opaque action touches. The honest contract is therefore an *observational* one whose
/// answer is `UNKNOWN`: it records that the plan will look, that nothing it sees can confirm the
/// intent, and that a failure here says nothing about the plan's state (§23.2). Declaring a
/// required check over an action nobody modelled would be the invented future §1.3 forbids.
#[must_use]
pub fn opaque_contract(plan: &PlanId, description: &str) -> VerificationContract {
    VerificationContract::new(
        plan,
        VerificationClass::Observational,
        format!("opaque action: {description}"),
        "unknown",
    )
    .about(ono_change_core::EquivalenceDomain::ExternalSideEffect)
    .timeout_is_unknown()
}

/// The equivalence domain a verification of `domain` is evidence about (§25.1).
const fn equivalence_of(domain: EffectDomain) -> ono_change_core::EquivalenceDomain {
    use ono_change_core::EquivalenceDomain;
    match domain {
        EffectDomain::ProcessRuntime
        | EffectDomain::KernelRuntime
        | EffectDomain::NetworkRuntime
        | EffectDomain::ProviderTransactionState => EquivalenceDomain::RuntimeState,
        EffectDomain::ExternalSideEffect | EffectDomain::RemoteSystem => {
            EquivalenceDomain::ExternalSideEffect
        }
        _ => EquivalenceDomain::PersistentState,
    }
}

/// The version §4.4 records a provider binding at.
///
/// A provider that does not publish one is recorded as `unknown` rather than as a number nobody
/// stated: Appendix G.4 requires a provider to degrade to unsupported rather than execute
/// semantics it has not validated, and inventing a version here would hide exactly that.
fn provider_version(_provider: &str) -> &'static str {
    "unknown"
}
