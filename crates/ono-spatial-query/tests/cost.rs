//! The spatial query cost model (v0.4.1 §34.1, §34.2, §34.4).
//!
//! §34.1 asks the planner for a coarse estimate before it expands anything expensive, and sets
//! the bar it has to clear:
//!
//! > It need not be mathematically exact. It MUST be conservative enough to avoid obviously
//! > explosive work.
//!
//! §34.2 fixes the vocabulary — `cheap`, `moderate`, `expensive`, `external` — and one property
//! of it: **"The class MUST be machine-readable."** A class that exists only as a Rust variant is
//! not machine-readable, so it is declared in `docs/spec/hardening/cost_classes.yaml` and
//! compared against the implementation in both directions.
//!
//! §34.4 is the rule the estimate exists to serve: *"A local neighborhood query SHOULD NOT
//! require construction of the complete system graph."*

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::collections::BTreeSet;

use ono_spatial_core::{AcquisitionCost, CostClass, spaces};
use ono_spatial_query::cost::{CostEstimate, INTERACTIVE_BUDGET, refusal};

/// §34.2's four class names, typed from the specification.
const ACQUISITION_CLASSES: [&str; 4] = ["cheap", "moderate", "expensive", "external"];

#[test]
fn should_assign_a_declared_cost_class_to_every_canonical_query() {
    // 1. The implementation knows exactly §34.2's four, and no more.
    let known: BTreeSet<&str> = AcquisitionCost::ALL
        .iter()
        .map(|class| class.as_str())
        .collect();
    assert_eq!(
        known,
        ACQUISITION_CLASSES.into_iter().collect::<BTreeSet<_>>(),
        "v0.4.1 §34.2 fixes the vocabulary a cost class is named in"
    );

    // 2. The registry declares the same four, so the class is machine-readable (§34.2, §52.1).
    let declared = declared_classes();
    assert_eq!(
        declared,
        ACQUISITION_CLASSES
            .into_iter()
            .map(str::to_owned)
            .collect::<BTreeSet<String>>(),
        "docs/spec/hardening/cost_classes.yaml declares the classes of v0.4.1 §34.2"
    );

    // 3. Every internal cost class maps onto one of them, so nothing the planner reasons about
    //    is outside the vocabulary a caller can read.
    for class in CostClass::ALL {
        let acquisition = class.acquisition();
        assert!(
            known.contains(acquisition.as_str()),
            "`{}` maps onto `{}`, which is not one of §34.2's four",
            class.as_str(),
            acquisition.as_str()
        );
    }

    // 4. Every canonical relation carries one. §34.2 speaks of "relationship/provider
    //    acquisition", so a relation with no class is a query whose cost nobody declared.
    for spec in ono_spatial_core::relation::RELATIONS {
        let acquisition = spec.cost_class.acquisition();
        assert!(
            known.contains(acquisition.as_str()),
            "the `{}` relation has no §34.2 class",
            spec.id
        );
    }

    // 5. And every space a query can enumerate.
    for space in spaces() {
        let Some(source) = ono_spatial_query::source_of_space(space.id) else {
            continue;
        };
        assert!(
            known.contains(source.cost.acquisition().as_str()),
            "the `{}` space has no §34.2 class for its enumeration",
            space.id
        );
    }
}

#[test]
fn should_refuse_with_the_estimated_cost_when_the_estimate_exceeds_the_interactive_budget() {
    // §34.1: the estimate is coarse and conservative. §33.3: "If the planner predicts cost beyond
    // the supported interactive budget, Ono MUST refuse or switch to a bounded lower-detail
    // strategy rather than silently appear hung."
    let explosive = CostEstimate::new(200_000, 4, AcquisitionCost::Moderate, 2);
    assert!(
        explosive.exceeds(INTERACTIVE_BUDGET),
        "two hundred thousand candidates with a fan-out of four is obviously explosive work, and \
         the estimate put it at {} units against a budget of {INTERACTIVE_BUDGET}",
        explosive.units()
    );

    let error = refusal(&explosive, "map");
    assert_eq!(
        error.code().code(),
        "Ono-Sendai-E1401",
        "a cost refusal has a stable code a script can catch, got {error:?}"
    );
    let message = error.message();
    assert!(
        message.contains(&explosive.units().to_string()),
        "§34.1's refusal names the estimate rather than saying `too expensive`, got {message:?}"
    );
    assert!(
        message.contains("200000") || message.contains("200,000"),
        "and it names what it was going to work on, got {message:?}"
    );

    // A small neighbourhood is not refused, and nothing here is refused for being merely large:
    // the budget is what §34.1 calls "obviously explosive", not a performance target.
    let ordinary = CostEstimate::new(500, 4, AcquisitionCost::Moderate, 2);
    assert!(
        !ordinary.exceeds(INTERACTIVE_BUDGET),
        "an ordinary place is answered rather than refused, and this one estimated {} units",
        ordinary.units()
    );
}

#[test]
fn should_pay_for_an_expensive_relation_when_it_is_explicitly_requested() {
    // §34.3: "If a relationship is described as 'available on request', there MUST actually be a
    // request path." The estimate is what the default path consults, and asking explicitly is
    // what makes it pay: the same query, with the request, is not refused.
    let expensive = CostEstimate::new(200_000, 4, AcquisitionCost::Expensive, 2);
    assert!(
        expensive.exceeds(INTERACTIVE_BUDGET),
        "the default path refuses this one"
    );
    assert!(
        !expensive.requested().exceeds(INTERACTIVE_BUDGET),
        "a caller who asked for the expensive relation by name has said the cost is acceptable, \
         and §34.3 requires that path to exist"
    );

    // An external acquisition costs more per candidate than a cheap one, which is the whole point
    // of §34.2's vocabulary: the class is an input to the estimate, not a label beside it.
    let external = CostEstimate::new(1_000, 1, AcquisitionCost::External, 1);
    let cheap = CostEstimate::new(1_000, 1, AcquisitionCost::Cheap, 1);
    assert!(
        external.units() > cheap.units(),
        "§34.1 lists \"relationship acquisition cost class\" among the estimate's inputs; \
         external estimated {} and cheap {}",
        external.units(),
        cheap.units()
    );
}

/// The class ids `docs/spec/hardening/cost_classes.yaml` declares.
fn declared_classes() -> BTreeSet<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/spec/hardening/cost_classes.yaml");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "v0.4.1 §34.2 requires the cost class to be machine-readable, and {} is where it is \
             declared: {error}",
            path.display()
        )
    });
    let document: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&text).expect("the registry is valid YAML");
    document["classes"]
        .as_sequence()
        .expect("the registry declares `classes`")
        .iter()
        .filter_map(|row| row["id"].as_str().map(str::to_owned))
        .collect()
}
