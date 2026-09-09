//! Mutation domains, the first step of the protection coverage algorithm (Appendix A.1, A.2).
//!
//! Appendix A.1 is normative and short: *"For each MUTATE action, Ono MUST derive one or more
//! `MutationDomain` records."* The domains come from the effects the provider declared, which is
//! what makes Appendix A.1's own worked examples fall out rather than being special-cased —
//! `restart service nginx` touches `process-runtime` and `network-runtime` and no persistent
//! domain because a restart declares no persistent effect, and the same plan with a
//! configuration replacement touches `filesystem-persistent` because that action does.
//!
//! Appendix A.2 then assigns each domain a recovery objective, and [`objective_for`] is that
//! assignment as a default. Appendix A.2 says the objective follows "plan intent and policy", so
//! a policy layer may override one; what it may not do is leave a mutated domain without one,
//! because Appendix A.5 composes coverage over exactly these records.
//!
//! The output is deliberately free of provider knowledge. It names a domain, an objective and the
//! objects involved, and says nothing about datasets, subvolumes or snapshots: which provider
//! could cover a domain is Appendix A.3's question, and it is asked in another crate.

use std::sync::Arc;

use ono_change_core::{EffectDomain, PlanAction, RecoveryObjective};

/// One domain a plan mutates, what recovery would have to achieve there, and what is involved
/// (Appendix A.1, A.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationDomain {
    domain: EffectDomain,
    objective: RecoveryObjective,
    objects: Vec<Arc<str>>,
}

impl MutationDomain {
    /// Records that `domain` is mutated and needs `objective`.
    #[must_use]
    pub fn new(domain: EffectDomain, objective: RecoveryObjective) -> Self {
        Self {
            domain,
            objective,
            objects: Vec::new(),
        }
    }

    /// Adds an object the mutation lands on, ignoring one already recorded.
    #[must_use]
    pub fn involving(mut self, object: impl Into<Arc<str>>) -> Self {
        let object = object.into();
        if !self.objects.contains(&object) {
            self.objects.push(object);
        }
        self
    }

    /// The domain (Appendix A.1).
    #[must_use]
    pub const fn domain(&self) -> EffectDomain {
        self.domain
    }

    /// What recovery would have to achieve here (Appendix A.2).
    #[must_use]
    pub const fn objective(&self) -> RecoveryObjective {
        self.objective
    }

    /// The objects the mutation lands on, in the order the plan names them.
    #[must_use]
    pub fn objects(&self) -> &[Arc<str>] {
        &self.objects
    }

    /// Whether this domain holds state that outlives the process that made it (Appendix A.5).
    #[must_use]
    pub const fn is_persistent(&self) -> bool {
        self.domain.is_persistent()
    }
}

/// The recovery objective Appendix A.2 assigns a domain by default.
///
/// Appendix A.2 states two of these outright — `PRESERVE_EXACT` for file and configuration
/// persistent state, `RESTORE_SEMANTIC` where identity may legitimately change, "such as service
/// worker PIDs" — and the rest follow from the same reading:
///
/// - Every persistent domain takes `PRESERVE_EXACT`. It is the strict objective, satisfied only
///   by a captured state image (Appendix A.5), and choosing the strict one where the
///   specification is silent is the reading that cannot overstate coverage (§2.10).
/// - Runtime domains take `RESTORE_SEMANTIC`: a restarted service is the same service with
///   different pids, which is the example Appendix A.2 gives.
/// - An external side effect takes `COMPENSATE`. §35.2 says such effects are "usually not
///   recoverable through local snapshots" and §35.3 permits a compensating action, which is
///   exactly what `COMPENSATE` means and no more.
/// - Provider transaction state takes `NO_RECOVERY_REQUIRED`: §27.1 makes the provider's own
///   transaction responsible for it, so nothing outside has to bring it back.
/// - An unknown domain takes `UNKNOWN`, which nothing satisfies (Appendix A.7).
#[must_use]
pub const fn objective_for(domain: EffectDomain) -> RecoveryObjective {
    match domain {
        EffectDomain::FilesystemPersistent
        | EffectDomain::BlockStoragePersistent
        | EffectDomain::ApplicationPersistent
        | EffectDomain::IdentitySecurityState => RecoveryObjective::PreserveExact,
        EffectDomain::ProcessRuntime
        | EffectDomain::KernelRuntime
        | EffectDomain::NetworkRuntime
        | EffectDomain::RemoteSystem => RecoveryObjective::RestoreSemantic,
        EffectDomain::ExternalSideEffect => RecoveryObjective::Compensate,
        EffectDomain::ProviderTransactionState => RecoveryObjective::NoRecoveryRequired,
        EffectDomain::Unknown => RecoveryObjective::Unknown,
    }
}

/// The domains `actions` mutate, in Appendix A.1's own order (Appendix A.1).
///
/// Only the actions that change the target system contribute: Appendix A.1 says "for each MUTATE
/// action", and a verification or a cleanup step changes nothing that has to be recoverable.
///
/// Two cases produce [`EffectDomain::Unknown`] rather than nothing, and both are Appendix A.7's
/// "unknown domain" arriving honestly. A mutating action that declares no effect has not told Ono
/// what it changes, and an opaque action (§6.3) cannot. Appendix A.7 caps a plan holding one at
/// partially protected, and that cap only works if the record exists.
#[must_use]
pub fn mutation_domains(actions: &[PlanAction]) -> Vec<MutationDomain> {
    let mut found: Vec<(EffectDomain, Vec<Arc<str>>)> = Vec::new();
    let mut record = |domain: EffectDomain, object: Option<&str>| {
        let objects = match found.iter_mut().find(|(known, _)| *known == domain) {
            Some((_, objects)) => objects,
            None => {
                found.push((domain, Vec::new()));
                // The entry was just pushed, so the last one is it.
                match found.last_mut() {
                    Some((_, objects)) => objects,
                    None => return,
                }
            }
        };
        if let Some(object) = object {
            let object: Arc<str> = Arc::from(object);
            if !objects.contains(&object) {
                objects.push(object);
            }
        }
    };

    for action in actions {
        if !action.role().mutates_target() {
            continue;
        }
        if action.effects().is_empty() || action.execution().is_opaque() {
            record(
                EffectDomain::Unknown,
                action.target().or(Some(action.summary())),
            );
        }
        for effect in action.effects() {
            record(effect.domain(), effect.object().or_else(|| action.target()));
        }
    }

    // Appendix A.1 lists the canonical domains in a fixed order, and `EffectDomain::ALL` is that
    // list. Answering in it makes two derivations of one plan compare equal, which is what §4.4's
    // digest over a sealed plan needs.
    EffectDomain::ALL
        .iter()
        .filter_map(|domain| {
            found
                .iter()
                .find(|(known, _)| known == domain)
                .map(|(domain, objects)| MutationDomain {
                    domain: *domain,
                    objective: objective_for(*domain),
                    objects: objects.clone(),
                })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    #[test]
    fn should_require_the_strict_objective_for_every_persistent_domain() {
        for domain in EffectDomain::ALL {
            if domain.is_persistent() {
                assert_eq!(
                    objective_for(*domain),
                    RecoveryObjective::PreserveExact,
                    "Appendix A.2 gives persistent state the objective only a state image \
                     satisfies"
                );
            }
        }
    }

    #[test]
    fn should_give_an_external_side_effect_a_compensating_objective_only() {
        assert_eq!(
            objective_for(EffectDomain::ExternalSideEffect),
            RecoveryObjective::Compensate,
            "§35.2: a request already served is not recoverable through a local snapshot"
        );
    }

    #[test]
    fn should_leave_an_unknown_domain_unsatisfiable() {
        assert_eq!(
            objective_for(EffectDomain::Unknown),
            RecoveryObjective::Unknown,
            "Appendix A.7: an unknown domain is not quietly treated as covered"
        );
    }

    #[test]
    fn should_assign_an_objective_to_every_canonical_domain() {
        for domain in EffectDomain::ALL {
            let objective = objective_for(*domain);
            assert!(
                RecoveryObjective::ALL.contains(&objective),
                "Appendix A.2 gives `{}` an objective from its own closed list",
                domain.as_str()
            );
        }
    }
}
