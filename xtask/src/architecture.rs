//! The module decomposition and crate layering of v0.4.1 §29–§31 and §56, checked against the tree.
//!
//! Phase H9 cut three files into modules under one rule: no test may change (AGENTS.md §11,
//! v0.4.1 §65.12). That rule is what makes the result trustworthy and also what leaves it
//! undefended — a decomposition proved by an *unchanged* suite has, by construction, no test of
//! its own. §66.6 asks for navigability, which is a property of the layout, so this module reads
//! `docs/contracts/hardening/module_architecture.yaml` and holds the tree to it.
//!
//! Every rule here runs in both directions. A responsibility the specification names must have a
//! module, **and** a module must be a responsibility somebody named: the second half is what stops
//! the decomposition from growing a quiet home for whatever did not fit, which is precisely how a
//! split file reassembles itself.
//!
//! One rule is a proxy and says so. The gate cannot recognise "domain logic moved up into the
//! composition root" (§30.4) by reading it, so `composition_root` declares `ono-cli`'s top-level
//! modules and an undeclared one fails: adding a module to the composition root becomes a decision
//! somebody writes down. The rule beside it is not a proxy — a module of `ono-cli` may not be named
//! for a domain a lower crate owns.

use std::collections::BTreeMap;
use std::path::Path;

use serde_yaml_ng::Value as Yaml;

use crate::scan::Problem;

/// Where the declaration lives, relative to the repository root.
const REGISTRY: &str = "docs/contracts/hardening/module_architecture.yaml";

/// Reports every disagreement between the declared architecture and the tree.
#[must_use]
pub fn check(root: &Path) -> Vec<Problem> {
    let path = root.join(REGISTRY);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return vec![Problem::new(
            REGISTRY,
            "is missing, so nothing states which responsibility owns which module (v0.4.1 §29.2, \
             §30.2, §31.2, §56)"
                .to_owned(),
        )];
    };
    let Ok(document) = serde_yaml_ng::from_str::<Yaml>(&text) else {
        return vec![Problem::new(
            REGISTRY,
            "is not valid YAML, so the architecture it declares cannot be read".to_owned(),
        )];
    };

    let mut problems = Vec::new();
    problems.extend(check_modules(root, &document, "parser"));
    problems.extend(check_modules(root, &document, "evaluator"));
    problems.extend(check_modules(root, &document, "native_execution"));
    problems.extend(check_state_groups(root, &document));
    problems.extend(check_composition_root(root, &document));
    problems.extend(check_layering(root, &document));
    problems.sort_by(|left, right| left.location.cmp(&right.location));
    problems
}

/// Whether a declared responsibility has its module, and every module a responsibility.
fn check_modules(root: &Path, document: &Yaml, section: &str) -> Vec<Problem> {
    let Some(entry) = document.get(section) else {
        return vec![Problem::new(
            REGISTRY,
            format!("declares no `{section}` section"),
        )];
    };
    let Some(directory) = entry.get("root").and_then(Yaml::as_str) else {
        return vec![Problem::new(
            REGISTRY,
            format!("`{section}` declares no `root` directory"),
        )];
    };
    let crate_name = entry
        .get("crate")
        .and_then(Yaml::as_str)
        .unwrap_or_default();
    let spec = entry.get("spec").and_then(Yaml::as_str).unwrap_or_default();
    let base = root.join("crates").join(crate_name).join(directory);

    let mut problems = Vec::new();
    let mut declared: BTreeMap<String, String> = BTreeMap::new();
    for row in entry
        .get("responsibilities")
        .and_then(Yaml::as_sequence)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let module = row.get("module").and_then(Yaml::as_str).unwrap_or_default();
        let responsibility = row
            .get("responsibility")
            .and_then(Yaml::as_str)
            .unwrap_or_default();
        if row.get("optional").and_then(Yaml::as_bool) == Some(true)
            && row.get("reason").and_then(Yaml::as_str).is_none()
        {
            problems.push(Problem::new(
                REGISTRY,
                format!(
                    "`{section}` marks `{module}` optional and gives no reason. A module beyond \
                     what {spec} names is a decision, and a decision without a reason is a file \
                     somebody added."
                ),
            ));
        }
        if !base.join(module).exists() {
            problems.push(Problem::new(
                format!("{}/{directory}/{module}", crate_display(crate_name)),
                format!(
                    "is declared as the home of `{responsibility}` ({spec}) and does not exist. \
                     Either the responsibility moved and the declaration did not follow it, or \
                     the module was never written."
                ),
            ));
        }
        declared.insert(module.to_owned(), responsibility.to_owned());
    }

    for name in modules_in(&base) {
        if name == "mod.rs" || declared.contains_key(&name) {
            continue;
        }
        problems.push(Problem::new(
            format!("{}/{directory}/{name}", crate_display(crate_name)),
            format!(
                "is a module {spec} does not name and `{REGISTRY}` does not declare. A \
                 decomposition reassembles itself through exactly this file: the one nobody \
                 claimed. Give it a responsibility, or fold it into the module that owns its work."
            ),
        ));
    }
    problems
}

/// Whether §31.2's state groups exist as private types in the session.
fn check_state_groups(root: &Path, document: &Yaml) -> Vec<Problem> {
    let Some(entry) = document.get("session") else {
        return vec![Problem::new(
            REGISTRY,
            "declares no `session` section".to_owned(),
        )];
    };
    let Some(file) = entry.get("file").and_then(Yaml::as_str) else {
        return vec![Problem::new(
            REGISTRY,
            "`session` declares no `file`".to_owned(),
        )];
    };
    let crate_name = entry
        .get("crate")
        .and_then(Yaml::as_str)
        .unwrap_or_default();
    let path = root.join("crates").join(crate_name).join(file);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return vec![Problem::new(
            format!("{}/{file}", crate_display(crate_name)),
            "is declared as the home of §31.2's state groups and cannot be read".to_owned(),
        )];
    };

    let mut problems = Vec::new();
    for row in entry
        .get("state_groups")
        .and_then(Yaml::as_sequence)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let group = row.get("group").and_then(Yaml::as_str).unwrap_or_default();
        let owns = row.get("owns").and_then(Yaml::as_str).unwrap_or_default();
        if !text.contains(&format!("struct {group} "))
            && !text.contains(&format!("struct {group}<"))
        {
            problems.push(Problem::new(
                format!("{}/{file}", crate_display(crate_name)),
                format!(
                    "declares no `{group}`, which §31.2 names as the owner of {owns}. A session \
                     whose state has no owner is the flat field list §31.3 asks to be replaced."
                ),
            ));
        }
    }
    problems
}

/// Whether the composition root holds only modules somebody declared.
fn check_composition_root(root: &Path, document: &Yaml) -> Vec<Problem> {
    let Some(entry) = document.get("composition_root") else {
        return vec![Problem::new(
            REGISTRY,
            "declares no `composition_root` section".to_owned(),
        )];
    };
    let crate_name = entry
        .get("crate")
        .and_then(Yaml::as_str)
        .unwrap_or_default();
    let directory = entry.get("root").and_then(Yaml::as_str).unwrap_or("src");
    let base = root.join("crates").join(crate_name).join(directory);

    let declared: Vec<String> = entry
        .get("modules")
        .and_then(Yaml::as_sequence)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect();

    let owned_elsewhere = crate_domains(root, crate_name);
    let excused: Vec<String> = entry
        .get("named_for_a_lower_crate")
        .and_then(Yaml::as_sequence)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter(|row| row.get("reason").and_then(Yaml::as_str).is_some())
        .filter_map(|row| row.get("module").and_then(Yaml::as_str).map(str::to_owned))
        .collect();
    let mut problems = Vec::new();
    for name in modules_in(&base) {
        if !declared.contains(&name) {
            problems.push(Problem::new(
                format!("{}/{directory}/{name}", crate_display(crate_name)),
                format!(
                    "is a module of the composition root that `{REGISTRY}` does not declare. \
                     v0.4.1 §30.4 forbids moving domain logic up into `{crate_name}` to shrink a \
                     file, and a move like that leaves no failing test behind — so a new module \
                     here is a decision, and this is where it is written down."
                ),
            ));
        }
        let stem = name.trim_end_matches(".rs");
        if owned_elsewhere.contains(&stem.to_owned()) && !excused.contains(&name) {
            problems.push(Problem::new(
                format!("{}/{directory}/{name}", crate_display(crate_name)),
                format!(
                    "is named for a domain the crate `ono-{stem}` owns. Whatever it contains, a \
                     module of the composition root carrying a lower crate's name is the \
                     inversion §30.4 describes."
                ),
            ));
        }
    }
    problems
}

/// Whether every dependency edge respects the declared layer order.
fn check_layering(root: &Path, document: &Yaml) -> Vec<Problem> {
    let Some(entry) = document.get("layering") else {
        return vec![Problem::new(REGISTRY, "declares no `layering`".to_owned())];
    };
    let mut rank: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for (index, layer) in entry
        .get("layers")
        .and_then(Yaml::as_sequence)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let name = layer
            .get("layer")
            .and_then(Yaml::as_str)
            .unwrap_or_default();
        for value in layer
            .get("crates")
            .and_then(Yaml::as_sequence)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            if let Some(krate) = value.as_str() {
                rank.insert(krate.to_owned(), (index, name.to_owned()));
            }
        }
    }

    let mut problems = Vec::new();
    let Ok(entries) = std::fs::read_dir(root.join("crates")) else {
        return problems;
    };
    let mut crates: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    crates.sort();

    for krate in &crates {
        let Some((from, from_layer)) = rank.get(krate) else {
            problems.push(Problem::new(
                REGISTRY,
                format!(
                    "places no layer on the crate `{krate}`. A crate outside the layering is a \
                     crate the rule cannot hold, which is the same as having no rule."
                ),
            ));
            continue;
        };
        for dependency in workspace_dependencies(root, krate) {
            let Some((to, to_layer)) = rank.get(&dependency) else {
                continue;
            };
            if to > from {
                problems.push(Problem::new(
                    format!("crates/{krate}/Cargo.toml"),
                    format!(
                        "depends on `{dependency}`, which the layering places in `{to_layer}`, \
                         above `{krate}`'s own `{from_layer}`. §56 and §30.4: a crate may reach \
                         into its own layer or below it and never upwards."
                    ),
                ));
            }
        }
    }
    problems
}

/// The crates a crate depends on inside this workspace.
fn workspace_dependencies(root: &Path, krate: &str) -> Vec<String> {
    let path = root.join("crates").join(krate).join("Cargo.toml");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut found: Vec<String> = text
        .lines()
        .filter_map(|line| line.trim().strip_suffix(".workspace = true"))
        .filter(|name| name.starts_with("ono-") && *name != krate)
        .map(str::to_owned)
        .collect();
    found.sort();
    found.dedup();
    found
}

/// The domain names other crates own, as bare stems: `ono-parser` becomes `parser`.
fn crate_domains(root: &Path, own: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join("crates")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name != own)
        .filter_map(|name| name.strip_prefix("ono-").map(str::to_owned))
        .collect()
}

/// The `.rs` files and subdirectories directly inside `base`, sorted.
fn modules_in(base: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(base) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|entry| {
            entry.path().is_dir() || entry.path().extension().is_some_and(|ext| ext == "rs")
        })
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// `crates/<name>`, for a location a reader can paste into an editor.
fn crate_display(krate: &str) -> String {
    format!("crates/{krate}")
}
