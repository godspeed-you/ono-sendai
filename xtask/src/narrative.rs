//! The rules that keep the set of narrative specifications honest.
//!
//! The specification stopped being one file the moment the user added an enhancement beside the
//! base. That is a deliberate shape — a base nobody rewrites, plus later documents that extend
//! it — and it needs two guarantees the single-file rule used to give for free: that every one of
//! them is covered by the immutability checksum, and that the agent instructions enumerate all of
//! them. The first enhancement satisfied neither, and was discovered by a red gate rather than by
//! being read, which is the failure this module exists to prevent from repeating.

use std::path::Path;

use crate::scan::Problem;

/// The files whose instructions an agent follows, and which must therefore name the base.
const INSTRUCTIONS: [&str; 3] = ["AGENTS.md", "CLAUDE.md", "README.md"];

/// Checks the narrative specifications found under `docs/`.
///
/// The base specification is the earliest by name; everything else is an enhancement layered on
/// top of it (ADR-0026).
#[must_use]
pub fn check(root: &Path) -> Vec<Problem> {
    let specs = narrative_specs(root);
    let Some((base, enhancements)) = specs.split_first() else {
        return vec![Problem {
            location: "docs/".to_owned(),
            detail: "no narrative specification found under docs/; the product has nothing to be \
                     measured against"
                .to_owned(),
        }];
    };

    let mut problems = Vec::new();
    problems.extend(check_checksums(root, &specs));
    problems.extend(check_instructions(root, base, enhancements));
    problems
}

/// The narrative specifications, sorted, so the base comes first.
#[must_use]
pub fn narrative_specs(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join("docs")) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.contains("shell_spec") && name.ends_with(".md"))
        .collect();
    found.sort();
    found
}

/// Every specification must be covered by `docs/spec.sha256`, or nothing proves it untouched.
fn check_checksums(root: &Path, specs: &[String]) -> Vec<Problem> {
    let recorded =
        std::fs::read_to_string(root.join("docs").join("spec.sha256")).unwrap_or_default();
    specs
        .iter()
        .filter(|name| !recorded.contains(name.as_str()))
        .map(|name| Problem {
            location: "docs/spec.sha256".to_owned(),
            detail: format!(
                "docs/{name} has no checksum entry, so no gate run would notice it being edited. \
                 Add its `sha256sum` line."
            ),
        })
        .collect()
}

/// The instructions must name the base, and `AGENTS.md` must name every enhancement as well.
///
/// An enhancement no instruction file mentions is one no agent reads: the authoritative
/// instruction set is where the complete specification list belongs.
fn check_instructions(root: &Path, base: &str, enhancements: &[String]) -> Vec<Problem> {
    let mut problems = Vec::new();
    for file in INSTRUCTIONS {
        let Ok(text) = std::fs::read_to_string(root.join(file)) else {
            problems.push(Problem {
                location: file.to_owned(),
                detail: format!("{file} is missing"),
            });
            continue;
        };
        if !text.contains(base) {
            problems.push(Problem {
                location: file.to_owned(),
                detail: format!("{file} does not reference the base specification `docs/{base}`"),
            });
        }
        if file != "AGENTS.md" {
            continue;
        }
        for enhancement in enhancements {
            if !text.contains(enhancement.as_str()) {
                problems.push(Problem {
                    location: file.to_owned(),
                    detail: format!(
                        "{file} does not reference the enhancement specification \
                         `docs/{enhancement}`; an enhancement the instructions do not enumerate \
                         is one no agent reads"
                    ),
                });
            }
        }
    }
    problems
}

/// The example lines inside every ```ono fence of a markdown document: the lines a reader is
/// invited to type, without blank lines and `#` comments.
#[must_use]
pub fn ono_examples(markdown: &str) -> Vec<String> {
    let mut examples = Vec::new();
    let mut inside = false;
    for line in markdown.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            inside = !inside && trimmed.trim_start_matches('`').trim() == "ono";
            continue;
        }
        if inside && !trimmed.is_empty() && !trimmed.starts_with('#') {
            examples.push(trimmed.to_owned());
        }
    }
    examples
}

/// Every ```ono example in `markdown` that does not parse as a complete statement, reported
/// against `location`.
#[must_use]
pub fn check_examples_in(markdown: &str, location: &str) -> Vec<Problem> {
    ono_examples(markdown)
        .into_iter()
        .filter_map(|example| {
            let parsed = ono_parser::parse(&example);
            if !parsed.has_errors() && parsed.is_complete() {
                return None;
            }
            let complaint = parsed.diagnostics().first().map_or_else(
                || "the line is unfinished".to_owned(),
                |diagnostic| format!("{}: {}", diagnostic.code().code(), diagnostic.message()),
            );
            Some(Problem {
                location: location.to_owned(),
                detail: format!("the example `{example}` does not parse: {complaint}"),
            })
        })
        .collect()
}

/// The README's ```ono examples must parse (spec §36.5: a doc example that no longer parses is
/// contract drift); running them is the job of `xtask/tests/adapter_evidence.rs`.
#[must_use]
pub fn check_readme_examples(root: &Path) -> Vec<Problem> {
    let readme = std::fs::read_to_string(root.join("README.md")).unwrap_or_default();
    check_examples_in(&readme, "README.md")
}
