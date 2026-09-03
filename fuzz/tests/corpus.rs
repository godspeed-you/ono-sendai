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

/// The entry points v0.4.1 §41.2 requires the coverage-guided tier to cover, in its own words,
/// and the target that covers each.
///
/// §41.2's list is §35.6's with two additions: the handshake decoder read apart from the framing
/// around it, and the adapter decoders, which are on no remote path at all. Attacker classes 7
/// and 8 of §5.2 are exactly those two.
const GUIDED: &[(&str, &str)] = &[
    ("parser/lexer entry points", "parser"),
    ("Value deserialization", "serializers"),
    ("remote frame decoder", "remote-protocol"),
    ("remote handshake decoder", "remote-handshake"),
    ("KUANG frame decoder", "plugin-protocol"),
    (
        "procfs/netlink or equivalent structured system-data decoders",
        "system-decoders",
    ),
    (
        "adapter machine-readable decoders with attacker-controlled bytes",
        "adapter-decoders",
    ),
];

#[test]
fn should_have_one_target_for_every_area_the_specification_names() {
    let covered: BTreeSet<&str> = TARGETS.iter().map(|target| target.area).collect();
    for area in AREAS {
        assert!(
            covered.contains(area),
            "spec §35.6 names `{area}` and no target covers it"
        );
    }
}

#[test]
fn should_have_a_target_for_every_entry_point_the_coverage_guided_tier_must_cover() {
    // v0.4.1 §41.2 states the minimum the scheduled tier covers. A list in a workflow file could
    // drift from the targets; this is the list read against the targets themselves.
    for (entry, target) in GUIDED {
        assert!(
            ono_fuzz::target(target).is_some(),
            "v0.4.1 §41.2 requires `{entry}` and there is no `{target}` target for it"
        );
    }
    let named: BTreeSet<&str> = GUIDED.iter().map(|(_, target)| *target).collect();
    let all: BTreeSet<&str> = TARGETS.iter().map(|target| target.name).collect();
    assert_eq!(
        named, all,
        "every target is one §41.2 names, and every entry point §41.2 names has one: a target \
         the scheduled tier does not run is a target that runs for four hundred iterations a \
         gate run and never again"
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

// --- v0.4.1 §41.4 and §41.5: what a campaign keeps, and what a hang becomes --------------------

#[test]
fn should_reload_the_persisted_corpus_for_every_target() {
    // §41.4: "Interesting corpus inputs and every minimized crash reproducer MUST be committed or
    // stored as durable CI artifacts and promoted into regression tests when they represent a
    // fixed bug." Both halves are here rather than in a policy document: the seeds and the past
    // findings are on disk, `load_for` reloads them, and
    // `should_survive_every_input_the_corpus_and_the_past_findings_hold` replays every one of
    // them on every gate run — which is what "promoted into a regression test" means when the
    // promotion is not a thing somebody has to remember to do.
    for target in TARGETS {
        let seeds = ono_fuzz::load(&ono_fuzz::corpus_dir(target.name));
        let all = ono_fuzz::load_for(target.name);
        assert!(
            !seeds.is_empty(),
            "`{}` has no committed corpus to reload",
            target.name
        );
        assert!(
            all.len() >= seeds.len(),
            "`{}` reloads its seeds and every artifact beside them, got {} of {}",
            target.name,
            all.len(),
            seeds.len()
        );
        for finding in ono_fuzz::load(&ono_fuzz::artifacts_dir(target.name)) {
            assert!(
                all.contains(&finding),
                "`{}` has a committed crash reproducer that the replay does not reload, so a \
                 fixed bug could come back unnoticed (§41.4)",
                target.name
            );
        }
    }

    // And the coverage-guided tier keeps its own, which is the durable-artifact half: a corpus
    // libFuzzer grew is too large to commit and too valuable to discard, so the scheduled
    // workflow restores it before the run and the cache keeps it after.
    let workflow = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows/fuzz.yml"),
    )
    .expect(".github/workflows/fuzz.yml is readable");
    assert!(
        workflow.contains("restore-keys:") && workflow.contains("fuzz-corpus-"),
        "§41.4: the coverage-guided corpus survives the runner it was grown on"
    );
    assert!(
        workflow.contains("upload-artifact"),
        "§41.4: every crash reproducer leaves the runner, or it was found and lost"
    );
}

#[test]
fn should_record_an_input_that_exceeds_its_timeout_as_a_hang() {
    // §41.5: "Coverage-guided fuzz targets MUST enforce per-input timeouts where supported so
    // pathological hangs become findings, not merely long CI jobs." A hang cannot be observed
    // from inside the thread that hangs (ADR-0313), so what is enforced is the shape of one: an
    // input that returns, having taken longer than the budget allows, is a finding with its bytes
    // attached — and the bytes are what makes it reproducible tomorrow.
    fn dawdles(data: &[u8]) {
        if data.starts_with(b"slow") {
            std::thread::sleep(std::time::Duration::from_millis(120));
        }
    }
    let target = ono_fuzz::Target {
        name: "dawdling",
        area: "the harness itself",
        run: dawdles,
    };
    let report = ono_fuzz::run(
        &target,
        &[b"slow".to_vec()],
        &Budget {
            iterations: 0,
            per_input: std::time::Duration::from_millis(10),
            ..Budget::default()
        },
    );
    let finding = report
        .findings
        .iter()
        .find(|finding| finding.fault == ono_fuzz::Fault::TooSlow)
        .expect("an input over its per-input budget is a finding, not a slow run nobody counts");
    assert_eq!(
        finding.input, b"slow",
        "the finding carries the exact bytes, or it cannot be reproduced"
    );
    assert!(
        finding.detail.contains("10ms"),
        "the finding says what budget it broke, got {:?}",
        finding.detail
    );

    // The same input inside a budget it fits is not a finding: the ceiling is a ceiling and not a
    // dislike of slow inputs.
    let patient = ono_fuzz::run(
        &target,
        &[b"slow".to_vec()],
        &Budget {
            iterations: 0,
            per_input: std::time::Duration::from_secs(5),
            ..Budget::default()
        },
    );
    assert!(patient.findings.is_empty(), "got {:?}", patient.findings);

    // Both tiers enforce it. The gate passes `--per-input-ms`; the scheduled tier passes
    // libFuzzer's own `-timeout`, which kills the process rather than measuring afterwards and is
    // the stronger of the two.
    let workflow = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows/fuzz.yml"),
    )
    .expect(".github/workflows/fuzz.yml is readable");
    assert!(
        workflow.contains("-timeout=") && workflow.contains("-rss_limit_mb="),
        "§41.5: the coverage-guided tier enforces a per-input timeout, and a memory ceiling \
         beside it"
    );
}
