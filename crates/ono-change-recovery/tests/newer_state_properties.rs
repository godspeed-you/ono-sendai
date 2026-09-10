//! Property tests for newer-state analysis (v0.6 §54.2, §24.2, §24.3, §24.5, §13.6, Appendix C.3,
//! Appendix C.4).
//!
//! §54.2: *recovery plans never omit known newer-state destruction.* `conflict.rs` walks Appendix
//! C.4's example and its neighbours; this holds the rule against generated worlds — restore sets,
//! captured digests, observations that are unchanged, rewritten, removed or unobservable, change
//! instants before and after the apply, every restore method, and snapshots a method would
//! destroy — both in the analysis and in the recovery plan built on it. Each case is fixed by its
//! seed, and a failure names the seed that reproduces it.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::collections::BTreeSet;
use std::sync::Arc;

use jiff::Timestamp;
use ono_change_core::{NewerStateClass, NewerStateImpact, RecoveryGoal, RestoreMethod};
use ono_change_recovery::builder::{RecoveryRequest, plan_recovery};
use ono_change_recovery::conflict::{ConflictRequest, ObjectObservation, ObservedState, analyse};
use ono_change_recovery::gate::{conflicting_objects, destroyed_objects, unestablished_objects};
use ono_testkit::Rng;
use support::{TestProvider, at, class_of, ready_asset, registry};

/// How many generated worlds each property is held against.
const CASES: u64 = 256;

const PROVIDER: &str = "ono.recovery.test";

/// One generated recovery: what the asset holds, what the world holds now, and what the method
/// would do.
struct Scenario {
    objects: Vec<String>,
    restored: Vec<String>,
    captured: Vec<(String, String)>,
    observations: Vec<ObjectObservation>,
    captured_at: Timestamp,
    applied_at: Option<Timestamp>,
    method: RestoreMethod,
    destroyed: Vec<String>,
}

impl Scenario {
    fn generate(seed: u64) -> Self {
        let mut rng = Rng::seeded(seed);
        let count = 1 + rng.below(8);
        let objects: Vec<String> = (0..count)
            .map(|index| format!("/srv/app/object-{index}.conf"))
            .collect();
        let mut restored: Vec<String> = objects.iter().filter(|_| rng.chance(2)).cloned().collect();
        if restored.is_empty() {
            restored.push(objects[0].clone());
        }
        let captured: Vec<(String, String)> = objects
            .iter()
            .enumerate()
            .filter(|_| !rng.chance(5))
            .map(|(index, object)| (object.clone(), format!("sha256:captured-{index}")))
            .collect();
        let instants = [
            None,
            Some(at(9, 30)),
            Some(at(10, 30)),
            Some(at(11, 0)),
            Some(at(12, 15)),
        ];
        let observations = objects
            .iter()
            .enumerate()
            .map(|(index, object)| {
                let changed_at = *rng.pick(&instants).expect("five instants");
                let state = match rng.below(6) {
                    0 | 1 => ObservedState::Present {
                        changed_at,
                        digest: Arc::from(format!("sha256:captured-{index}")),
                    },
                    2 | 3 => ObservedState::Present {
                        changed_at,
                        digest: Arc::from(format!("sha256:newer-{index}")),
                    },
                    4 => ObservedState::Absent { changed_at },
                    _ => ObservedState::unestablished("the link to the host was down"),
                };
                ObjectObservation::new(object.as_str(), state)
            })
            .collect();
        let applied_at = (!rng.chance(4)).then(|| at(11, 0));
        let method = *rng.pick(RestoreMethod::ALL).expect("a closed vocabulary");
        let destroyed = (0..rng.below(4))
            .map(|ordinal| format!("tank/data@later-{ordinal}"))
            .collect();
        Self {
            objects,
            restored,
            captured,
            observations,
            captured_at: at(10, 0),
            applied_at,
            method,
            destroyed,
        }
    }

    fn captured_digest(&self, object: &str) -> Option<&str> {
        self.captured
            .iter()
            .find(|(name, _)| name == object)
            .map(|(_, digest)| digest.as_str())
    }

    fn observed(&self, object: &str) -> ObservedState {
        self.observations
            .iter()
            .find(|observation| observation.object() == object)
            .map_or_else(
                || ObservedState::unestablished("not generated"),
                |observation| observation.state().clone(),
            )
    }

    /// Whether `object` holds content written after the apply that the asset does not hold —
    /// known newer state, established by digest evidence rather than guessed from a timestamp.
    ///
    /// A change at or before the apply instant is the original plan's own work, which recovery
    /// exists to undo (Appendix C.4), so it is not newer state.
    fn has_known_newer_state(&self, object: &str) -> bool {
        let Some(captured) = self.captured_digest(object) else {
            return false;
        };
        let state = self.observed(object);
        let differs = match &state {
            ObservedState::Present { digest, .. } => digest.as_ref() != captured,
            ObservedState::Absent { .. } => true,
            ObservedState::Unestablished { .. } => false,
        };
        let the_plans_own = matches!(
            (self.applied_at, state.changed_at()),
            (Some(applied), Some(changed)) if changed <= applied
        );
        differs && !the_plans_own
    }

    fn is_unobservable(&self, object: &str) -> bool {
        matches!(self.observed(object), ObservedState::Unestablished { .. })
    }

    fn analyse(&self) -> NewerStateImpact {
        let mut request = ConflictRequest::new(self.method, self.captured_at, &self.observations);
        if let Some(applied) = self.applied_at {
            request = request.applied_at(applied);
        }
        for object in &self.restored {
            request = request.restoring(object.as_str());
        }
        for (object, digest) in &self.captured {
            request = request.captured(object.as_str(), digest.as_str());
        }
        for snapshot in &self.destroyed {
            request = request.destroying(snapshot.as_str());
        }
        analyse(&request)
    }

    fn describe(&self) -> String {
        let mut lines = vec![format!(
            "method {}, captured {}, applied {:?}, destroying {:?}",
            self.method, self.captured_at, self.applied_at, self.destroyed
        )];
        for object in &self.objects {
            lines.push(format!(
                "  {object}{}: captured {:?}, now {:?}",
                if self.restored.contains(object) {
                    " (restored)"
                } else {
                    ""
                },
                self.captured_digest(object),
                self.observed(object)
            ));
        }
        lines.join("\n")
    }
}

fn for_every_scenario(property: impl Fn(&Scenario) -> Option<String>) {
    for seed in 0..CASES {
        let scenario = Scenario::generate(seed);
        if let Some(violation) = property(&scenario) {
            panic!("seed {seed}: {violation}\n{}", scenario.describe());
        }
    }
}

// --- The analysis (Appendix C.3) --------------------------------------------------------------

#[test]
fn should_list_every_newer_edit_to_a_restored_object_as_a_loss_or_an_unknown() {
    for_every_scenario(|scenario| {
        let impact = scenario.analyse();
        scenario
            .restored
            .iter()
            .filter(|object| scenario.has_known_newer_state(object))
            .find_map(|object| match class_of(&impact, object) {
                Some(class) if class.is_loss() || class == NewerStateClass::Unknown => None,
                other => Some(format!(
                    "Appendix C.4: {object} was written again after the apply and restoring it \
                     would discard that, and the analysis says {other:?}"
                )),
            })
    });
}

#[test]
fn should_list_every_changed_object_as_discarded_when_the_method_rewinds_the_whole_domain() {
    for_every_scenario(|scenario| {
        if !scenario.method.discards_newer_state() {
            return None;
        }
        let impact = scenario.analyse();
        scenario
            .objects
            .iter()
            .filter(|object| !scenario.restored.contains(object))
            .filter(|object| {
                scenario.captured_digest(object).is_some_and(|captured| {
                    scenario.observed(object).digest() != Some(captured)
                        && !scenario.is_unobservable(object)
                })
            })
            .find_map(|object| {
                let class = class_of(&impact, object);
                (!class.is_some_and(NewerStateClass::is_loss)).then(|| {
                    format!(
                        "Appendix C.3: {} rewinds the whole domain, so the newer content of \
                         {object} goes with it, and the analysis says {class:?}",
                        scenario.method
                    )
                })
            })
    });
}

#[test]
fn should_list_every_object_nobody_could_observe_as_unknown() {
    for_every_scenario(|scenario| {
        let impact = scenario.analyse();
        scenario
            .objects
            .iter()
            .filter(|object| scenario.is_unobservable(object))
            .find_map(|object| {
                let class = class_of(&impact, object);
                (class != Some(NewerStateClass::Unknown)).then(|| {
                    format!(
                        "§56.3: the state of {object} could not be established, and the analysis \
                         says {class:?} rather than unknown"
                    )
                })
            })
    });
}

#[test]
fn should_name_every_snapshot_the_method_would_destroy() {
    for_every_scenario(|scenario| {
        let impact = scenario.analyse();
        let named: BTreeSet<&str> = impact
            .destroyed_assets()
            .iter()
            .map(AsRef::as_ref)
            .collect();
        let expected: BTreeSet<&str> = scenario.destroyed.iter().map(String::as_str).collect();
        if named != expected {
            return Some(format!(
                "§13.6: the method destroys {expected:?} and the analysis names {named:?}"
            ));
        }
        (!expected.is_empty() && !impact.requires_destructive_acceptance()).then(|| {
            "§24.5: destroying newer history does not require explicit acceptance".to_owned()
        })
    });
}

// --- The recovery plan (§24.1, §24.3, §24.5) --------------------------------------------------

/// Plans the recovery of `scenario` through `plan_recovery`, with a provider that restores
/// objects selectively and an asset that holds every object of the scenario.
fn recovery_of(seed: u64, scenario: &Scenario) -> ono_change_core::RecoveryPlan {
    let registry = registry(vec![TestProvider::new(PROVIDER).shared()]);
    let covers: Vec<&str> = scenario.objects.iter().map(String::as_str).collect();
    let assets = vec![ready_asset(
        PROVIDER,
        "tank/data@ono-a82f",
        "tank/data",
        &covers,
        scenario.captured_at,
    )];
    let observe = |object: &str| scenario.observed(object);
    let mut request = RecoveryRequest::new(
        &registry,
        &assets,
        &observe,
        RecoveryGoal::RestoreChangedObjects,
        "session-recover",
        at(13, 0),
    );
    if let Some(applied) = scenario.applied_at {
        request = request.applied_at(applied);
    }
    for object in &scenario.restored {
        request = request.restoring(object.as_str());
    }
    for (object, digest) in &scenario.captured {
        request = request.captured(object.as_str(), digest.as_str());
    }
    for snapshot in &scenario.destroyed {
        request = request.destroying(snapshot.as_str());
    }
    plan_recovery(&request).unwrap_or_else(|error| {
        panic!(
            "seed {seed}: a recovery from a ready asset is planned: {}\n{}",
            error.message(),
            scenario.describe()
        )
    })
}

#[test]
fn should_carry_every_newer_edit_to_a_restored_object_into_the_recovery_plan() {
    for seed in 0..CASES {
        let scenario = Scenario::generate(seed);
        let recovery = recovery_of(seed, &scenario);
        let named: Vec<String> = conflicting_objects(&recovery)
            .into_iter()
            .chain(unestablished_objects(&recovery))
            .collect();
        for object in scenario
            .restored
            .iter()
            .filter(|object| scenario.has_known_newer_state(object))
        {
            assert!(
                named
                    .iter()
                    .any(|line| line.starts_with(&format!("{object} — "))),
                "seed {seed}: §24.3: the recovery plan omits the newer edit to {object}, which \
                 restoring it would discard; it names {named:?}\n{}",
                scenario.describe()
            );
        }
    }
}

#[test]
fn should_carry_every_destroyed_snapshot_into_the_recovery_plan() {
    for seed in 0..CASES {
        let scenario = Scenario::generate(seed);
        let recovery = recovery_of(seed, &scenario);
        let named: BTreeSet<String> = destroyed_objects(&recovery).into_iter().collect();
        let expected: BTreeSet<String> = scenario.destroyed.iter().cloned().collect();
        assert_eq!(
            named,
            expected,
            "seed {seed}: §13.6: the recovery plan must name every snapshot it would destroy\n{}",
            scenario.describe()
        );
        if !expected.is_empty() {
            assert!(
                recovery.needs_destructive_acceptance(),
                "seed {seed}: §24.5: a recovery that destroys newer history runs only after \
                 explicit acceptance"
            );
        }
    }
}
