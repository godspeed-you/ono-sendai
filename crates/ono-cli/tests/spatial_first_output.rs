//! Time to first result for interactive spatial work (v0.4.1 §0.5.7, §32, §33, §35).
//!
//! v0.4.1 §0.5.7 records the defect this suite exists for: *"High-cardinality spatial operations
//! have demonstrated time-to-first-result failures that are not visible in small fixtures."*
//! §32.1 draws the conclusion — *"v0.4.1 MUST stop treating one small fixture passing a latency
//! budget as sufficient proof that a spatial operation is performant"* — and §33.3 states the
//! floor underneath every latency target:
//!
//! > A supported interactive operation MUST NOT spend 30 seconds producing neither output nor
//! > progress on the reference Profile M/L fixtures.
//!
//! §61.3 makes that a watchdog: *"A watchdog acceptance test MUST fail any interactive spatial
//! command that produces neither first result nor progress/refusal within the declared hard
//! interactive budget."* This suite is that watchdog, and the budget it declares is §33.3's own
//! thirty seconds — a number the specification fixes rather than one this file invented, sixty
//! times above §33.2's 500 ms Profile M target, so a loaded machine cannot turn it into a verdict
//! about performance (issue #21, ADR-0252).
//!
//! **The fixture.** `ono_testkit::ProcessPopulation` puts the host at a §32.2 reference profile
//! by creating real processes the production provider reads out of `/proc` like any other —
//! §32.2 requires exactly that: *"provider/planner code exercised by the benchmark MUST match
//! production logic."* Issue #22 has stood open for want of exactly this: its own report ends at
//! "the difference is the machine, not the build" — 920 processes on a desktop against four in
//! the demo container — and a defect whose reproduction is the reporter's hardware is a defect
//! nobody else can work on. The population is what replaces that sentence, and it is the fixture
//! phase H7 needs for issues #82 and #85.
//!
//! **What is red.** The watchdog test is the §57 phase H0 failure proof required before any phase
//! H7 fix lands (issue #31), and its command is issue #22's own. It is `#[ignore]`d until the
//! increment that stabilises `map --live` (§35.1) turns it green; ADR-0431 records why it lands
//! ignored, and what it measured about the neighbouring instance in issue #20 that is *not*
//! encoded here — the full-screen map of `COMPUTE` answers this fixture in about the width of
//! §33.3's budget, so a watchdog over it would be a coin toss rather than a proof.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod support;

use std::time::Duration;

use ono_testkit::{
    PROFILE_L, PROFILE_M, PROFILE_S, ProcessPopulation, Profile, SocketPopulation, scratch,
};

use support::{Bounded, run_bounded};

/// The hard interactive budget of v0.4.1 §33.3, which §61.3 makes a watchdog.
///
/// This is not a performance assertion. §33.2 asks for a first frame in 500 ms on Profile M and
/// progress within 1.5 s on Profile L; thirty seconds is the point at which the shell is simply
/// not answering, and a machine would have to be twenty times slower than the reference
/// environment before a working implementation came near it.
const WATCHDOG: Duration = Duration::from_secs(30);

/// Runs `script` against a host held at `profile`, and reports what it said before the watchdog.
fn under_load(profile: Profile, script: &str) -> (ProcessPopulation, Bounded) {
    let population = ProcessPopulation::of(profile);
    let home = scratch();
    let outcome = run_bounded(&home, script, WATCHDOG);
    (population, outcome)
}

/// The failure message §33.3 deserves: what was asked, of how big a system, and what came back.
fn blank(profile: Profile, run: &Bounded) -> String {
    format!(
        "v0.4.1 §33.3: a supported interactive operation must not spend {:?} producing neither \
         output nor progress on the Profile {} fixture ({} processes placed). §35.1 makes this \
         class of behaviour — `map --live` producing no bytes for tens of seconds on a realistic \
         host — a release blocker. {}",
        WATCHDOG,
        profile.name,
        profile.processes,
        run.report()
    )
}

// --- the fixture itself -----------------------------------------------------------------------

#[test]
fn should_show_a_placed_population_to_the_process_provider_when_a_profile_fixture_is_built() {
    // v0.4.1 §32.2 allows a fixture to synthesize entities, but not to bypass the code under
    // measurement: "provider/planner code exercised by the benchmark MUST match production
    // logic". The population is therefore real processes, and the way to prove it is production
    // logic is to ask the shell's own provider to count them.
    let home = scratch();
    let mine = std::process::id();
    // Counted as a difference rather than as a total, for the reason the socket fixture is: the
    // watchdog beside this test places a Profile M population from the same parent process, and
    // `cargo test` runs the two on their own threads. A count that assumed it was the only
    // fixture on the machine would be a test that fails when a neighbour is doing its job.
    let counting =
        format!("get process | where ppid == {mine} | where name == \"sleep\" | count | to json");

    let before = rows_count(&run_bounded(&home, &counting, WATCHDOG));
    let population = ProcessPopulation::of(PROFILE_S);
    let run = run_bounded(&home, &counting, WATCHDOG);

    assert!(
        run.finished,
        "counting a small population answers. {}",
        run.report()
    );
    let after = rows_count(&run);
    assert!(
        after >= before + population.len() as u64,
        "the process provider must see every process the Profile {} fixture placed: {before} \
         before, {} placed, {after} after. {}",
        population.profile().name,
        population.len(),
        run.report()
    );
}

#[test]
fn should_show_a_placed_socket_population_to_the_socket_provider_when_a_profile_fixture_is_built() {
    // §32.2's socket-specific profiles (Appendix F.2) get the same treatment as its process
    // ones: the fixture opens real listening sockets and the shell's own provider counts them.
    // A unix socket carries no path in `ono.socket/1`, so the fixture is proven by the
    // difference it makes rather than by naming its members — which is the honest measurement
    // anyway, because the host is running its own listeners throughout.
    let home = scratch();
    let listening = "get socket | where family == \"unix\" | where state == \"listen\" \
                     | count | to json";

    let before = rows_count(&run_bounded(&home, listening, WATCHDOG));
    let population = SocketPopulation::of(PROFILE_S);
    let run = run_bounded(&home, listening, WATCHDOG);

    assert!(
        run.finished,
        "counting the listening sockets of a small population answers. {}",
        run.report()
    );
    let after = rows_count(&run);
    assert!(
        after >= before + population.len() as u64,
        "the socket provider must see every socket the Profile {} fixture opened: {before} \
         listening unix sockets before, {} placed, {after} after. {}",
        population.profile().name,
        population.len(),
        run.report()
    );
}

// --- the watchdog of v0.4.1 §61.3 ---------------------------------------------------------------

// Issue #22's own command, and the §57 phase H0 failure proof that had to exist before phase H7
// touched it. It produced zero bytes on either stream for the whole of §33.3's budget; it now
// answers, because a live map that has watched a still system for ten seconds says so instead of
// looking hung (ADR-0492). What satisfies this is deliberately wide, because §33.3 is: output,
// progress metadata or a deterministic refusal.
#[test]
fn should_answer_or_refuse_the_live_map_within_the_interactive_watchdog_on_profile_m() {
    let (population, run) = under_load(PROFILE_M, "map --live --json | take 3 | to json");

    assert!(!run.silent(), "{}", blank(population.profile(), &run));
}

// --- the reference targets of v0.4.1 §33.2 ------------------------------------------------------

/// §33.2's table, typed from the specification.
///
/// The same four rows are declared as data in `xtask::perf::TARGETS`, which is what
/// `cargo xtask perf` measures them by. They are written out again here for the reason
/// `crates/ono-spatial-query/tests/profiles.rs` writes out Appendix F: a check that read the
/// declaration to check the declaration would agree with itself.
///
/// Each row names the benchmark that answers it, at which profile and at which of §37.3's three
/// temperatures. "Basic **cached**" is §37.3's cache hit — the same query answered again — and
/// the rest are cold: an interactive operation is budgeted from where the user pressed return.
const REFERENCE_TARGETS: [(&str, &str, &str, &str, f64); 4] = [
    // §33.2 wording, benchmark, profile, temperature, p95 budget in milliseconds
    (
        "basic cached look/near first result",
        "spatial.look",
        "S",
        "cache_hit",
        50.0,
    ),
    (
        "spatial query Profile M first result",
        "spatial.query",
        "M",
        "cold",
        150.0,
    ),
    (
        "map live Profile M initial visible frame",
        "spatial.map_first_frame",
        "M",
        "cold",
        500.0,
    ),
    (
        "map live Profile L initial progress/summary",
        "spatial.map_first_frame",
        "L",
        "cold",
        1_500.0,
    ),
];

// §33.2's four rows are measured with `cargo xtask perf` at twenty iterations and recorded in the
// baseline. Three of them are outside their budget on the reference environment (ADR-0491), and
// the remaining cause is one thing rather than two: an *acquisition* that costs more than the
// budget allows, per object, from outside this shell.
//
// COMPUTE pays about 400 ms on every orientation for the systemd service enumeration — 569 units
// at three D-Bus round trips each, already made concurrent, and §34.2's `external` class by
// construction. Profile L pays for absorbing a hundred thousand socket records before anything is
// drawn. Neither is the cardinality the profile is named for, and neither is a planner defect:
// both are the shell asking a provider for every object when it needs a bounded view of them,
// which is §34.4's sentence and what its remaining half owes.
// REASON: red at HEAD; un-ignored by the increment that lets an orientation query take a bounded
// answer from a provider instead of the complete one (§34.4). Defended by ADR-0491 and ADR-0496.
#[ignore = "red until an orientation query can take a bounded answer from a provider (§34.4; ADR-0496)"]
#[test]
fn should_hold_every_time_to_first_result_target_of_the_reference_targets_table() {
    let baseline = recorded_baseline();
    let mut missed = Vec::new();
    for (spec, benchmark, profile, temperature, budget_ms) in REFERENCE_TARGETS {
        let record = baseline
            .iter()
            .find(|record| {
                text_of(record, "benchmark") == benchmark
                    && text_of(record, "profile") == profile
                    && text_of(record, "temperature") == temperature
            })
            .unwrap_or_else(|| {
                panic!(
                    "v0.4.1 §33.2's \"{spec}\" is answered by `{benchmark}` at Profile {profile} \
                     ({temperature}), and the baseline holds no such record. `cargo xtask perf` \
                     is what measures it"
                )
            });
        let p95 = number_of(record, "p95_ms");
        if p95 > budget_ms {
            missed.push(format!(
                "  {spec}: {p95:.0} ms against {budget_ms:.0} ms ({:.1}x)",
                p95 / budget_ms
            ));
        }
    }
    assert!(
        missed.is_empty(),
        "v0.4.1 §33.2's reference targets are not met on the reference environment:\n{}",
        missed.join("\n")
    );
}

// §33.3 at the other reference profile, and the rule this file's Profile M watchdog states, met
// at the cardinality §32.2 names beside it. Against a hundred thousand listening sockets the live
// map produced nothing for 79 s in a debug build and 23 s in a release one; it now answers with
// §34.1's cost refusal, naming the estimate (ADR-0494, ADR-0496).
//
// The margin is 1.5x rather than the twenty ADR-0431 had at Profile M: the refusal arrives after
// the observation, and observing a hundred thousand sockets is 20 s in a debug build. What is
// left is the observation itself, which §34.4 is about and which this branch reports as owed.
#[test]
fn should_answer_or_refuse_within_the_interactive_budget_on_the_profile_l_fixture() {
    // Profile L's ten thousand processes belong to the container; its hundred thousand listening
    // sockets bind in two seconds and are built here (ADR-0488).
    let sockets = SocketPopulation::of(PROFILE_L);
    let home = scratch();

    let run = run_bounded(
        &home,
        "enter network; map --live --json | take 1 | to json",
        WATCHDOG,
    );

    assert!(
        !run.silent(),
        "{}",
        format!(
            "v0.4.1 §33.3 at Profile L: {} listening sockets placed, and the live map produced \
             neither output nor progress inside {:?}. §33.2 allows the answer to be progress \
             metadata or a deterministic cost message rather than the picture itself. {}",
            sockets.len(),
            WATCHDOG,
            run.report()
        )
    );
}

/// The measurements `cargo xtask perf` recorded on the reference environment (§32.4, §37.2).
fn recorded_baseline() -> Vec<serde_yaml_ng::Value> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/spec/hardening/performance_baseline.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "the regression baseline of v0.4.1 §32.4 is at {}: {error}",
            path.display()
        )
    });
    let document: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(&text).expect("the baseline is a JSON document");
    document["measurements"]
        .as_sequence()
        .expect("the baseline holds a sequence of measurements")
        .clone()
}

/// A string field of a measurement record.
fn text_of(record: &serde_yaml_ng::Value, field: &str) -> String {
    record[field].as_str().unwrap_or_default().to_owned()
}

/// A numeric field of a measurement record.
fn number_of(record: &serde_yaml_ng::Value, field: &str) -> f64 {
    record[field]
        .as_f64()
        .unwrap_or_else(|| panic!("a measurement record carries a numeric `{field}`"))
}

/// The one number a `count | to json` run printed.
fn rows_count(run: &Bounded) -> u64 {
    let document: serde_yaml_ng::Value =
        serde_yaml_ng::from_str(run.stdout.trim()).unwrap_or_else(|error| {
            panic!("`count` emits a JSON document ({error}). {}", run.report())
        });
    document
        .as_sequence()
        .and_then(|values| values.first())
        .and_then(serde_yaml_ng::Value::as_u64)
        .unwrap_or_else(|| panic!("`count` emits one number. {}", run.report()))
}
