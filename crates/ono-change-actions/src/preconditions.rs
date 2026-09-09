//! The facts an action freezes so material drift can be detected (spec §7.2, §7.4).
//!
//! §7.2 requires each action to declare preconditions "sufficient to detect material drift", and
//! names the facts by example: a file's hash, a service's generation or state, a package's
//! installed version, a route's identity, a mount that still resolves to the same dataset, and —
//! for every one of them — that the recovery provider the plan depends on is still available.
//! `plannable_operations:` says which of those an operation needs; this module turns each into a
//! [`Precondition`] over one frozen target.
//!
//! Two rules decide the shapes here, and both come from §2.4.
//!
//! - **An absence is a value.** A fact observed to be absent freezes as [`Value::Null`], because
//!   spec §35.3 makes null the absence of a value rather than the absence of an answer. Planning
//!   to create a user that does not exist yet is therefore a plan whose precondition holds, and
//!   one that someone else created in the meantime is material drift.
//! - **An unobservable fact is not a passing one.** [`Observer::fact`] answers `None` when it
//!   could not look, and [`Precondition::check`] turns that into [`ono_change_core::DriftVerdict::Unknown`], which
//!   blocks the apply. A precondition nobody could check has not held.
//!
//! §7.4's tolerances live here too. A service's CPU usage moving does not invalidate a restart
//! plan, and the row says so: the tolerance is contract-declared rather than guessed, which is
//! §7.4's own sentence.

use ono_change_core::{DriftFinding, FrozenTarget, PlanAction, Precondition, PreconditionKind};
use ono_value::Value;

use crate::registry::{Operation, RebootRequirement};

/// What the world says about one fact (§7.2, §7.3).
///
/// Taken as an injected trait rather than read from `/proc` here for the reason §50.1 gives the
/// whole core: a resolver that cannot open a file cannot break §2.1's side-effect-free planning
/// by accident, and a test can state exactly what the machine looked like without one.
pub trait Observer: Send + Sync + std::fmt::Debug {
    /// The value of `field` on `subject`, or `None` where the observation could not be made.
    ///
    /// `Some(Value::Null)` and `None` are different answers. The first is "there is no such
    /// object, and I looked"; the second is "I could not look", which §2.4 forbids reading as
    /// either of the other two.
    fn fact(&self, subject: &str, kind: PreconditionKind, field: &str) -> Option<Value>;

    /// What the provider says about a reboot after `operation` on `subject` (§30.5).
    ///
    /// The default is [`RebootRequirement::Unknown`], which is the honest answer for a provider
    /// that does not track reboot flags: it has not thereby said that no reboot is needed.
    fn reboot(&self, subject: &str, operation: &str) -> RebootRequirement {
        let _ = (subject, operation);
        RebootRequirement::Unknown
    }
}

/// The field a precondition of this kind is about.
///
/// One name per kind, so a frozen fact and the observation that rechecks it ask the same
/// question. §7.2's examples are the source of every one of them.
#[must_use]
pub const fn field_of(kind: PreconditionKind) -> &'static str {
    match kind {
        PreconditionKind::Existence => "identity",
        PreconditionKind::ContentDigest => "sha256",
        PreconditionKind::Generation => "state",
        PreconditionKind::Version => "version",
        PreconditionKind::PersistenceDomain => "persistence-domain",
        PreconditionKind::ProviderAvailable => "available",
        PreconditionKind::Capability => "capability",
        PreconditionKind::Field => "field",
    }
}

/// The sentence a refusal shows when this fact has moved (§7.3).
const fn detail_of(kind: PreconditionKind) -> &'static str {
    match kind {
        PreconditionKind::Existence => {
            "§7.1: a plan stores stable object identities, and this one no longer names the same \
             object"
        }
        PreconditionKind::ContentDigest => {
            "§7.2: the file's hash no longer equals what was resolved, so the plan would write \
             over something it never read"
        }
        PreconditionKind::Generation => {
            "§7.2: the service's state no longer equals what was resolved"
        }
        PreconditionKind::Version => {
            "§7.2: the installed version no longer equals what was resolved"
        }
        PreconditionKind::PersistenceDomain => {
            "§7.2 and Appendix B: the path no longer resolves to the same dataset or subvolume. A \
             target that moved filesystems moved recovery domains, and the protection the plan \
             arranged covers the domain it left"
        }
        PreconditionKind::ProviderAvailable => {
            "§7.2: the recovery provider the plan depends on is no longer available, so the way \
             back the plan showed is not there"
        }
        PreconditionKind::Capability => "§43.2: the capability this action needs is no longer held",
        PreconditionKind::Field => "§7.2: a declared field no longer holds its resolved value",
    }
}

/// The facts one action over one target declares (§7.2, §7.4).
///
/// `provider` is the id of the provider the plan depends on, which is the subject of the
/// availability and capability preconditions: those are facts about the mechanism rather than
/// about the object, and §7.2 lists them beside the object's own.
#[must_use]
pub fn for_target(
    operation: &Operation,
    target: &FrozenTarget,
    provider: &str,
    capability: Option<&str>,
    observer: &dyn Observer,
) -> Vec<Precondition> {
    let mut declared = Vec::with_capacity(operation.preconditions().len());
    for kind in operation.preconditions() {
        declared.push(match kind {
            PreconditionKind::ProviderAvailable => {
                Precondition::new(*kind, provider, field_of(*kind), Value::Bool(true))
                    .explained(detail_of(*kind))
            }
            PreconditionKind::Capability => {
                let held = capability.map_or(Value::Null, Value::string);
                Precondition::new(*kind, provider, field_of(*kind), held)
                    .explained(detail_of(*kind))
            }
            _ => {
                let field = field_of(*kind);
                let observed = observer.fact(target.identity(), *kind, field);
                let unobserved = observed.is_none();
                let precondition = Precondition::new(
                    *kind,
                    target.identity(),
                    field,
                    observed.unwrap_or_default(),
                );
                if unobserved {
                    precondition.explained(format!(
                        "{}. This fact could not be observed when the plan was resolved, so \
                         revalidation cannot say it still holds (§2.4)",
                        detail_of(*kind)
                    ))
                } else {
                    precondition.explained(detail_of(*kind))
                }
            }
        });
    }
    for tolerance in operation.tolerances() {
        let observed = observer
            .fact(
                target.identity(),
                PreconditionKind::Field,
                tolerance.field(),
            )
            .unwrap_or_default();
        declared.push(
            Precondition::new(
                PreconditionKind::Field,
                target.identity(),
                tolerance.field(),
                observed,
            )
            .tolerant()
            .explained(tolerance.doc()),
        );
    }
    declared
}

/// What the mechanism a plan depends on says about itself (§7.2, §43.2).
///
/// Two of §7.2's facts are about the provider rather than about the object — "recovery provider
/// still available", and §43.2's capability — and an [`Observer`] of the world is the wrong thing
/// to ask. The provider is asked instead, and it answers for itself.
pub trait Mechanism {
    /// Whether `provider` can still run here (§7.2, Appendix G.4).
    fn is_available(&self, provider: &str) -> bool;

    /// Whether `provider` still holds `capability` (§43.2).
    fn holds(&self, provider: &str, capability: &str) -> bool;
}

/// Rechecks every precondition of `action` against the world (§7.3).
///
/// Returns one finding per precondition that did not come back [`ono_change_core::DriftVerdict::Unchanged`], so a
/// tolerated move is reported and does not block: §7.4 makes the difference the contract's to
/// declare, and [`ono_change_core::DriftVerdict::blocks_apply`] the caller's to read.
#[must_use]
pub fn revalidate(
    action: &PlanAction,
    observer: &dyn Observer,
    mechanism: &dyn Mechanism,
) -> Vec<DriftFinding> {
    action
        .preconditions()
        .iter()
        .filter_map(|precondition| {
            let subject = precondition.subject();
            let observed = match precondition.kind() {
                PreconditionKind::ProviderAvailable => {
                    Some(Value::Bool(mechanism.is_available(subject)))
                }
                PreconditionKind::Capability => precondition.expected().as_str().ok().map(|name| {
                    if mechanism.holds(subject, name) {
                        Value::string(name)
                    } else {
                        Value::Null
                    }
                }),
                kind => observer.fact(subject, kind, precondition.field()),
            };
            let verdict = precondition.check(observed.as_ref());
            (verdict != ono_change_core::DriftVerdict::Unchanged)
                .then(|| DriftFinding::new(precondition, verdict, observed))
        })
        .collect()
}
