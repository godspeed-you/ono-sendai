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
use ono_spatial_index::PinRegistry;
use ono_spatial_query::{NeighborhoodRequest, neighborhood_of};

mod common;

use common::NOW;
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

// --- v0.4.1 §34.4: a local question does not build the whole graph (issue #87) ------------------

#[test]
fn should_answer_a_local_neighborhood_question_without_projecting_every_domain() {
    // §34.4: "A local neighborhood query SHOULD NOT require construction of the complete system
    // graph when provider APIs can answer the neighborhood incrementally."
    //
    // The property is asserted as an outcome, not by watching a call path (AGENTS.md §11): the
    // same question, asked of an index holding one unrelated object and of an index holding two
    // thousand, has to give the same answer. A planner that consulted the whole index to answer
    // about one place could not, because the second index holds a great deal more.
    let (small, center) = host_with(2);
    let (large, other) = host_with(2_000);
    assert_eq!(center, other, "the centre is the same object in both");

    let near_small = neighborhood_of(
        &small,
        &center,
        &NeighborhoodRequest::new(),
        &PinRegistry::new(),
        NOW,
    );
    let near_large = neighborhood_of(
        &large,
        &center,
        &NeighborhoodRequest::new(),
        &PinRegistry::new(),
        NOW,
    );

    assert_eq!(
        labels(&near_small),
        labels(&near_large),
        "§34.4: the neighbourhood of one place is its own edges, so two thousand unrelated \
         objects elsewhere on the host must not change it"
    );
    assert_eq!(
        near_small.hidden_count(),
        near_large.hidden_count(),
        "and they must not change what it hid either"
    );
    for label in labels(&near_small) {
        assert_eq!(
            total(&near_small, &label),
            total(&near_large, &label),
            "the `{label}` exit counts the centre's own neighbours, not the host's objects"
        );
    }
}

#[test]
fn should_keep_the_work_of_a_neighborhood_question_within_its_declared_cost_class() {
    // §34.1's estimate is what §33.3 refuses on, so the estimate for a *local* question has to be
    // about the neighbourhood rather than about the system it sits in. Two thousand objects on
    // the host, three of them adjacent: the estimate is three, and the question is answered.
    let (large, center) = host_with(2_000);
    let neighbours: usize = labels(&large_neighbourhood(&large, &center))
        .iter()
        .filter_map(|label| total(&large_neighbourhood(&large, &center), label))
        .sum();

    let estimate = CostEstimate::new(
        neighbours,
        1,
        ono_spatial_core::AcquisitionCost::Moderate,
        1,
    );
    assert!(
        !estimate.exceeds(INTERACTIVE_BUDGET),
        "§34.4: a local neighbourhood question estimated {} units over {neighbours} neighbours, \
         which is the cost of the host rather than of the question",
        estimate.units()
    );

    // And the whole host, asked for as a whole, is what the budget is for.
    let global = CostEstimate::new(
        2_000 * 200,
        4,
        ono_spatial_core::AcquisitionCost::Moderate,
        2,
    );
    assert!(
        global.exceeds(INTERACTIVE_BUDGET),
        "a complete graph of the system is what §34.1 calls obviously explosive work"
    );
}

/// The neighbourhood of `center` in `index`, with the default request.
fn large_neighbourhood(
    index: &ono_spatial_index::SpatialIndex,
    center: &ono_spatial_core::SpatialId,
) -> ono_spatial_core::Neighborhood {
    neighborhood_of(
        index,
        center,
        &NeighborhoodRequest::new(),
        &PinRegistry::new(),
        NOW,
    )
}

/// A host holding one `nginx` process with three sockets, plus `filler` unrelated processes.
fn host_with(filler: usize) -> (ono_spatial_index::SpatialIndex, ono_spatial_core::SpatialId) {
    let mut records = vec![common::process(1842, "nginx", "running")];
    for step in 0..3 {
        records.push(common::with(
            common::socket_with(5_000 + step, Some("listen"), None),
            "process",
            ono_value::Value::Int(1842),
        ));
    }
    for step in 0..filler {
        let pid = 20_000 + i64::try_from(step).expect("a small fixture");
        records.push(common::process(pid, &format!("filler-{step}"), "sleeping"));
    }
    let mut index = common::index();
    let mut bridge = common::bridge();
    let absorbed = bridge.absorb(&mut index, &records, NOW);
    assert!(absorbed.refused().is_empty(), "{:?}", absorbed.refused());
    let center = index
        .by_alias("nginx")
        .first()
        .expect("the process is indexed")
        .object()
        .spatial_id()
        .clone();
    (index, center)
}

/// The exit labels of a neighbourhood, in order.
fn labels(neighborhood: &ono_spatial_core::Neighborhood) -> Vec<String> {
    neighborhood
        .groups()
        .iter()
        .map(|group| group.label().to_owned())
        .collect()
}

/// What one exit says it holds.
fn total(neighborhood: &ono_spatial_core::Neighborhood, label: &str) -> Option<usize> {
    neighborhood
        .groups()
        .iter()
        .find(|group| group.label() == label)
        .and_then(ono_spatial_core::NeighborhoodGroup::total)
}
