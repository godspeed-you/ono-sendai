//! The frozen v0.4.1 baseline of spec §57 H0 (issue #30, ADR-0548).
//!
//! §57's phase H0 asks for a starting point:
//!
//! > freeze a v0.4.1 baseline test/performance snapshot; … record current release artifact hashes
//! > and workflow inputs.
//!
//! It was the last issue of the phase rather than the first, and by the time it was worked the
//! tranche was complete — twelve phases, ninety-nine issues, a green gate. There is no *before*
//! left to freeze, and reconstructing one would be measuring today's binary and calling the
//! figures yesterday's, which v0.4.1 §2.6 forbids in the plainest terms it has: an unknown stays
//! unknown rather than becoming a plausible number.
//!
//! So this is the snapshot of the finished state, and it is deliberately thin. The two things
//! H0 asked to freeze already exist as their own machine-readable files:
//!
//! - `docs/contracts/hardening/performance_baseline.json` — §32.4's regression baseline, six metrics
//!   per benchmark on a named environment (H7, ADR-0489, ADR-0490);
//! - `dist/build-inputs.json` — Appendix H's release input manifest, written by
//!   `cargo xtask build-manifest` (H10, ADR-0451).
//!
//! #30's exit test wants *"a machine-readable baseline file in the repository that H7 and H11
//! both consume rather than re-derive"*, and copying either of those two into a third file is the
//! second copy §52.2 exists to forbid. What this file adds is the binding: it names every figure
//! the regression baseline holds, embeds the manifest as the generator produced it, records the
//! repository counts nothing else keeps as history, and states in words what does not exist yet.

use std::path::Path;

use serde_json::{Value as Json, json};

use crate::scan::Problem;

/// Where the snapshot lives.
pub const PATH: &str = "docs/baselines/v0.4.1.json";

/// What the snapshot says about itself.
const SCHEMA: &str = "ono.baseline.v1";

/// Why the release artifact hashes of §57 H0 are absent rather than zero.
const NO_ARTIFACTS: &str = "No v0.4.1 release has been published: no `v*` tag exists, so no \
                            artifact has been built by the release workflow and there are no \
                            hashes to record. v0.4.1 §2.6 keeps an unknown unknown, so this is \
                            null with a reason rather than an empty list that would read as \
                            `nothing was published`. The first `v*` tag writes `SHA256SUMS`, the \
                            provenance and the signature (§47), and `scripts/verify-release.sh` \
                            checks them; re-running `cargo xtask baseline --write` after that tag \
                            records the digests here.";

/// Assembles the snapshot for the repository at `root`.
///
/// Every value is read from a source rather than typed: the counts from `crate::metrics`, the
/// benchmark list from the regression baseline, the manifest from `crate::provenance` — which is
/// what ADR-0451 asked for when it noted that #30's baseline *"should be a captured manifest
/// rather than a second, hand-written list of the same facts"*.
///
/// # Errors
///
/// Returns the reason the snapshot could not be assembled: a performance registry that will not
/// parse, or a repository the manifest cannot be read from.
pub fn capture(root: &Path) -> Result<String, String> {
    let manifest = crate::provenance::build_inputs(root);
    let commit = manifest
        .get("source")
        .and_then(|source| source.get("commit"))
        .cloned()
        .unwrap_or(Json::Null);

    let text = std::fs::read_to_string(root.join(crate::perf::BASELINE))
        .map_err(|error| format!("cannot read {}: {error}", crate::perf::BASELINE))?;
    let measured = crate::perf::Baseline::parse(&text).map_err(|problems| {
        problems
            .into_iter()
            .map(|problem| format!("{} — {}", problem.location, problem.detail))
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    let raw: Json = serde_json::from_str(&text)
        .map_err(|error| format!("{} is not JSON: {error}", crate::perf::BASELINE))?;

    let environment = crate::perf::reference_environment(root)?;

    Ok(serde_json::to_string_pretty(&json!({
        "schema": SCHEMA,
        "tranche": "v0.4.1",
        "note": "The v0.4.1 tranche as it stands, captured by `cargo xtask baseline --write`. \
                 §57's phase H0 asked for a snapshot of the state before the hardening work; this \
                 is the state after it, because H0's last issue was worked after H1-H12 and \
                 measuring today's tree cannot produce yesterday's figures (§2.6, ADR-0548). It \
                 restates nothing: the performance figures live in \
                 `docs/contracts/hardening/performance_baseline.json` and the build inputs are \
                 captured from `cargo xtask build-manifest`, so the file is a binding rather than \
                 a copy (§52.2).",
        "captured": {
            "commit": commit,
            "state": "the v0.4.1 tranche complete",
        },
        "tests": {
            "source": "cargo xtask metrics",
            "checked_by": "xtask/src/metrics.rs::check_readme",
            "note": "History, not a claim about today. §50.1 makes the README's generated block \
                     the live figure and `spec-check` compares it against the tree on every gate \
                     run, so a count here that had to equal the present would be that same \
                     number typed twice (§52.2).",
            "at_capture": counts(root),
        },
        "performance": {
            "source": crate::perf::BASELINE,
            "written_by": "cargo xtask perf --write-baseline",
            "checked_by": "xtask/src/perf.rs::check_registries",
            "environment": environment.id,
            "load_average": raw.get("load_average").cloned().unwrap_or(Json::Null),
            "note": "§32.4's regression baseline is the machine-readable baseline #30 asked for, \
                     and H7 wrote it. This names every figure it holds, so a benchmark that \
                     disappears from it is visible here, and holds no numbers of its own.",
            "measurements": measured
                .measurements
                .iter()
                .map(|record| json!({
                    "benchmark": record.benchmark,
                    "profile": record.profile,
                    "temperature": record.temperature.as_str(),
                    "commit": record.commit,
                }))
                .collect::<Vec<_>>(),
        },
        "release_inputs": {
            "source": "cargo xtask build-manifest",
            "artifact": "dist/build-inputs.json",
            "note": "Appendix H's manifest, captured from the generator the release workflow \
                     runs rather than transcribed (ADR-0451). A manifest generated outside a \
                     workflow run has no tag and no run identity, and says null for both.",
            "at_capture": manifest,
        },
        "artifacts": {
            "hashes": Json::Null,
            "reason": NO_ARTIFACTS,
        },
    }))
    .map_err(|error| format!("cannot serialise the snapshot: {error}"))?
        + "\n")
}

/// The repository counts of §50, as a document.
fn counts(root: &Path) -> Json {
    let mut map = serde_json::Map::new();
    for line in crate::metrics::measure(root).render().lines() {
        if let Some((key, value)) = line.split_once('=') {
            map.insert(
                key.to_owned(),
                value.parse::<u64>().map_or(Json::Null, Json::from),
            );
        }
    }
    Json::Object(map)
}

/// Writes the snapshot.
///
/// # Errors
///
/// Returns the reason the snapshot could not be assembled or written.
pub fn write(root: &Path) -> Result<String, String> {
    let text = capture(root)?;
    let path = root.join(PATH);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    std::fs::write(&path, text)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(PATH.to_owned())
}

/// Holds the snapshot against the sources it froze (v0.4.1 §52.3, §57 H0).
///
/// A snapshot is evidence only while its references resolve. The checks are cross-references
/// rather than value comparisons, because the figures in it are history: a benchmark it names
/// must still be in the regression baseline with all six of §32.3's metrics, every benchmark the
/// regression baseline holds must be named here, the environment must be the one §37.2 declares,
/// the manifest must have the shape the generator produces, and an absence must carry a reason.
#[must_use]
pub fn check(root: &Path) -> Vec<Problem> {
    let Ok(text) = std::fs::read_to_string(root.join(PATH)) else {
        // A missing snapshot is not an error before the increment that writes it (AGENTS.md §14).
        return Vec::new();
    };
    let problem = |detail: String| Problem::new(PATH, detail);
    let snapshot: Json = match serde_json::from_str(&text) {
        Ok(document) => document,
        Err(error) => return vec![problem(format!("is not JSON: {error}"))],
    };
    let mut problems = Vec::new();

    if snapshot["schema"].as_str() != Some(SCHEMA) {
        problems.push(problem(format!(
            "declares no `schema: {SCHEMA}`, so a reader cannot tell what shape it is"
        )));
    }
    let commit = snapshot["captured"]["commit"].as_str().unwrap_or_default();
    if commit.len() != 40 || !commit.chars().all(|c| c.is_ascii_hexdigit()) {
        problems.push(problem(
            "records no commit it was captured at, so nothing says which tree it describes"
                .to_owned(),
        ));
    }
    if snapshot["note"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        problems.push(problem(
            "carries no `note`; a baseline whose meaning is not written down is read as whatever \
             the reader assumed"
                .to_owned(),
        ));
    }

    problems.extend(check_counts(root, &snapshot));
    problems.extend(check_performance(root, &snapshot));
    problems.extend(check_release_inputs(root, &snapshot));

    match (
        snapshot["artifacts"]["hashes"].as_array(),
        snapshot["artifacts"]["reason"].as_str(),
    ) {
        (None, None | Some("")) => problems.push(problem(
            "records no artifact hashes and gives no `reason`. v0.4.1 §2.6 and spec §35.3 keep an \
             unknown unknown rather than absent: a null nobody explained is a question nobody \
             asked"
                .to_owned(),
        )),
        (Some(hashes), _) => {
            for entry in hashes {
                if entry["file"].as_str().is_none() || entry["sha256"].as_str().is_none() {
                    problems.push(problem(
                        "records an artifact hash without both a `file` and a `sha256`".to_owned(),
                    ));
                }
            }
        }
        (None, Some(_)) => {}
    }
    problems
}

/// The recorded counts name the metrics §50 computes, and no others.
fn check_counts(root: &Path, snapshot: &Json) -> Vec<Problem> {
    let Some(recorded) = snapshot["tests"]["at_capture"].as_object() else {
        return vec![Problem::new(
            PATH,
            "records no repository counts; §57 H0 asks the baseline to freeze the test snapshot"
                .to_owned(),
        )];
    };
    let mut problems = Vec::new();
    let computed: Vec<String> = crate::metrics::measure(root)
        .render()
        .lines()
        .filter_map(|line| line.split_once('=').map(|(key, _)| key.to_owned()))
        .collect();
    for metric in &computed {
        if !recorded.contains_key(metric) {
            problems.push(Problem::new(
                PATH,
                format!("records no `{metric}`, which `cargo xtask metrics` computes"),
            ));
        }
    }
    for metric in recorded.keys() {
        if !computed.contains(metric) {
            problems.push(Problem::new(
                PATH,
                format!(
                    "records `{metric}`, which `cargo xtask metrics` does not compute. A count \
                     nothing produces is a count nobody can check"
                ),
            ));
        }
    }
    problems
}

/// Every benchmark the snapshot names resolves, with all six of §32.3's metrics, and none is left
/// out.
fn check_performance(root: &Path, snapshot: &Json) -> Vec<Problem> {
    let mut problems = Vec::new();
    let Ok(text) = std::fs::read_to_string(root.join(crate::perf::BASELINE)) else {
        return problems;
    };
    let Ok(raw) = serde_json::from_str::<Json>(&text) else {
        return problems;
    };
    let records: Vec<&Json> = raw["measurements"]
        .as_array()
        .map_or_else(Vec::new, |rows| rows.iter().collect());
    let label = |row: &Json| {
        format!(
            "{} at profile {} ({})",
            row["benchmark"].as_str().unwrap_or("?"),
            row["profile"].as_str().unwrap_or("?"),
            row["temperature"].as_str().unwrap_or("?")
        )
    };
    let same = |left: &Json, right: &Json| {
        ["benchmark", "profile", "temperature"]
            .iter()
            .all(|field| left[*field] == right[*field])
    };

    let named = snapshot["performance"]["measurements"]
        .as_array()
        .map_or_else(Vec::new, |rows| rows.iter().collect::<Vec<_>>());
    for row in &named {
        let Some(record) = records.iter().find(|record| same(record, row)) else {
            problems.push(Problem::new(
                PATH,
                format!(
                    "names `{}`, and `{}` holds no such figure",
                    label(row),
                    crate::perf::BASELINE
                ),
            ));
            continue;
        };
        for metric in crate::perf::REQUIRED_METRICS {
            if record.get(metric.field).is_none() {
                problems.push(Problem::new(
                    PATH,
                    format!(
                        "names `{}`, whose record states no `{}` — v0.4.1 §32.3's \"{}\"",
                        label(row),
                        metric.field,
                        metric.spec
                    ),
                ));
            }
        }
    }
    for record in &records {
        if !named.iter().any(|row| same(record, row)) {
            problems.push(Problem::new(
                PATH,
                format!(
                    "says nothing about `{}`, which `{}` measured. A snapshot that names some of \
                     the figures is a snapshot of what somebody remembered",
                    label(record),
                    crate::perf::BASELINE
                ),
            ));
        }
    }

    if let Ok(environment) = crate::perf::reference_environment(root) {
        let named = snapshot["performance"]["environment"]
            .as_str()
            .unwrap_or_default();
        if named != environment.id {
            problems.push(Problem::new(
                PATH,
                format!(
                    "was captured on `{named}`, and \
                     `docs/contracts/hardening/performance_environment.yaml` names `{}` (v0.4.1 §32.4)",
                    environment.id
                ),
            ));
        }
    }
    problems
}

/// The captured manifest has the shape `cargo xtask build-manifest` produces.
fn check_release_inputs(root: &Path, snapshot: &Json) -> Vec<Problem> {
    let generated = crate::provenance::build_inputs(root);
    let Some(expected) = generated.as_object() else {
        return Vec::new();
    };
    let Some(recorded) = snapshot["release_inputs"]["at_capture"].as_object() else {
        return vec![Problem::new(
            PATH,
            "captures no release input manifest; §57 H0 asks the baseline to record the workflow \
             inputs, and Appendix H is the list"
                .to_owned(),
        )];
    };
    let mut problems = Vec::new();
    for field in expected.keys() {
        if !recorded.contains_key(field) {
            problems.push(Problem::new(
                PATH,
                format!(
                    "captured a release input manifest without `{field}`, which \
                     `cargo xtask build-manifest` emits (Appendix H)"
                ),
            ));
        }
    }
    for field in recorded.keys() {
        if !expected.contains_key(field) {
            problems.push(Problem::new(
                PATH,
                format!(
                    "captured `{field}` in the release input manifest, and \
                     `cargo xtask build-manifest` produces no such field. The snapshot is a \
                     capture, not a second list"
                ),
            ));
        }
    }
    problems
}
