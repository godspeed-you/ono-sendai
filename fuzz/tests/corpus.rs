//! The corpus is a regression suite: every seed and every past finding, replayed on every run.
//!
//! This is what stops a fixed crash from coming back, and what makes a committed artifact worth
//! keeping. It is deliberately not a fuzz run — it makes no mutations and takes no budget — so
//! `cargo test` stays fast and deterministic while still executing every input the project has
//! ever cared about.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use std::collections::BTreeSet;

use ono_fuzz::{Budget, TARGETS};

/// The areas spec §35.6 names, in its own words.
const AREAS: &[&str] = &[
    "parser",
    "serializers",
    "remote protocol",
    "plugin protocol",
    "procfs/netlink decoders",
];

#[test]
fn should_have_one_target_for_every_area_the_specification_names() {
    let covered: BTreeSet<&str> = TARGETS.iter().map(|target| target.area).collect();
    let named: BTreeSet<&str> = AREAS.iter().copied().collect();
    assert_eq!(
        covered, named,
        "spec §35.6 names exactly these areas and each must have a target"
    );
}

#[test]
fn should_carry_a_seed_corpus_for_every_target() {
    for target in TARGETS {
        let seeds = ono_fuzz::load(&ono_fuzz::corpus_dir(target.name));
        assert!(
            !seeds.is_empty(),
            "`{}` has no seed corpus, and a mutator with nothing to mutate reaches nothing \
             (spec §35.6)",
            target.name
        );
    }
}

#[test]
fn should_survive_every_input_the_corpus_and_the_past_findings_hold() {
    for target in TARGETS {
        for input in ono_fuzz::load_for(target.name) {
            let report = ono_fuzz::run(
                target,
                std::slice::from_ref(&input),
                &Budget {
                    iterations: 0,
                    ..Budget::default()
                },
            );
            assert!(
                report.findings.is_empty(),
                "`{}` still fails on a committed input; reproduce it with \
                 `cargo run -p ono-fuzz -- repro {} fuzz/artifacts/{}/{}.bin`: {:?}",
                target.name,
                target.name,
                target.name,
                ono_fuzz::digest(&input),
                report.findings
            );
        }
    }
}

#[test]
fn should_find_a_planted_panic_and_write_it_where_it_can_be_reproduced() {
    // The harness itself needs a test, or a green fuzz step proves only that the harness is
    // silent. This target panics on one input the mutator can reach from the seed.
    fn explodes(data: &[u8]) {
        assert!(!data.starts_with(b"boom"), "the planted panic");
    }
    let target = ono_fuzz::Target {
        name: "planted",
        area: "the harness itself",
        run: explodes,
    };
    let report = ono_fuzz::run(&target, &[b"boom".to_vec()], &Budget::default());
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.fault == ono_fuzz::Fault::Panicked),
        "a panicking target is reported as a finding, not swallowed"
    );
    assert!(
        report.findings.iter().any(|finding| {
            finding.detail.contains("the planted panic") && finding.input.starts_with(b"boom")
        }),
        "the finding carries the message and the exact bytes that caused it, got {:?}",
        report.findings
    );
}

#[test]
fn should_execute_the_same_inputs_when_it_is_run_again_with_the_same_seed() {
    // A target cannot capture, so the record goes through a static the test reads back.
    static RECORD: std::sync::Mutex<Vec<Vec<u8>>> = std::sync::Mutex::new(Vec::new());
    fn remember(data: &[u8]) {
        RECORD.lock().unwrap().push(data.to_vec());
    }
    let target = ono_fuzz::Target {
        name: "recorder",
        area: "the harness itself",
        run: remember,
    };
    let budget = Budget {
        iterations: 64,
        ..Budget::default()
    };
    let corpus = vec![b"get process | where cpu > 20".to_vec(), b"{}".to_vec()];
    let _ = ono_fuzz::run(&target, &corpus, &budget);
    let first = std::mem::take(&mut *RECORD.lock().unwrap());
    let _ = ono_fuzz::run(&target, &corpus, &budget);
    let second = std::mem::take(&mut *RECORD.lock().unwrap());
    assert_eq!(
        first, second,
        "a run is fixed by its seed, so a finding in the gate reproduces on a developer machine"
    );
}
