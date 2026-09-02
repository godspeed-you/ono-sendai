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

use ono_testkit::{PROFILE_M, PROFILE_S, ProcessPopulation, Profile, scratch};

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
    let population = ProcessPopulation::of(PROFILE_S);
    let home = scratch();
    let mine = std::process::id();

    let run = run_bounded(
        &home,
        &format!("get process | where ppid == {mine} | where name == \"sleep\" | count | to json"),
        WATCHDOG,
    );

    assert!(
        run.finished,
        "counting a small population answers. {}",
        run.report()
    );
    let counted = rows_count(&run);
    assert_eq!(
        counted,
        population.len() as u64,
        "the process provider must see every process the Profile {} fixture placed, got {counted} \
         of {}. {}",
        population.profile().name,
        population.len(),
        run.report()
    );
}

// --- the watchdog of v0.4.1 §61.3 ---------------------------------------------------------------

// Issue #22's own command, and the §57 phase H0 failure proof that has to exist before phase H7
// touches it. It produces zero bytes on either stream for the whole of §33.3's budget.
// REASON: red at HEAD; un-ignored by the increment that delivers §35.1/§35.2's bounded initial
// projection and closes #22. Defended by ADR-0431.
#[ignore = "red until issue #22 stops `map --live` producing nothing on a Profile M host (ADR-0431)"]
#[test]
fn should_answer_or_refuse_the_live_map_within_the_interactive_watchdog_on_profile_m() {
    let (population, run) = under_load(PROFILE_M, "map --live --json | take 3 | to json");

    assert!(!run.silent(), "{}", blank(population.profile(), &run));
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
