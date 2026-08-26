//! Contract drift between `docs/spec/` and the implementation (spec §36.5).
//!
//! The registries are the public contract (spec §27), and a contract nobody checks is a
//! description of what someone once intended. This module is the check: every command's verb,
//! target, capability and schema must resolve; every schema must be internally consistent; every
//! argument mode must agree with the grammar of ADR-0009; and the error taxonomy must be
//! identical in the registry and in `ono-core`.
//!
//! A registry describes the whole product, not the part that exists today (ADR-0012), so a
//! `planned` entry may point at a schema a later phase will write. Anything `stable` may not.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_yaml_ng::Value as Yaml;

pub use crate::scan::Problem;

/// Checks every registry under `root/docs/spec` against every other.
///
/// Returns an empty vector when the contracts agree. Missing registries are not an error: they
/// arrive with the phase that needs them (AGENTS.md §14).
#[must_use]
pub fn check_contracts(root: &Path) -> Vec<Problem> {
    let spec = root.join("docs").join("spec");
    if !spec.is_dir() {
        return Vec::new();
    }

    let mut problems = Vec::new();
    let mut documents: BTreeMap<PathBuf, Yaml> = BTreeMap::new();

    for path in yaml_files(&spec) {
        let relative = relative(root, &path);
        match std::fs::read_to_string(&path) {
            Ok(text) if text.trim().is_empty() => problems.push(Problem {
                location: relative,
                detail: "the contract is empty; an empty contract promises nothing and hides that \
                         it promises nothing"
                    .to_owned(),
            }),
            Ok(text) => match serde_yaml_ng::from_str::<Yaml>(&text) {
                Ok(document) => {
                    documents.insert(path, document);
                }
                Err(error) => problems.push(Problem {
                    location: relative,
                    detail: format!("is not valid YAML: {error}"),
                }),
            },
            Err(error) => problems.push(Problem {
                location: relative,
                detail: format!("cannot be read: {error}"),
            }),
        }
    }

    let verbs = names_in(&documents, &spec.join("verbs.yaml"), "verbs", "verb");
    let targets = names_in(&documents, &spec.join("targets.yaml"), "targets", "name");
    let capabilities = names_in(
        &documents,
        &spec.join("capabilities.yaml"),
        "provider_capabilities",
        "id",
    );
    let expression_heads = expression_heads(&documents, &spec.join("language.yaml"));

    let schemas = collect_schemas(root, &spec, &documents, &mut problems);
    let deferred = collect_deferred(root, &spec, &schemas, &mut problems);
    check_commands(
        root,
        &documents,
        &verbs,
        &targets,
        &capabilities,
        &schemas,
        &deferred,
        &expression_heads,
        &mut problems,
    );
    problems.extend(check_error_registry(root));

    problems.sort_by(|a, b| (&a.location, &a.detail).cmp(&(&b.location, &b.detail)));
    problems
}

/// Checks `docs/spec/errors.yaml` against the taxonomy in `ono-core`.
///
/// This is the one part of spec §36.5 that can be checked exactly today, because the taxonomy
/// exists in both places. The registry is read as data and the implementation is read through
/// `ono_core::ErrorCode`, so neither can drift without the gate noticing.
#[must_use]
pub fn check_error_registry(root: &Path) -> Vec<Problem> {
    let path = root.join("docs").join("spec").join("errors.yaml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(document) = serde_yaml_ng::from_str::<Yaml>(&text) else {
        return Vec::new();
    };
    let location = relative(root, &path);
    let mut problems = Vec::new();

    let mut registered: BTreeMap<String, (String, String)> = BTreeMap::new();
    for entry in sequence(&document, "errors") {
        let Some(code) = string_at(entry, "code") else {
            problems.push(Problem {
                location: location.clone(),
                detail: "an error entry has no `code`".to_owned(),
            });
            continue;
        };
        registered.insert(
            code,
            (
                string_at(entry, "name").unwrap_or_default(),
                string_at(entry, "kind").unwrap_or_default(),
            ),
        );
    }

    for code in ono_core::ErrorCode::ALL {
        match registered.remove(code.code()) {
            None => problems.push(Problem {
                location: location.clone(),
                detail: format!(
                    "`{}` ({}) is implemented but the registry does not define it; spec §43 \
                     requires codes to be stable, which starts with them being written down",
                    code.code(),
                    code.name()
                ),
            }),
            Some((name, kind)) => {
                if name != code.name() {
                    problems.push(Problem {
                        location: location.clone(),
                        detail: format!(
                            "`{}` is `{name}` in the registry and `{}` in the implementation",
                            code.code(),
                            code.name()
                        ),
                    });
                }
                if kind != code.kind().as_str() {
                    problems.push(Problem {
                        location: location.clone(),
                        detail: format!(
                            "`{}` has kind `{kind}` in the registry and `{}` in the \
                             implementation (ADR-0006 fixes the mapping)",
                            code.code(),
                            code.kind().as_str()
                        ),
                    });
                }
            }
        }
    }

    for (code, (name, _)) in registered {
        problems.push(Problem {
            location: location.clone(),
            detail: format!(
                "the registry defines `{code}` ({name}) but nothing implements it; a code a \
                 script can match on must exist in the code that raises it"
            ),
        });
    }

    problems
}

/// Every schema id declared under `docs/spec/schemas/`, checked for internal consistency.
fn collect_schemas(
    root: &Path,
    spec: &Path,
    documents: &BTreeMap<PathBuf, Yaml>,
    problems: &mut Vec<Problem>,
) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let directory = spec.join("schemas");

    for (path, document) in documents {
        // `deferred.yaml` lives beside the schemas because it is about them, but it declares
        // none of its own.
        if !path.starts_with(&directory) || path.ends_with("deferred.yaml") {
            continue;
        }
        let location = relative(root, path);
        let Some(id) = string_at(document, "id") else {
            problems.push(Problem {
                location,
                detail: "a schema file has no `id`".to_owned(),
            });
            continue;
        };

        // `ono.process/1` must live in `process.v1.yaml`, so a reader can find a schema from its
        // id without grepping, and two files cannot quietly declare the same one.
        let expected = schema_file_name(&id);
        let actual = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if expected.as_deref() != Some(actual.as_str()) {
            problems.push(Problem {
                location: location.clone(),
                detail: format!(
                    "declares `{id}`, which belongs in `{}`",
                    expected.unwrap_or_else(|| "<unparsable id>".to_owned())
                ),
            });
        }

        let fields: BTreeSet<String> = mapping_keys(document, "fields");
        if fields.is_empty() {
            problems.push(Problem {
                location: location.clone(),
                detail: format!("`{id}` declares no fields"),
            });
        }

        for (name, field) in mapping_entries(document, "fields") {
            if string_at(field, "doc").is_none_or(|doc| doc.trim().is_empty()) {
                problems.push(Problem {
                    location: location.clone(),
                    detail: format!(
                        "field `{name}` of `{id}` has no `doc`; help is generated from here \
                         (spec §36.2), so an undocumented field is a blank in a help page"
                    ),
                });
            }
            if string_at(field, "type").is_none() {
                problems.push(Problem {
                    location: location.clone(),
                    detail: format!("field `{name}` of `{id}` has no `type`"),
                });
            }
        }

        for column in string_sequence(document, "identity") {
            if !fields.contains(&column) {
                problems.push(Problem {
                    location: location.clone(),
                    detail: format!(
                        "`{id}` is identified by `{column}`, which is not one of its fields"
                    ),
                });
            }
        }
        for column in default_view_columns(document) {
            if !fields.contains(&column) {
                problems.push(Problem {
                    location: location.clone(),
                    detail: format!(
                        "the default view of `{id}` shows `{column}`, which is not one of its \
                         fields"
                    ),
                });
            }
        }

        if !ids.insert(id.clone()) {
            problems.push(Problem {
                location,
                detail: format!("`{id}` is declared by more than one file"),
            });
        }
    }

    ids
}

/// Schemas a command already advertises and a later phase will write (ADR-0012).
///
/// The debt is enumerated rather than tolerated: a reference to a schema nobody has written is a
/// promise, and a promise nobody wrote down is one nobody will keep. An entry that names a schema
/// which now exists is reported too, so the list cannot quietly rot into permanence.
fn collect_deferred(
    root: &Path,
    spec: &Path,
    schemas: &BTreeSet<String>,
    problems: &mut Vec<Problem>,
) -> BTreeMap<String, String> {
    let path = spec.join("schemas").join("deferred.yaml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return BTreeMap::new();
    };
    let location = relative(root, &path);
    let Ok(document) = serde_yaml_ng::from_str::<Yaml>(&text) else {
        problems.push(Problem {
            location,
            detail: "is not valid YAML".to_owned(),
        });
        return BTreeMap::new();
    };

    let mut listed = BTreeMap::new();
    for entry in sequence(&document, "deferred") {
        let Some(id) = string_at(entry, "id") else {
            problems.push(Problem {
                location: location.clone(),
                detail: "a deferred entry has no `id`".to_owned(),
            });
            continue;
        };
        let Some(phase) = string_at(entry, "phase") else {
            problems.push(Problem {
                location: location.clone(),
                detail: format!("`{id}` is deferred without naming the phase that will write it"),
            });
            continue;
        };
        if schemas.contains(&id) {
            problems.push(Problem {
                location: location.clone(),
                detail: format!(
                    "`{id}` is listed as deferred but now exists; remove the entry so the list \
                     keeps meaning what it says"
                ),
            });
        }
        listed.insert(id, phase);
    }
    listed
}

#[expect(
    clippy::too_many_arguments,
    reason = "each registry is a distinct input to the cross-check; bundling them into a struct would name the same things twice"
)]
fn check_commands(
    root: &Path,
    documents: &BTreeMap<PathBuf, Yaml>,
    verbs: &BTreeSet<String>,
    targets: &BTreeSet<String>,
    capabilities: &BTreeSet<String>,
    schemas: &BTreeSet<String>,
    deferred: &BTreeMap<String, String>,
    expression_heads: &BTreeSet<String>,
    problems: &mut Vec<Problem>,
) {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();

    for (path, document) in documents {
        if !path
            .parent()
            .is_some_and(|parent| parent.ends_with("commands"))
        {
            continue;
        }
        let location = relative(root, path);

        for command in sequence(document, "commands") {
            let Some(id) = string_at(command, "id") else {
                problems.push(Problem {
                    location: location.clone(),
                    detail: "a command entry has no `id`".to_owned(),
                });
                continue;
            };
            let stability = string_at(command, "stability").unwrap_or_default();
            let planned = stability == "planned"
                || string_at(command, "phase").is_some_and(|phase| phase == "planned");

            if let Some(previous) = seen.insert(id.clone(), location.clone()) {
                problems.push(Problem {
                    location: location.clone(),
                    detail: format!(
                        "`{id}` is already claimed by {previous}; a stable id resolving to two \
                         commands is `resolve.ambiguous` waiting to happen (spec §40.4)"
                    ),
                });
            }

            let verb = string_at(command, "verb").unwrap_or_default();
            if !verb.is_empty() && !verbs.contains(&verb) {
                problems.push(Problem {
                    location: location.clone(),
                    detail: format!(
                        "`{id}` uses the verb `{verb}`, which `verbs.yaml` does not define; \
                         spec §7 keeps the verb registry small and curated on purpose"
                    ),
                });
            }
            let target = string_at(command, "target").unwrap_or_default();
            // A transform operates on whatever the pipeline carries and names no target, which
            // the registry writes as `target: null` (spec §53, ADR-0012).
            if !target.is_empty() && target != "null" && !targets.contains(&target) {
                problems.push(Problem {
                    location: location.clone(),
                    detail: format!(
                        "`{id}` uses the target `{target}`, which `targets.yaml` does not define"
                    ),
                });
            }
            if let Some(capability) = string_at(command, "provider_capability")
                && !capability.is_empty()
                && capability != "null"
                && !capabilities.contains(&capability)
            {
                problems.push(Problem {
                    location: location.clone(),
                    detail: format!(
                        "`{id}` requires the provider capability `{capability}`, which \
                         `capabilities.yaml` does not define"
                    ),
                });
            }

            if !planned {
                for field in ["input", "output"] {
                    for referenced in schema_ids_in(&string_at(command, field).unwrap_or_default())
                    {
                        if !schemas.contains(&referenced) && !deferred.contains_key(&referenced) {
                            problems.push(Problem {
                                location: location.clone(),
                                detail: format!(
                                    "`{id}` advertises `{referenced}` as its `{field}`, and no \
                                     schema declares it. Write the schema, or list it in \
                                     `docs/spec/schemas/deferred.yaml` with the phase that will \
                                     (ADR-0012)"
                                ),
                            });
                        }
                    }
                }
            }

            let mode = string_at(command, "argument_mode").unwrap_or_default();
            let should_be_expression = expression_heads.contains(&verb);
            if !mode.is_empty()
                && !expression_heads.is_empty()
                && (mode == "expression") != should_be_expression
            {
                problems.push(Problem {
                    location: location.clone(),
                    detail: format!(
                        "`{id}` declares `argument_mode: {mode}` for the head `{verb}`, which \
                         ADR-0009 parses in {} mode; completion and help would describe a \
                         language the parser does not implement",
                        if should_be_expression {
                            "expression"
                        } else {
                            "words"
                        }
                    ),
                });
            }

            if !planned && string_sequence(command, "examples").is_empty() {
                problems.push(Problem {
                    location: location.clone(),
                    detail: format!(
                        "`{id}` advertises no example; spec §50 requires documented examples to \
                         parse and execute, which needs there to be one"
                    ),
                });
            }
        }
    }
}

// --- reading the registries -----------------------------------------------------------------

fn names_in(
    documents: &BTreeMap<PathBuf, Yaml>,
    path: &Path,
    key: &str,
    field: &str,
) -> BTreeSet<String> {
    documents
        .get(path)
        .map(|document| {
            sequence(document, key)
                .into_iter()
                .filter_map(|entry| string_at(entry, field))
                .collect()
        })
        .unwrap_or_default()
}

fn expression_heads(documents: &BTreeMap<PathBuf, Yaml>, path: &Path) -> BTreeSet<String> {
    documents
        .get(path)
        .and_then(|document| document.get("argument_modes"))
        .map(|modes| {
            string_sequence(modes, "expression_heads")
                .into_iter()
                .collect()
        })
        .unwrap_or_default()
}

/// The schema ids mentioned in a type expression such as `stream<ono.process/1>`.
fn schema_ids_in(declaration: &str) -> Vec<String> {
    declaration
        .split(|c: char| !(c.is_alphanumeric() || c == '.' || c == '/' || c == '_' || c == '-'))
        .filter(|piece| piece.contains('/') && piece.starts_with("ono."))
        .map(str::to_owned)
        .collect()
}

/// `ono.process/1` lives in `process.v1.yaml`.
fn schema_file_name(id: &str) -> Option<String> {
    let (name, version) = id.split_once('/')?;
    let short = name.strip_prefix("ono.").unwrap_or(name);
    Some(format!("{}.v{version}.yaml", short.replace('.', "-")))
}

fn sequence<'a>(document: &'a Yaml, key: &str) -> Vec<&'a Yaml> {
    document
        .get(key)
        .and_then(Yaml::as_sequence)
        .map(|items| items.iter().collect())
        .unwrap_or_default()
}

fn string_at(document: &Yaml, key: &str) -> Option<String> {
    document
        .get(key)
        .and_then(Yaml::as_str)
        .map(str::to_owned)
        .or_else(|| {
            document
                .get(key)
                .filter(|value| value.is_null())
                .map(|_| "null".to_owned())
        })
}

fn string_sequence(document: &Yaml, key: &str) -> Vec<String> {
    sequence(document, key)
        .into_iter()
        .filter_map(|item| item.as_str().map(str::to_owned))
        .collect()
}

fn default_view_columns(document: &Yaml) -> Vec<String> {
    document
        .get("default_view")
        .map(|view| string_sequence(view, "columns"))
        .unwrap_or_default()
}

fn mapping_entries<'a>(document: &'a Yaml, key: &str) -> Vec<(String, &'a Yaml)> {
    document
        .get(key)
        .and_then(Yaml::as_mapping)
        .map(|mapping| {
            mapping
                .iter()
                .filter_map(|(name, value)| Some((name.as_str()?.to_owned(), value)))
                .collect()
        })
        .unwrap_or_default()
}

fn mapping_keys(document: &Yaml, key: &str) -> BTreeSet<String> {
    mapping_entries(document, key)
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

fn yaml_files(directory: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_yaml(directory, &mut files);
    files.sort();
    files
}

fn collect_yaml(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_yaml(&path, files);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "yaml")
        {
            files.push(path);
        }
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}
