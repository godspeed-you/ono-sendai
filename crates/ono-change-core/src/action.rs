//! Plan actions (spec v0.6 §3.3, §46.2) and the structured execution they carry.
//!
//! §2.17 is the rule this module is built around: *"Provider operations MUST be structured
//! execution plans, not interpolated shell command strings."* [`Execution`] therefore has no
//! variant that holds a command line. A program is a path plus an argument vector, a provider
//! action is an operation id plus typed arguments, and there is nowhere for a quoted, expanded,
//! word-split string to live. §12.3 and §43.6 fall out of that shape rather than resting on
//! review.
//!
//! §41.1's idempotency class is on the action rather than on the command, because §41.2 forbids
//! blindly rerunning an unknown or non-idempotent action after a crash — a decision the resume
//! path has to make per action, from what the plan recorded before it ran.

use std::sync::Arc;

use ono_value::Value;

use crate::effect::ProposedEffect;
use crate::id::{ActionId, PlanId};
use crate::target::Precondition;
use crate::vocab::vocabulary;

vocabulary! {
    /// What one action is for (§3.3).
    ActionRole {
        Prepare => "prepare", "§3.3: create protection, acquire leases, validate prerequisites. §4.5 runs these immediately before the first mutation.";
        Mutate => "mutate", "§3.3: change target system state.";
        Verify => "verify", "§3.3: establish whether the expected state exists (§23).";
        Recover => "recover", "§3.3: restore or compensate after failure (§24).";
        Cleanup => "cleanup", "§3.3: remove temporary resources once retention permits (§37).";
    }
}

impl ActionRole {
    /// Whether an action in this role changes the target system (§4.7, Appendix F).
    #[must_use]
    pub const fn mutates_target(self) -> bool {
        matches!(self, ActionRole::Mutate | ActionRole::Recover)
    }

    /// Whether an action in this role runs before the first mutation (§4.5).
    #[must_use]
    pub const fn is_preparation(self) -> bool {
        matches!(self, ActionRole::Prepare)
    }
}

vocabulary! {
    /// Whether an action may be run again after an uncertain outcome (§41.1).
    Idempotency {
        Idempotent => "idempotent", "§41.1: running it twice is running it once.";
        RetrySafeWithToken => "retry-safe-with-token", "§41.1: safe to retry when the same request token is presented.";
        NonIdempotent => "non-idempotent", "§41.1: running it twice is not running it once.";
        Unknown => "unknown", "§41.1: the contract does not say, which §41.2 treats as not safe to retry.";
    }
}

impl Idempotency {
    /// Whether resume may rerun an action whose outcome is unknown (§41.2, §41.3).
    ///
    /// `UNKNOWN` answers `false`. §41.2 says Ono "MUST NOT blindly rerun unknown/non-idempotent
    /// actions", and a default of "probably fine" is exactly the blind rerun it forbids.
    #[must_use]
    pub const fn permits_blind_retry(self) -> bool {
        matches!(self, Idempotency::Idempotent)
    }

    /// Whether resume may rerun the action when it can present the original request token.
    #[must_use]
    pub const fn permits_token_retry(self) -> bool {
        matches!(
            self,
            Idempotency::Idempotent | Idempotency::RetrySafeWithToken
        )
    }
}

vocabulary! {
    /// What is known about one action after the executor ran it (§4.7, Appendix F.2).
    ActionStatus {
        Pending => "pending", "The action has not been started.";
        Running => "running", "The action is in flight.";
        Succeeded => "succeeded", "The action completed and the provider said so.";
        Failed => "failed", "The action did not complete, and the provider said so.";
        Skipped => "skipped", "The action was not run because something it depends on did not happen.";
        Unknown => "unknown", "Appendix F.2: the outcome could not be established — a remote link dropped, a process vanished. It is not a failure and it is not a success.";
    }
}

impl ActionStatus {
    /// Whether the action may have changed the system.
    ///
    /// `UNKNOWN` answers `true`. Appendix F.2 makes an unestablished outcome an uncertainty
    /// boundary for recovery planning, and treating it as "nothing happened" is the one reading
    /// that can lose data.
    #[must_use]
    pub const fn may_have_mutated(self) -> bool {
        matches!(
            self,
            ActionStatus::Running
                | ActionStatus::Succeeded
                | ActionStatus::Failed
                | ActionStatus::Unknown
        )
    }

    /// Whether the action is finished, whatever the outcome was.
    #[must_use]
    pub const fn is_settled(self) -> bool {
        matches!(
            self,
            ActionStatus::Succeeded
                | ActionStatus::Failed
                | ActionStatus::Skipped
                | ActionStatus::Unknown
        )
    }
}

/// How an action is carried out — structured, never a shell string (§2.17, §12.3).
#[derive(Debug, Clone, PartialEq)]
pub enum Execution {
    /// A native provider action: an operation id, a target and typed arguments.
    ProviderAction {
        /// The provider that owns the operation.
        provider: Arc<str>,
        /// The operation id, such as `ono.service.restart`.
        operation: Arc<str>,
        /// The arguments, already typed. Nothing here is interpolated into text.
        arguments: Vec<(Arc<str>, Value)>,
    },
    /// A program run directly: an absolute path and an argument vector (§12.3).
    ///
    /// There is no shell. `program` is a resolved path and `argv` is passed to `execve` as it
    /// stands, so a dataset name containing a semicolon is a dataset name containing a semicolon.
    Program {
        /// The resolved absolute path of the program.
        program: Arc<str>,
        /// The arguments, one element per argument, unquoted and unexpanded.
        argv: Vec<Arc<str>>,
    },
    /// A recovery provider operation, named by capability (§12.1, §12.2).
    RecoveryOperation {
        /// The recovery provider id, such as `ono.recovery.zfs`.
        provider: Arc<str>,
        /// The capability being exercised, such as `recovery.prepare`.
        capability: Arc<str>,
        /// The operation's typed arguments.
        arguments: Vec<(Arc<str>, Value)>,
    },
    /// An action the operator declared opaque, whose scope Ono cannot reason about (§6.3).
    ///
    /// It carries no protection claim of its own, and Appendix A.7 caps the plan it appears in.
    Opaque {
        /// What the operator said the action does.
        description: Arc<str>,
        /// The resolved program, where one was named.
        program: Option<Arc<str>>,
        /// The argument vector, where one was given.
        argv: Vec<Arc<str>>,
    },
}

impl Execution {
    /// The provider or program this execution runs through, for the plan's provider bindings.
    #[must_use]
    pub fn actor(&self) -> &str {
        match self {
            Execution::ProviderAction { provider, .. }
            | Execution::RecoveryOperation { provider, .. } => provider,
            Execution::Program { program, .. } => program,
            Execution::Opaque { program, .. } => program.as_deref().unwrap_or("opaque"),
        }
    }

    /// Whether Ono can reason about what this execution touches (§6.2, §6.3).
    #[must_use]
    pub const fn is_opaque(&self) -> bool {
        matches!(self, Execution::Opaque { .. })
    }

    /// The canonical text this execution contributes to a plan digest (§4.4).
    #[must_use]
    pub fn digest_text(&self) -> String {
        match self {
            Execution::ProviderAction {
                provider,
                operation,
                arguments,
            }
            | Execution::RecoveryOperation {
                provider,
                capability: operation,
                arguments,
            } => {
                let mut text = format!("{provider}\u{1f}{operation}");
                for (name, value) in arguments {
                    use std::fmt::Write as _;
                    let _ = write!(text, "\u{1f}{name}={}", crate::digest::value_text(value));
                }
                text
            }
            Execution::Program { program, argv } => {
                format!("program\u{1f}{program}\u{1f}{}", argv.join("\u{1f}"))
            }
            Execution::Opaque {
                description,
                program,
                argv,
            } => format!(
                "opaque\u{1f}{description}\u{1f}{}\u{1f}{}",
                program.as_deref().unwrap_or(""),
                argv.join("\u{1f}")
            ),
        }
    }
}

/// One semantically meaningful operation in a plan (§3.3, §46.2).
#[derive(Debug, Clone, PartialEq)]
pub struct PlanAction {
    id: ActionId,
    ordinal: usize,
    role: ActionRole,
    summary: Arc<str>,
    target: Option<Arc<str>>,
    execution: Execution,
    depends_on: Vec<ActionId>,
    preconditions: Vec<Precondition>,
    idempotency: Idempotency,
    effects: Vec<ProposedEffect>,
    recovery_semantics: Option<Arc<str>>,
    requires_privilege: bool,
    status: ActionStatus,
}

impl PlanAction {
    /// Declares the `ordinal`-th action of `plan`.
    #[must_use]
    pub fn new(
        plan: &PlanId,
        ordinal: usize,
        role: ActionRole,
        summary: impl Into<Arc<str>>,
        execution: Execution,
    ) -> Self {
        let summary = summary.into();
        Self {
            id: ActionId::of(plan, ordinal, &summary),
            ordinal,
            role,
            summary,
            target: None,
            execution,
            depends_on: Vec::new(),
            preconditions: Vec::new(),
            idempotency: Idempotency::Unknown,
            effects: Vec::new(),
            recovery_semantics: None,
            requires_privilege: false,
            status: ActionStatus::Pending,
        }
    }

    /// Rebuilds an action out of the fields a store read back (§41.2).
    ///
    /// `pub(crate)` for the same reason as [`crate::ChangePlan::restore`]: an action's identity
    /// and its recorded status are facts about what happened, and re-deriving them from a summary
    /// would quietly discard the evidence resume depends on.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub(crate) fn restore(
        id: ActionId,
        ordinal: usize,
        role: ActionRole,
        summary: Arc<str>,
        target: Option<Arc<str>>,
        execution: Execution,
        depends_on: Vec<ActionId>,
        preconditions: Vec<Precondition>,
        idempotency: Idempotency,
        effects: Vec<ProposedEffect>,
        recovery_semantics: Option<Arc<str>>,
        requires_privilege: bool,
        status: ActionStatus,
    ) -> Self {
        Self {
            id,
            ordinal,
            role,
            summary,
            target,
            execution,
            depends_on,
            preconditions,
            idempotency,
            effects,
            recovery_semantics,
            requires_privilege,
            status,
        }
    }

    /// Names the frozen target this action acts on.
    #[must_use]
    pub fn on(mut self, target: impl Into<Arc<str>>) -> Self {
        self.target = Some(target.into());
        self
    }

    /// Declares that this action runs after `earlier` (§3.2's action graph).
    #[must_use]
    pub fn after(mut self, earlier: ActionId) -> Self {
        self.depends_on.push(earlier);
        self
    }

    /// Adds a precondition the action needs (§7.2).
    #[must_use]
    pub fn requiring(mut self, precondition: Precondition) -> Self {
        self.preconditions.push(precondition);
        self
    }

    /// Declares the action's idempotency class (§41.1).
    #[must_use]
    pub const fn with_idempotency(mut self, idempotency: Idempotency) -> Self {
        self.idempotency = idempotency;
        self
    }

    /// Declares an effect the action may have (§8).
    #[must_use]
    pub fn effecting(mut self, effect: ProposedEffect) -> Self {
        self.effects.push(effect);
        self
    }

    /// States the action's recovery semantics, or its explicit lack of them (§6.1).
    #[must_use]
    pub fn recovery_semantics(mut self, semantics: impl Into<Arc<str>>) -> Self {
        self.recovery_semantics = Some(semantics.into());
        self
    }

    /// Marks the action as needing elevated privilege (§43.3).
    #[must_use]
    pub const fn privileged(mut self) -> Self {
        self.requires_privilege = true;
        self
    }

    /// Records what the executor now knows about the action.
    #[must_use]
    pub const fn with_status(mut self, status: ActionStatus) -> Self {
        self.status = status;
        self
    }

    /// The action's identity.
    #[must_use]
    pub const fn id(&self) -> &ActionId {
        &self.id
    }

    /// Its position in the plan, which is also its display number.
    #[must_use]
    pub const fn ordinal(&self) -> usize {
        self.ordinal
    }

    /// What the action is for.
    #[must_use]
    pub const fn role(&self) -> ActionRole {
        self.role
    }

    /// The line a person reads.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// The frozen target, where the action names one.
    #[must_use]
    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    /// How the action is carried out.
    #[must_use]
    pub const fn execution(&self) -> &Execution {
        &self.execution
    }

    /// The actions this one runs after.
    #[must_use]
    pub fn depends_on(&self) -> &[ActionId] {
        &self.depends_on
    }

    /// The preconditions the action needs.
    #[must_use]
    pub fn preconditions(&self) -> &[Precondition] {
        &self.preconditions
    }

    /// The action's idempotency class.
    #[must_use]
    pub const fn idempotency(&self) -> Idempotency {
        self.idempotency
    }

    /// The effects the action may have.
    #[must_use]
    pub fn effects(&self) -> &[ProposedEffect] {
        &self.effects
    }

    /// The action's recovery semantics, where the contract states them.
    #[must_use]
    pub fn declared_recovery(&self) -> Option<&str> {
        self.recovery_semantics.as_deref()
    }

    /// Whether the action needs elevated privilege (§43.3).
    #[must_use]
    pub const fn needs_privilege(&self) -> bool {
        self.requires_privilege
    }

    /// What is known about the action's outcome.
    #[must_use]
    pub const fn status(&self) -> ActionStatus {
        self.status
    }

    /// Whether resume may rerun this action given what is known about it (§41.3).
    #[must_use]
    pub const fn may_resume(&self) -> bool {
        match self.status {
            ActionStatus::Pending | ActionStatus::Skipped => true,
            ActionStatus::Running | ActionStatus::Unknown => self.idempotency.permits_blind_retry(),
            ActionStatus::Failed => self.idempotency.permits_token_retry(),
            ActionStatus::Succeeded => false,
        }
    }

    /// The canonical text this action contributes to a plan digest (§4.4).
    #[must_use]
    pub fn digest_text(&self) -> String {
        let mut text = format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            self.id.as_str(),
            self.role.as_str(),
            self.target.as_deref().unwrap_or(""),
            self.execution.digest_text(),
            self.idempotency.as_str(),
            u8::from(self.requires_privilege),
        );
        for dependency in &self.depends_on {
            text.push('\u{1f}');
            text.push_str(dependency.as_str());
        }
        for precondition in &self.preconditions {
            text.push('\u{1f}');
            text.push_str(&precondition.digest_text());
        }
        for effect in &self.effects {
            text.push('\u{1f}');
            text.push_str(&effect.digest_text());
        }
        text
    }
}

/// Orders `actions` so every action follows the ones it depends on, or names the cycle (§3.2).
///
/// §3.2 requires the action graph to be acyclic unless a provider-local transaction encapsulates
/// the cycle, so a cycle is a defect in the plan and is reported as one rather than deadlocking
/// the executor.
///
/// # Errors
///
/// Returns the identities that form a cycle, in the order they were declared.
pub fn topological_order(actions: &[PlanAction]) -> Result<Vec<usize>, Vec<ActionId>> {
    let mut indegree: Vec<usize> = vec![0; actions.len()];
    let mut edges: Vec<Vec<usize>> = vec![Vec::new(); actions.len()];
    for (index, action) in actions.iter().enumerate() {
        for dependency in action.depends_on() {
            if let Some(source) = actions
                .iter()
                .position(|candidate| candidate.id() == dependency)
            {
                edges[source].push(index);
                indegree[index] += 1;
            }
        }
    }
    // Roles order the frontier: §4.5 runs preparation before mutation, and a ready PREPARE action
    // must not sit behind a ready MUTATE one merely because it was declared later.
    let mut ready: Vec<usize> = (0..actions.len())
        .filter(|index| indegree[*index] == 0)
        .collect();
    ready.sort_by_key(|index| (role_rank(actions[*index].role()), actions[*index].ordinal()));
    let mut order = Vec::with_capacity(actions.len());
    while let Some(next) = ready.first().copied() {
        ready.remove(0);
        order.push(next);
        for target in &edges[next] {
            indegree[*target] -= 1;
            if indegree[*target] == 0 {
                ready.push(*target);
            }
        }
        ready.sort_by_key(|index| (role_rank(actions[*index].role()), actions[*index].ordinal()));
    }
    if order.len() == actions.len() {
        Ok(order)
    } else {
        Err(actions
            .iter()
            .enumerate()
            .filter(|(index, _)| !order.contains(index))
            .map(|(_, action)| action.id().clone())
            .collect())
    }
}

const fn role_rank(role: ActionRole) -> u8 {
    match role {
        ActionRole::Prepare => 0,
        ActionRole::Mutate => 1,
        ActionRole::Verify => 2,
        ActionRole::Recover => 3,
        ActionRole::Cleanup => 4,
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

    fn plan() -> PlanId {
        PlanId::derive(&["p"])
    }

    fn program(name: &str) -> Execution {
        Execution::Program {
            program: Arc::from("/usr/sbin/zfs"),
            argv: vec![Arc::from("snapshot"), Arc::from(name)],
        }
    }

    fn action(ordinal: usize, role: ActionRole, summary: &str) -> PlanAction {
        PlanAction::new(&plan(), ordinal, role, summary, program(summary))
    }

    #[test]
    fn should_keep_an_argument_vector_out_of_one_string() {
        let execution = Execution::Program {
            program: Arc::from("/usr/sbin/zfs"),
            argv: vec![Arc::from("snapshot"), Arc::from("tank/data@ono; rm -rf /")],
        };
        let Execution::Program { argv, .. } = &execution else {
            panic!("the variant is a program");
        };
        assert_eq!(
            argv.len(),
            2,
            "§2.17 and §43.6: a name containing shell syntax is one argument, not two commands"
        );
    }

    #[test]
    fn should_refuse_to_retry_an_unknown_action_blindly() {
        let unknown = action(1, ActionRole::Mutate, "restart")
            .with_idempotency(Idempotency::Unknown)
            .with_status(ActionStatus::Unknown);
        assert!(
            !unknown.may_resume(),
            "§41.2 and §55.9 case 41: a crash after an unknown action does not blindly retry"
        );
    }

    #[test]
    fn should_resume_an_idempotent_action_whose_outcome_is_unknown() {
        let idempotent = action(1, ActionRole::Mutate, "write file")
            .with_idempotency(Idempotency::Idempotent)
            .with_status(ActionStatus::Unknown);
        assert!(
            idempotent.may_resume(),
            "§55.9 case 40: a crash after an idempotent action can resume safely"
        );
    }

    #[test]
    fn should_never_rerun_an_action_that_already_succeeded() {
        let done = action(1, ActionRole::Mutate, "restart")
            .with_idempotency(Idempotency::Idempotent)
            .with_status(ActionStatus::Succeeded);
        assert!(!done.may_resume(), "a completed action is not resumed");
    }

    #[test]
    fn should_treat_an_unknown_outcome_as_possibly_having_changed_the_system() {
        assert!(
            ActionStatus::Unknown.may_have_mutated(),
            "Appendix F.2: an unestablished outcome is an uncertainty boundary, not a no-op"
        );
        assert!(!ActionStatus::Pending.may_have_mutated());
        assert!(!ActionStatus::Skipped.may_have_mutated());
    }

    #[test]
    fn should_order_preparation_before_mutation() {
        let actions = vec![
            action(1, ActionRole::Mutate, "replace config"),
            action(2, ActionRole::Prepare, "snapshot dataset"),
        ];
        let order = topological_order(&actions).expect("no cycle");
        assert_eq!(
            actions[order[0]].role(),
            ActionRole::Prepare,
            "§4.5: preparation runs immediately before the first mutating action"
        );
    }

    #[test]
    fn should_respect_a_declared_dependency() {
        let first = action(1, ActionRole::Mutate, "replace config");
        let second = action(2, ActionRole::Mutate, "restart service").after(first.id().clone());
        let actions = vec![second, first];
        let order = topological_order(&actions).expect("no cycle");
        let ordinals: Vec<usize> = order
            .iter()
            .map(|index| actions[*index].ordinal())
            .collect();
        assert_eq!(
            ordinals,
            vec![1, 2],
            "the graph decides the order, not the declaration order"
        );
    }

    #[test]
    fn should_report_a_cycle_rather_than_looping() {
        let mut first = action(1, ActionRole::Mutate, "a");
        let second = action(2, ActionRole::Mutate, "b").after(first.id().clone());
        first = first.after(second.id().clone());
        let actions = vec![first, second];
        let cycle = topological_order(&actions).expect_err("§3.2 requires a DAG");
        assert_eq!(
            cycle.len(),
            2,
            "the refusal names the actions that form the cycle"
        );
    }

    #[test]
    fn should_change_the_digest_when_an_actions_arguments_change() {
        let one = action(1, ActionRole::Prepare, "snapshot tank/data");
        let two = action(1, ActionRole::Prepare, "snapshot tank/other");
        assert_ne!(
            one.digest_text(),
            two.digest_text(),
            "§4.4: the action graph is part of the seal"
        );
    }

    #[test]
    fn should_mark_an_opaque_action_as_something_ono_cannot_reason_about() {
        let opaque = Execution::Opaque {
            description: Arc::from("sh -c 'rm -rf /somewhere'"),
            program: None,
            argv: Vec::new(),
        };
        assert!(
            opaque.is_opaque(),
            "§6.3: an opaque action's impact and reversibility are unknown"
        );
    }
}
