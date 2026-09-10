//! Property tests for the word at the top of the coverage matrix (v0.6 §54.2, §10.2, Appendix A.5,
//! Appendix A.7).
//!
//! Two of §54.2's properties are statements about every matrix rather than about the handful the
//! example suites name: `PROTECTED` implies the required domains have validated recovery behind
//! them, and unknown coverage never upgrades on its own. So they are checked over generated
//! matrices. Each case is fixed by its seed, and a failure names the seed that reproduces it and
//! the rows it was built from.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_change_core::{
    DomainCoverage, DomainProtection, EffectDomain, ProtectionLevel, ProtectionSummary,
    RecoveryAssetId, RecoveryObjective,
};
use ono_testkit::Rng;

/// How many generated matrices each property is held against.
const CASES: u64 = 256;

const PROVIDERS: &[&str] = &["ono.provider.database", "ono.provider.packages"];

/// A coverage matrix of one to six rows.
///
/// A quarter of the matrices are a single provider's transaction. `ProtectionSummary::level`
/// answers that branch before any other (§27.1), and rows drawn independently almost never line
/// up to reach it, so without the bias the properties would never be asked about it.
fn matrix(rng: &mut Rng) -> ProtectionSummary {
    let transactional = rng.chance(4);
    let count = 1 + rng.below(6);
    let rows = (0..count)
        .map(|index| row(rng, index, transactional))
        .collect();
    ProtectionSummary::of(rows)
}

fn row(rng: &mut Rng, index: usize, transactional: bool) -> DomainCoverage {
    let domain = *rng.pick(EffectDomain::ALL).expect("a closed vocabulary");
    let objective = *rng
        .pick(RecoveryObjective::ALL)
        .expect("a closed vocabulary");
    let protection = if transactional {
        DomainProtection::Transactional
    } else {
        *rng.pick(DomainProtection::ALL)
            .expect("a closed vocabulary")
    };
    let mut row = DomainCoverage::new(
        domain,
        objective,
        protection,
        format!("generated row {index}"),
    );
    if transactional {
        row = row.within_transaction(PROVIDERS[0]);
    } else if rng.chance(6) {
        row = row.within_transaction(*rng.pick(PROVIDERS).expect("two providers"));
    }
    for ordinal in 0..rng.below(3) {
        row = row.by_asset(RecoveryAssetId::of(
            "ono.recovery.zfs",
            None,
            &format!("tank/row-{index}"),
            &ordinal.to_string(),
        ));
    }
    if rng.chance(8) {
        row = row.declared_irrelevant();
    }
    row
}

/// The rows, one line each, so a failure shows the counterexample rather than a debug dump.
fn describe(summary: &ProtectionSummary) -> String {
    summary
        .rows()
        .iter()
        .map(|row| {
            format!(
                "  {} / {} / {}{}{}{}",
                row.domain(),
                row.objective(),
                row.protection(),
                row.transaction_scope()
                    .map_or_else(String::new, |scope| format!(" in {scope}")),
                if row.assets().is_empty() {
                    String::new()
                } else {
                    format!(" by {} asset(s)", row.assets().len())
                },
                if row.is_declared_irrelevant() {
                    " (declared irrelevant)"
                } else {
                    ""
                },
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Holds `property` against every generated matrix; `Some` is the violation, in words.
fn for_every_matrix(property: impl Fn(&ProtectionSummary) -> Option<String>) {
    for seed in 0..CASES {
        let summary = matrix(&mut Rng::seeded(seed));
        if let Some(violation) = property(&summary) {
            panic!(
                "seed {seed}: {violation}\nthe matrix reads {}:\n{}",
                summary.level(),
                describe(&summary)
            );
        }
    }
}

fn claims_persistent_coverage(summary: &ProtectionSummary) -> bool {
    summary.level().covers_persistent_state()
}

// --- PROTECTED implies the required domains are covered (§54.2, §10.2, Appendix A.5) ----------

#[test]
fn should_call_a_plan_protected_only_when_every_required_domain_is_satisfied() {
    for_every_matrix(|summary| {
        if summary.level() != ProtectionLevel::Protected {
            return None;
        }
        let unsatisfied: Vec<String> = summary
            .required_rows()
            .filter(|row| !row.is_satisfied())
            .map(|row| row.domain().to_string())
            .collect();
        (!unsatisfied.is_empty() || !summary.shortfall().is_empty()).then(|| {
            format!(
                "§10.2: PROTECTED was reported while required domains {unsatisfied:?} are not \
                 satisfied"
            )
        })
    });
}

#[test]
fn should_call_a_plan_protected_only_when_every_required_persistent_domain_rests_on_a_state_image()
{
    for_every_matrix(|summary| {
        if summary.level() != ProtectionLevel::Protected {
            return None;
        }
        let persistent: Vec<&DomainCoverage> = summary
            .required_rows()
            .filter(|row| row.domain().is_persistent())
            .collect();
        if persistent.is_empty() {
            return Some(
                "§10.2: PROTECTED was reported for a plan that requires no persistent domain"
                    .to_owned(),
            );
        }
        persistent
            .iter()
            .find(|row| !row.protection().is_state_image())
            .map(|row| {
                format!(
                    "§10.2: PROTECTED was reported while the required persistent domain {} is \
                     only {}",
                    row.domain(),
                    row.protection()
                )
            })
    });
}

// REASON: product defect. `ProtectionSummary::level` composes the word from each row's declared
// `DomainProtection` and never looks at whether the row names the asset (or the provider
// transaction) that is supposed to stand behind it, so a matrix can read PROTECTED with nothing
// behind it. Minimal counterexample (seed 195): a single row
// `block-storage-persistent / preserve-exact / protected` with no asset and no transaction scope
// composes to PROTECTED. `coverage::analyse` always attaches the proposed asset, so the
// planner's own rows are backed; the gap is that the type §10.2 calls "a property of the type"
// accepts an unbacked claim from any other producer (a decoded record, a contributed provider).
#[test]
fn should_call_a_plan_protected_only_when_every_required_persistent_domain_names_what_covers_it() {
    for_every_matrix(|summary| {
        if summary.level() != ProtectionLevel::Protected {
            return None;
        }
        summary
            .required_rows()
            .filter(|row| row.domain().is_persistent())
            .find(|row| row.assets().is_empty() && row.transaction_scope().is_none())
            .map(|row| {
                format!(
                    "§54.2: PROTECTED was reported while the required persistent domain {} names \
                     no validated recovery asset and no provider transaction behind its {} claim",
                    row.domain(),
                    row.protection()
                )
            })
    });
}

// --- Unknown coverage never upgrades automatically (§54.2, §2.4, Appendix A.7) -----------------

#[test]
fn should_never_claim_persistent_coverage_while_a_required_domain_has_unknown_protection() {
    for_every_matrix(|summary| {
        let unknown = summary
            .required_rows()
            .find(|row| row.protection() == DomainProtection::Unknown)?;
        claims_persistent_coverage(summary).then(|| {
            format!(
                "§2.4: the required domain {} has unknown protection and the plan was upgraded \
                 to {}",
                unknown.domain(),
                summary.level()
            )
        })
    });
}

#[test]
fn should_never_claim_persistent_coverage_while_a_required_domain_has_an_unknown_objective() {
    for_every_matrix(|summary| {
        let unknown = summary
            .required_rows()
            .find(|row| row.objective() == RecoveryObjective::Unknown)?;
        claims_persistent_coverage(summary).then(|| {
            format!(
                "§2.4: the required domain {} has an unknown recovery objective and the plan was \
                 upgraded to {}",
                unknown.domain(),
                summary.level()
            )
        })
    });
}

// REASON: product defect. `ProtectionSummary::level` returns TRANSACTIONAL before it applies
// Appendix A.7's cap, so a required domain of unknown kind inside one provider's transaction is
// upgraded past PARTIALLY_PROTECTED. Minimal counterexample (seed 20): a single row
// `unknown / preserve-exact / transactional in ono.provider.database` composes to TRANSACTIONAL.
// A.7: "plan-level protection cannot be stronger than PARTIALLY_PROTECTED unless policy explicitly
// declares that domain irrelevant". The cap belongs before the §27.1 transactional branch.
#[test]
fn should_never_claim_persistent_coverage_while_a_required_domain_is_of_an_unknown_kind() {
    for_every_matrix(|summary| {
        summary
            .required_rows()
            .find(|row| row.domain() == EffectDomain::Unknown)?;
        claims_persistent_coverage(summary).then(|| {
            format!(
                "Appendix A.7: a required domain of unknown kind caps the plan at \
                 partially-protected unless policy declares it irrelevant, and the plan was \
                 upgraded to {}",
                summary.level()
            )
        })
    });
}
