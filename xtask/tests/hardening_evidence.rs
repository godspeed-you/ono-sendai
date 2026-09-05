//! The release evidence of `docs/ACCEPTANCE.md` §4.8: the checklist of the v0.4.1 tranche is held
//! against the tree, so it cannot rot.
//!
//! `xtask/tests/spatial_evidence.rs` does this for §4.7 and stops at the `### 4.8` heading, so
//! the two checklists are read apart and neither harvester can silently answer for the other's
//! boxes (`docs/ACCEPTANCE.md` §4.8.1). This file is the v0.4.1 half.
//!
//! Everything here is a statement about *evidence* rather than about the shell:
//!
//! * every test §4.8 names as a proof exists, runs where the gate runs it, and is not
//!   `#[ignore]`d — so a box whose proof was renamed away fails here rather than staying ticked;
//! * every acceptance case §4.8 claims in backticks is a file the referee collects, and every one
//!   it records in prose is a name no file answers to yet (ADR-0401);
//! * no P0 or P1 box of the tranche is left open, which is §66.9's binding release criterion;
//! * every box that *is* left open names a dated ADR that predates the release-candidate freeze,
//!   which is the only exclusion §66.9 allows;
//! * every bullet of §66.1-§66.8 has a box in §4.8 that names its proof, so a criterion cannot
//!   lose its evidence by having its box rewritten away.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

mod support;
use support::{assert_proofs_exist, read, repo};

/// The fourteen families of v0.4.1 §40.3, in the order the specification lists them, typed out
/// from the specification rather than read from the checklist.
///
/// Reading them from the document the test is checking would make the test agree with whatever
/// the document said. Each entry is the phrase §4.8.13's box opens with.
const FAMILIES: [&str; 14] = [
    "Direct mutual TLS authentication",
    "Unknown client refusal",
    "Authorization-constrained capability negotiation",
    "Unauthorized action refusal",
    "Authorized exact action success",
    "Changed client key refusal",
    "Malformed authorization store fails closed at startup",
    "KUANG mandatory confinement setup failure",
    "`each` streams an unbounded source",
    "Materialization item and byte limits refuse",
    "Result-history truncation is visible",
    "Profile M spatial first result",
    "Live map cancellation under load",
    "Package signature, checksum and provenance",
];

/// The text of one `####` subsubsection of `docs/ACCEPTANCE.md`.
fn subsection(heading: &str) -> String {
    let acceptance = read("docs/ACCEPTANCE.md");
    let start = acceptance
        .find(heading)
        .unwrap_or_else(|| panic!("docs/ACCEPTANCE.md carries `{heading}`"));
    let end = acceptance[start..]
        .find("\n#### ")
        .map_or(acceptance.len(), |offset| start + offset);
    acceptance[start..end].to_owned()
}

/// One checklist box: its whole text, and whether it is ticked.
fn boxes(passage: &str) -> Vec<(bool, String)> {
    let mut found = Vec::new();
    for line in passage.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("- [x] ") {
            found.push((true, rest.to_owned()));
        } else if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            found.push((false, rest.to_owned()));
        } else if line.starts_with(' ')
            && !trimmed.is_empty()
            && let Some((_, text)) = found.last_mut()
        {
            // A box's continuation lines are indented under it. A heading or a paragraph at
            // column zero belongs to the subsection, not to the box above it, and reading it as
            // one made a box look like it named proofs the prose beside it happened to mention.
            text.push(' ');
            text.push_str(trimmed);
        }
    }
    found
}

/// Every `NNN-kebab-name` a passage carries, whether or not it is inside backticks.
///
/// ADR-0401 reads a backticked name as a claim and a plain one as a name recorded absent, so both
/// spellings are collected here and the difference is what the test asserts about.
fn case_names(text: &str) -> Vec<(String, bool)> {
    let mut found = Vec::new();
    let bytes: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index + 4 <= bytes.len() {
        if bytes[index..index + 3].iter().all(char::is_ascii_digit) && bytes[index + 3] == '-' {
            let start = index;
            let mut end = index + 4;
            while end < bytes.len()
                && (bytes[end].is_ascii_alphanumeric() || bytes[end] == '-' || bytes[end] == '_')
            {
                end += 1;
            }
            let name: String = bytes[start..end].iter().collect();
            if name.contains(|c: char| c.is_ascii_alphabetic()) {
                let quoted =
                    start > 0 && bytes[start - 1] == '`' && end < bytes.len() && bytes[end] == '`';
                found.push((name, quoted));
            }
            index = end;
            continue;
        }
        index += 1;
    }
    found
}

#[test]
fn should_find_a_case_for_every_one_of_the_fourteen_acceptance_families() {
    // v0.4.1 §40.3 names fourteen families and the exit test of #91 is that each appears as a
    // numbered case. §4.8.13 holds one box per family, in §40.3's order, and this resolves every
    // pointer in it: a family whose case exists names it in backticks, and a family whose case
    // some later phase still owes names it in prose, which is §4.8's own convention (ADR-0401).
    // Nothing here reads a tick — the acceptance run is what proves a case green, and this is
    // what proves the checklist points at something.
    let passage = subsection("#### 4.8.13 The fourteen acceptance families");
    let families = boxes(&passage);
    assert_eq!(
        families.len(),
        FAMILIES.len(),
        "§40.3 names {} acceptance families and §4.8.13 holds {} boxes",
        FAMILIES.len(),
        families.len()
    );

    let mut owed = Vec::new();
    for (index, expected) in FAMILIES.iter().enumerate() {
        let (_, text) = &families[index];
        assert!(
            text.starts_with(&format!("**{expected}**")),
            "§4.8.13's box {} is §40.3's `{expected}` family, got {text:?}",
            index + 1
        );
        let named = case_names(text);
        let (name, quoted) = named.first().unwrap_or_else(|| {
            panic!("the `{expected}` family names no acceptance case: {text:?}")
        });
        let path = repo()
            .join("docker/acceptance/cases")
            .join(format!("{name}.case"));
        if path.is_file() {
            assert!(
                *quoted,
                "case `{name}` exists, so the `{expected}` family claims it in backticks rather \
                 than recording it absent (ADR-0401)"
            );
        } else {
            assert!(
                !*quoted,
                "the `{expected}` family claims case `{name}` in backticks and there is no such \
                 file. Write the case, or record the name in prose until the increment that owes \
                 it lands (ADR-0401)"
            );
            owed.push((expected, name.clone(), text.clone()));
        }
    }

    for (family, name, text) in &owed {
        assert!(
            text.contains("H11") || text.contains("H12"),
            "the `{family}` family's case `{name}` is not written yet, so the box says which \
             phase owes it — otherwise a family nobody delivered reads like one nobody noticed: \
             {text:?}"
        );
    }
    assert!(
        owed.len() <= 1,
        "at most one of §40.3's families may still be owed by a later phase, and {} are: {:?}",
        owed.len(),
        owed.iter().map(|(family, _, _)| family).collect::<Vec<_>>()
    );
}

#[test]
fn should_find_a_finite_timeout_on_every_v041_case() {
    // §40.4: "Every acceptance case MUST have a finite timeout. A timeout is a failure unless the
    // case explicitly asserts timeout behavior as its product result." The harness applies one to
    // every case; the v0.4.1 cases state their own, because a case that runs a benchmark under a
    // container is not a case the default was chosen for.
    let harness = read("scripts/acceptance.sh");
    assert!(
        harness.contains("want_timeout=\"30\""),
        "the harness gives every case a finite default timeout"
    );
    assert!(
        harness.contains("timeout --kill-after=5 \"$want_timeout\""),
        "the harness enforces the budget with `timeout`, so a case cannot outlive it"
    );
    assert!(
        harness.contains("problem=\"timed out after ${want_timeout}s\""),
        "§40.4: a timeout is reported as a failure of the case, not as a slow pass"
    );

    let cases = repo().join("docker/acceptance/cases");
    let mut without = Vec::new();
    for entry in std::fs::read_dir(&cases).expect("the case directory is readable") {
        let path = entry.expect("a directory entry").path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.ends_with(".case") {
            continue;
        }
        let number: u32 = name
            .split('-')
            .next()
            .and_then(|digits| digits.parse().ok())
            .unwrap_or_else(|| panic!("every case is numbered, and `{name}` is not"));
        if !(170..=200).contains(&number) {
            continue;
        }
        let text = read(&format!("docker/acceptance/cases/{name}"));
        let declared = text
            .lines()
            .find_map(|line| line.strip_prefix("timeout: "))
            .and_then(|value| value.trim().parse::<u32>().ok());
        match declared {
            Some(seconds) if seconds > 0 => {}
            _ => without.push(name.to_owned()),
        }
    }
    assert!(
        without.is_empty(),
        "§40.4: every v0.4.1 case states a finite, positive timeout of its own, and these do \
         not: {without:?}"
    );
}

/// The text of `docs/ACCEPTANCE.md` §4.8, which is the v0.4.1 definition of done.
///
/// The passage begins at its own heading and ends where the next tranche's would begin, so this
/// harvester and `xtask/tests/spatial_evidence.rs` read one checklist each and neither can answer
/// for the other's boxes (§4.8.1's first box).
fn checklist() -> String {
    let acceptance = read("docs/ACCEPTANCE.md");
    let start = acceptance
        .find("### 4.8 The v0.4.1 tranche")
        .expect("docs/ACCEPTANCE.md carries §4.8, the v0.4.1 tranche");
    let end = ["\n### 4.9", "\n## 5. Stopping rule"]
        .iter()
        .find_map(|marker| acceptance[start..].find(marker))
        .map_or(acceptance.len(), |offset| start + offset);
    acceptance[start..end].to_owned()
}

/// Every box of §4.8, with the `####` subsection it stands under.
fn checklist_boxes() -> Vec<(bool, String, String)> {
    let passage = checklist();
    let mut found: Vec<(bool, String, String)> = Vec::new();
    let mut subsection = String::from("4.8");
    for line in passage.lines() {
        if let Some(heading) = line.strip_prefix("#### ") {
            subsection = heading.trim().to_owned();
            continue;
        }
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("- [x] ") {
            found.push((true, subsection.clone(), rest.to_owned()));
        } else if let Some(rest) = trimmed.strip_prefix("- [ ] ") {
            found.push((false, subsection.clone(), rest.to_owned()));
        } else if line.starts_with(' ')
            && !trimmed.is_empty()
            && let Some((_, _, text)) = found.last_mut()
        {
            text.push(' ');
            text.push_str(trimmed);
        }
    }
    found
}

/// The priority class a box declares — `P0`…`P3` — or `None` for §4.8.13's family boxes, which
/// carry the acceptance family's name instead.
fn priority(text: &str) -> Option<String> {
    let opener = text.strip_prefix("**")?;
    let (class, _) = opener.split_once(" · ")?;
    matches!(class, "P0" | "P1" | "P2" | "P3").then(|| class.to_owned())
}

/// Every `ADR-NNNN` a text names, in the order it names them.
fn named_adrs(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (at, _) in text.match_indices("ADR-") {
        let digits: String = text[at + 4..]
            .chars()
            .take(4)
            .take_while(char::is_ascii_digit)
            .collect();
        if digits.len() == 4 {
            found.push(format!("ADR-{digits}"));
        }
    }
    found.dedup();
    found
}

/// The `- Date:` an ADR carries, or `None` when no ADR of that id exists.
fn adr_date(id: &str) -> Option<String> {
    let entry = std::fs::read_dir(repo().join("docs/adr"))
        .expect("the decisions directory is readable")
        .flatten()
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("{id}-"))
        })?;
    let text = std::fs::read_to_string(entry.path()).expect("an ADR is readable");
    text.lines()
        .find_map(|line| line.strip_prefix("- Date: "))
        .map(|date| date.trim().to_owned())
}

/// The release-candidate freeze, stated once in §4.8.14 and read from there.
fn freeze() -> String {
    let passage = subsection("#### 4.8.14 Zero unresolved P0/P1");
    passage
        .lines()
        .find_map(|line| line.split_once("release-candidate freeze is **"))
        .and_then(|(_, rest)| rest.split_once("**"))
        .map(|(date, _)| date.to_owned())
        .expect("§4.8.14 states the release-candidate freeze as a date")
}

/// Whether an exclusion recorded on `date` was written before `freeze`.
///
/// Both are ISO-8601 days, which order the same as their text, so the comparison is the string's.
fn exclusion_is_timely(date: &str, freeze: &str) -> bool {
    date <= freeze
}

/// Every bullet of §66.1–§66.8, and the box of §4.8 that names its proof.
///
/// The mapping is the reviewer's, in §66's order; what this file guarantees is that no criterion
/// is left without a box and that the box it names is really there. §66.9 is not in the table: it
/// is the rule *about* the table, and §4.8.14 is where it is held.
const RELEASE_CRITERIA: [(&str, &str); 46] = [
    (
        "Direct TCP server and client cryptographically authenticate each other.",
        "P0 · The client verifies the server the same way the server verifies the client.",
    ),
    (
        "Unknown clients cannot reach provider negotiation/data.",
        "P0 · The listener requires an authenticated client certificate.",
    ),
    (
        "Authorized clients receive only policy-allowed operations.",
        "P0 · The offer a client receives is filtered by its policy.",
    ),
    (
        "`Act` requires exact granted capability.",
        "P0 · An action grant is an exact capability ID.",
    ),
    (
        "Direct-link downgrade is not automatic.",
        "P0 · Downgrade is impossible and never automatic.",
    ),
    (
        "Key files and authorization stores have secure/strict handling.",
        "P0 · A readable private key is refused, and the diagnostic prints no key material.",
    ),
    (
        "Mandatory KUANG native confinement failures prevent plugin execution.",
        "P0 · A pre-exec failure prevents the exec.",
    ),
    (
        "Documentation accurately describes native trust/isolation boundaries.",
        "P0 · The documentation states the native tier honestly.",
    ),
    (
        "Materialization has item and byte limits.",
        "P1 · Materialization is bounded in items and in bytes.",
    ),
    (
        "Captures use shared budgets.",
        "P1 · Capture buffers go through the same budget.",
    ),
    (
        "Recent result history has total/per-result byte ceilings and truthful truncation markers.",
        "P1 · Retained history is bounded and says when it truncated.",
    ),
    (
        "Finite-required operations refuse unbounded input before waiting indefinitely.",
        "P1 · An operation that needs finite input refuses an unbounded one immediately.",
    ),
    (
        "`each` consumes and emits incrementally.",
        "P1 · `each` consumes and emits incrementally.",
    ),
    (
        "`each` works with unbounded sources.",
        "P1 · `each` accepts an unbounded source.",
    ),
    (
        "backpressure and cancellation remain bounded.",
        "P1 · Backpressure and cancellation survived the rewrite.",
    ),
    (
        "implementation-convenience captures have been removed or explicitly justified/bounded.",
        "P1 · Every remaining capture is classified and justified.",
    ),
    (
        "cross-kind stream ordering semantics are documented.",
        "P2 · Cross-kind stream ordering is documented and tested.",
    ),
    (
        "Profile S/M/L fixtures exist.",
        "P1 · Profile S, M and L fixtures exist and are reproducible.",
    ),
    (
        "time-to-first-result is measured.",
        "P1 · Time to first result is measured, and a blank hang fails.",
    ),
    (
        "`map --live` no longer exhibits the reproduced long blank hang on supported profiles.",
        "P1 · `map --live` produces a first frame and can be cancelled.",
    ),
    (
        "selector/completion targets are met or bounded refusal behavior is implemented.",
        "P1 · A selector miss costs about what a hit costs.",
    ),
    (
        "cancellation under load is verified.",
        "P1 · A full-screen map stays responsive while a projection is in flight.",
    ),
    (
        "no silent test skips remain in covered patterns;",
        "P2 · A test run has three visible outcomes.",
    ),
    (
        "expected skips are machine-readable and checked;",
        "P2 · An unexpected skip fails the gate.",
    ),
    (
        "shared test helpers are consolidated;",
        "P2 · The shared test helpers are canonical.",
    ),
    (
        "normal gate fuzzing passes;",
        "P2 · Coverage-guided fuzzing is scheduled and the gate fuzzing stays fast.",
    ),
    (
        "scheduled coverage-guided fuzzing exists;",
        "P2 · Corpora persist and a hang is a failure.",
    ),
    (
        "targeted Miri/sanitizer jobs exist and are green for the release commit.",
        "P2 · Miri and the sanitizers run on the unsafe boundary.",
    ),
    (
        "parser responsibilities are decomposed without grammar regression;",
        "P2 · The parser is navigable by responsibility.",
    ),
    (
        "evaluator responsibilities are decomposed without execution regression;",
        "P2 · The evaluator is navigable by responsibility.",
    ),
    (
        "session state is segmented enough that history/resource invariants have a clear owner;",
        "P2 · Session state has owners.",
    ),
    (
        "no new cross-crate dependency inversion was introduced.",
        "P2 · No cross-crate dependency inversion was introduced.",
    ),
    (
        "required GitHub Actions are pinned by commit SHA;",
        "P2 · Every required Action is pinned by commit SHA.",
    ),
    (
        "release-critical container images are pinned by digest;",
        "P2 · Every release-critical image is pinned by digest.",
    ),
    (
        "Rust/tool dependencies are locked/pinned;",
        "P2 · Tools and toolchain are exact, and the fetch is reproducible.",
    ),
    (
        "dependency advisory/policy checks pass;",
        "P2 · The dependency policy is enforced and provably fails.",
    ),
    (
        "release packages rebuild identically in two clean runs;",
        "P2 · Two clean builds produce identical packages.",
    ),
    (
        "checksum manifest exists;",
        "P2 · `SHA256SUMS` covers every downloadable artifact, deterministically ordered.",
    ),
    (
        "signatures exist and verify;",
        "P2 · The manifest is signed and the signature verifies.",
    ),
    (
        "provenance exists and binds all published artifacts;",
        "P2 · Provenance binds seven fields to every artifact digest.",
    ),
    (
        "the exact tested bytes are the published bytes.",
        "P2 · The tested bytes are the published bytes.",
    ),
    (
        "README/Wiki/help use the security terminology contract;",
        "P2 · The security terminology contract holds across every document.",
    ),
    (
        "generated repository metrics are current;",
        "P3 · The repository metrics are computed, not typed.",
    ),
    (
        "remote client authorization migration is documented;",
        "P2 · The migration path is written down.",
    ),
    (
        "release verification instructions are documented;",
        "P2 · Verification instructions exist and work.",
    ),
    (
        "`docs/STATE.md`, acceptance documentation and release notes agree on status.",
        "P2 · The status documents agree.",
    ),
];

/// The bullets of §66.1–§66.8, read from the specification rather than from the table above.
fn release_definition() -> Vec<String> {
    let spec = read("docs/specs/ono_sendai_shell_spec_v0.4.1_hardening_trust_release_integrity.md");
    let start = spec
        .find("# 66. Release Definition")
        .expect("the specification carries §66");
    let end = spec[start..]
        .find("# 66.9 Zero unresolved")
        .map_or(spec.len(), |offset| start + offset);
    spec[start..end]
        .lines()
        .filter_map(|line| line.strip_prefix("- "))
        .map(|bullet| bullet.trim().to_owned())
        .collect()
}

#[test]
fn should_read_the_v041_checklist_apart_from_the_v04_one() {
    // §4.8.1's first box: the two harvesters read one checklist each. §4.7's passage ends at this
    // subsection's heading and §4.8's begins there, so a proof named in one is never counted as
    // evidence for a box in the other — which is what let §4.7 stay whole while §4.8 was still
    // naming tests its delivering increments had not written.
    let acceptance = read("docs/ACCEPTANCE.md");
    let spatial = &acceptance[acceptance
        .find("### 4.7 The v0.4 tranche")
        .expect("§4.7 is there")..];
    let spatial = &spatial[..spatial.find("\n### 4.8").expect("§4.7 ends at §4.8")];
    let hardening = checklist();

    assert!(
        !spatial.contains("### 4.8"),
        "§4.7's passage reaches into the v0.4.1 checklist"
    );
    assert!(
        !hardening.contains("### 4.7") && !hardening.contains("### 4.9"),
        "§4.8's passage reaches into a neighbouring tranche's checklist"
    );
    assert!(
        hardening.len() > 40_000 && spatial.len() > 20_000,
        "both passages are whole checklists: §4.7 is {} bytes and §4.8 is {}",
        spatial.len(),
        hardening.len()
    );
}

#[test]
fn should_find_every_test_the_v041_checklist_names_as_a_proof() {
    // §3's rule for every subsection: a box is ticked by a named test running un-ignored in the
    // gate. A proof that was renamed away, never written or left ignored leaves a box ticked by
    // nothing, and this is the mechanical statement that none of §4.8's is.
    assert_proofs_exist(&checklist(), "docs/ACCEPTANCE.md §4.8", 150);
}

#[test]
fn should_find_every_acceptance_case_the_v041_checklist_names() {
    // ADR-0401's convention, applied to the whole subsection: a case named in backticks is a
    // claim that the referee collects it, and a case named in prose is a name recorded absent.
    // Both are checked, because a claim about a file nobody wrote and a file nobody claimed are
    // the same defect seen from two sides. A case named by its bare number is a claim too —
    // §4.8.13's preamble names seven of them that way — so those are resolved as well.
    let passage = checklist();
    let named = case_names(&passage);
    let claimed = named.iter().filter(|(_, quoted)| *quoted).count();
    assert!(
        claimed >= 15,
        "§4.8 claims the tranche's cases 180–200; the harvester found {claimed}"
    );
    let collected: Vec<String> = std::fs::read_dir(repo().join("docker/acceptance/cases"))
        .expect("the acceptance cases exist")
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "case"))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();

    let mut wrong = Vec::new();
    for (name, quoted) in named {
        let exists = collected.iter().any(|case| case == &format!("{name}.case"));
        match (quoted, exists) {
            (true, false) => wrong.push(format!("`{name}` is claimed and the referee has no file")),
            (false, true) => {
                wrong.push(format!("{name} is recorded absent and the file is there"));
            }
            _ => {}
        }
    }

    let numbers: std::collections::BTreeSet<String> = passage
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|token| token.len() == 3 && token.chars().all(|c| c.is_ascii_digit()))
        .map(str::to_owned)
        .collect();
    for number in numbers {
        if !collected.iter().any(|case| case.starts_with(&number)) {
            wrong.push(format!(
                "case `{number}` is named by its number and the referee has no such case"
            ));
        }
    }

    assert!(
        wrong.is_empty(),
        "§4.8 and `docker/acceptance/cases/` disagree: {wrong:#?}"
    );
}

#[test]
fn should_find_every_p0_and_p1_box_of_the_v041_checklist_ticked() {
    // §66.9, the binding release criterion of this tranche: "There MUST be no known unresolved P0
    // or P1 issue in the v0.4.1 scope at final release." §3.1 makes the priority a property of the
    // box, so the criterion is checkable here rather than against the tracker's labels.
    let boxes = checklist_boxes();
    let mandatory: Vec<&(bool, String, String)> = boxes
        .iter()
        .filter(|(_, _, text)| matches!(priority(text).as_deref(), Some("P0" | "P1")))
        .collect();
    assert!(
        mandatory.len() >= 40,
        "§4.8's mandatory scope is dozens of boxes; the harvester found {}",
        mandatory.len()
    );
    let open: Vec<String> = mandatory
        .iter()
        .filter(|(ticked, _, _)| !*ticked)
        .map(|(_, subsection, text)| {
            format!("§{subsection}: {}", text.split("**").nth(1).unwrap_or(text))
        })
        .collect();
    assert!(
        open.is_empty(),
        "§66.9 allows no unresolved P0 or P1 at final release, and these are open: {open:#?}"
    );
}

#[test]
fn should_find_a_dated_adr_for_every_box_the_checklist_leaves_open() {
    // §66.9's second sentence: "A P2/P3 issue MAY remain only if it is explicitly excluded from
    // this specification through an ADR made before release candidate freeze." So an open box is
    // legible from the checklist alone — its class allows the exclusion, and it names the dated
    // ADR that records it.
    let freeze = freeze();
    let mut wrong = Vec::new();
    for (ticked, subsection, text) in checklist_boxes() {
        if ticked {
            continue;
        }
        let title = text.split("**").nth(1).unwrap_or(&text).to_owned();
        let at = format!("§{subsection}: {title}");
        match priority(&text).as_deref() {
            Some("P0" | "P1") => wrong.push(format!("{at} — §66.9 excludes no P0 or P1")),
            _ => {
                let dated = named_adrs(&text)
                    .into_iter()
                    .filter_map(|id| adr_date(&id).map(|date| (id, date)))
                    .collect::<Vec<(String, String)>>();
                if dated.is_empty() {
                    wrong.push(format!("{at} — names no ADR that exists"));
                } else if !dated
                    .iter()
                    .any(|(_, date)| exclusion_is_timely(date, &freeze))
                {
                    wrong.push(format!(
                        "{at} — every ADR it names postdates the freeze {freeze}"
                    ));
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "§66.9 allows an open box only as a dated exclusion, and these are not: {wrong:#?}"
    );
}

#[test]
fn should_refuse_an_exclusion_adr_dated_after_the_release_candidate_freeze() {
    // The guard's own guard. "Before release candidate freeze" is the whole force of §66.9's
    // exception: an ADR written afterwards is a decision taken to explain a box that was already
    // open, which is the opposite of a scope decision. The freeze is stated in §4.8.14 so that
    // one date governs the tranche and no test carries a second copy of it.
    let freeze = freeze();
    assert_eq!(
        freeze.len(),
        10,
        "the freeze is an ISO-8601 day, and §4.8.14 states `{freeze}`"
    );
    assert!(
        freeze.chars().all(|c| c.is_ascii_digit() || c == '-'),
        "the freeze is an ISO-8601 day, and §4.8.14 states `{freeze}`"
    );

    assert!(
        exclusion_is_timely("2026-09-01", "2026-09-04"),
        "an ADR written before the freeze records a scope decision"
    );
    assert!(
        exclusion_is_timely("2026-09-04", "2026-09-04"),
        "the freeze is the last day an exclusion may be decided on"
    );
    assert!(
        !exclusion_is_timely("2026-09-05", "2026-09-04"),
        "an ADR written after the freeze explains an open box rather than excluding it"
    );
}

#[test]
fn should_find_a_box_for_every_bullet_of_the_release_definition() {
    // §66.9's second paragraph: an exclusion "cannot waive a release criterion listed above". The
    // way a criterion could be waived without anyone deciding to is by losing its box — a box
    // rewritten, merged into a neighbour or dropped takes its criterion with it, and the checklist
    // still reads as complete. So every bullet of §66.1–§66.8 is held against the box that names
    // its proof. Nothing here reads a tick: whether a box is ticked is `scripts/release-check.sh`'s
    // own rule — it fails on the first `- [ ]` in this file — and whether an open one is a lawful
    // exclusion is `should_find_a_dated_adr_for_every_box_the_checklist_leaves_open`'s (ADR-0575).
    let bullets = release_definition();
    assert_eq!(
        bullets.len(),
        RELEASE_CRITERIA.len(),
        "§66.1–§66.8 carry {} bullets and the table maps {}",
        bullets.len(),
        RELEASE_CRITERIA.len()
    );
    let boxes = checklist_boxes();
    let mut wrong = Vec::new();
    for (index, (bullet, opener)) in RELEASE_CRITERIA.iter().enumerate() {
        assert_eq!(
            &bullets[index],
            bullet,
            "§66's bullet {} reads differently in the specification than in the table",
            index + 1
        );
        let needle = format!("**{opener}**");
        let matched: Vec<&(bool, String, String)> = boxes
            .iter()
            .filter(|(_, _, text)| text.starts_with(&needle))
            .collect();
        match matched.as_slice() {
            [] => wrong.push(format!("`{bullet}` — no box opens `{opener}`")),
            [_] => {}
            several => wrong.push(format!(
                "`{bullet}` — {} boxes open `{opener}`",
                several.len()
            )),
        }
    }
    assert!(
        wrong.is_empty(),
        "§66 criteria whose box in §4.8 cannot be resolved: {wrong:#?}"
    );
}
