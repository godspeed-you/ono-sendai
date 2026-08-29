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
    let language = spec.join("language.yaml");
    let expression_heads = expression_heads(&documents, &language);
    if expression_heads.as_ref().is_some_and(BTreeSet::is_empty) {
        problems.push(Problem {
            location: relative(root, &language),
            detail: "declares no expression-mode heads, so the argument mode of every command \
                     would be cross-checked against an empty set and nothing could disagree \
                     (ADR-0009)"
                .to_owned(),
        });
    }

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
        expression_heads.as_ref(),
        &mut problems,
    );
    problems.extend(check_expression_options(root, &documents, &spec));
    problems.extend(check_declared_options(root, &documents));
    problems.extend(check_error_registry(root));
    problems.extend(check_adapter_packs(root));
    problems.extend(check_spatial_registry(root));
    problems.extend(check_spatial_implementation(root));
    problems.extend(check_provider_claims(root));
    problems.extend(check_kuang_contracts(root));

    problems.sort_by(|a, b| (&a.location, &a.detail).cmp(&(&b.location, &b.detail)));
    problems
}

/// Checks that every option a command declares is named somewhere the shell can read it
/// (ADR-0233).
///
/// A capability a provider does not declare cannot be used, and `check_commands` has always said
/// so. The opposite direction had no referee: a command could advertise `--keep-grants` in its
/// help, its completion and its reference page while no code ever looked at the word, and the
/// user found out by the answer being wrong rather than by being refused.
///
/// What a static check can prove is the necessary condition: an option no implementation *names*
/// cannot be honoured. Only the crate sources count — `crates/*/src` and `xtask/src` — because a
/// test naming an option proves the test knows about it, not the shell. Both spellings count,
/// bare (`"tree"`, as a provider query reads it) and with dashes (`"--problems"`, as a builtin
/// reads its words).
fn check_declared_options(root: &Path, documents: &BTreeMap<PathBuf, Yaml>) -> Vec<Problem> {
    let mut sources = String::new();
    for crate_root in [root.join("crates"), root.join("xtask")] {
        collect_rust_sources(&crate_root, &mut sources);
    }

    let mut problems = Vec::new();
    for (path, document) in documents {
        if !path
            .parent()
            .is_some_and(|parent| parent.ends_with("commands"))
        {
            continue;
        }
        let location = relative(root, path);
        for command in sequence(document, "commands") {
            let stability = string_at(command, "stability").unwrap_or_default();
            if stability == "planned"
                || string_at(command, "phase").is_some_and(|phase| phase == "planned")
            {
                continue;
            }
            let id = string_at(command, "id").unwrap_or_default();
            for option in sequence(command, "options") {
                let Some(name) = string_at(option, "name") else {
                    continue;
                };
                if sources.contains(&format!("\"{name}\""))
                    || sources.contains(&format!("\"--{name}\""))
                {
                    continue;
                }
                problems.push(Problem {
                    location: location.clone(),
                    detail: format!(
                        "`{id}` declares the option `--{name}`, and no crate source names it. An                          option nothing reads is help text for behaviour that does not exist:                          implement it, or take it out of the contract (ADR-0233)"
                    ),
                });
            }
        }
    }
    problems
}

/// Appends the text of every `src/` Rust file under `directory` to `into`.
fn collect_rust_sources(directory: &Path, into: &mut String) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            collect_rust_sources(&path, into);
        } else if path.extension().is_some_and(|extension| extension == "rs")
            && path
                .components()
                .any(|component| component.as_os_str() == "src")
            && let Ok(text) = std::fs::read_to_string(&path)
        {
            into.push_str(&text);
            into.push('\n');
        }
    }
}

/// Checks that the predicate options of `language.yaml` are exactly the ones the parser reads
/// as expressions (ADR-0138).
///
/// A words-mode head may declare one option whose value is an expression rather than the next
/// word — `find place --where pid > 1`. The parser holds that table statically so the editor can
/// classify a line without a registry (ADR-0009), which means two places can disagree about what
/// `--where` means. They may not: help and completion would describe a language the parser does
/// not implement.
fn check_expression_options(
    root: &Path,
    documents: &BTreeMap<PathBuf, Yaml>,
    spec: &Path,
) -> Vec<Problem> {
    let path = spec.join("language.yaml");
    let Some(document) = documents.get(&path) else {
        return Vec::new();
    };
    let location = relative(root, &path);
    let mut problems = Vec::new();
    let mut declared: BTreeSet<(String, String)> = BTreeSet::new();

    for mode in sequence(document, "argument_modes") {
        for entry in sequence(mode, "option_values") {
            let (Some(head), Some(option)) = (string_at(entry, "head"), string_at(entry, "option"))
            else {
                problems.push(Problem {
                    location: location.clone(),
                    detail: "an `option_values` entry needs both a `head` and an `option`"
                        .to_owned(),
                });
                continue;
            };
            if !ono_parser::ArgMode::option_takes_expression(&head, &option) {
                problems.push(Problem {
                    location: location.clone(),
                    detail: format!(
                        "`{head} --{option}` is declared to take an expression, and the parser \
                         reads its value as a word (ADR-0138)"
                    ),
                });
            }
            declared.insert((head, option));
        }
    }

    for (head, option) in ono_parser::ArgMode::expression_options() {
        if !declared.contains(&((*head).to_owned(), (*option).to_owned())) {
            problems.push(Problem {
                location: location.clone(),
                detail: format!(
                    "the parser reads `{head} --{option}` as an expression, and `language.yaml` \
                     declares no `option_values` entry for it (ADR-0138)"
                ),
            });
        }
    }
    problems
}

/// Checks every first-party adapter pack under `docs/spec/adapters/first-party/`.
///
/// Spec v0.3 §1.44 wants the pack format machine-validated; `ono_adapter::validate` is the
/// validator the shell itself uses, so what passes here is what loads. A pack file that the
/// binary does not bundle is a contract nobody keeps, and is reported as such.
#[must_use]
pub fn check_adapter_packs(root: &Path) -> Vec<Problem> {
    let adapters = root.join("docs").join("spec").join("adapters");
    let first_party = adapters.join("first-party");
    if !first_party.is_dir() {
        return Vec::new();
    }
    let fixtures = adapters.join("fixtures");
    let bundled: BTreeSet<&str> = ono_adapter::first_party()
        .iter()
        .map(ono_adapter::AdapterPack::id)
        .collect();
    let mut problems = Vec::new();

    for path in yaml_files(&first_party) {
        let location = relative(root, &path);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let pack = match ono_adapter::AdapterPack::parse(&text) {
            Ok(pack) => pack,
            Err(error) => {
                problems.push(Problem {
                    location,
                    detail: format!("does not parse as an adapter pack: {error}"),
                });
                continue;
            }
        };
        if !bundled.contains(pack.id()) {
            problems.push(Problem {
                location: location.clone(),
                detail: format!(
                    "pack `{}` is not bundled by `ono-adapter`; add it to FIRST_PARTY so the \
                     shell keeps the promise the file makes",
                    pack.id()
                ),
            });
        }
        for problem in ono_adapter::validate(&pack, ono_value::builtin_schemas(), &fixtures)
            .into_iter()
            .chain(ono_adapter::conformance::check_pack(&pack, &fixtures))
        {
            problems.push(Problem {
                location: format!("{location} ({})", problem.location),
                detail: problem.detail,
            });
        }
    }
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

/// The value of a field every command must declare, reporting the omission when it is absent.
///
/// The cross-checks below all used to read `!value.is_empty() && …`, which silently passed any
/// command that left the field out — the one case where the registry says nothing and `help`,
/// completion and the parser have to guess. The field is part of the contract, so its absence is
/// drift like any other (spec §27, ADR-0012).
fn required(
    command: &Yaml,
    field: &str,
    id: &str,
    location: &str,
    problems: &mut Vec<Problem>,
) -> String {
    if let Some(value) = string_at(command, field) {
        return value;
    }
    problems.push(Problem {
        location: location.to_owned(),
        detail: format!(
            "`{id}` declares no `{field}`. Every command declares one (ADR-0012); a command that \
             leaves it out is checked against nothing"
        ),
    });
    String::new()
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
    expression_heads: Option<&BTreeSet<String>>,
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

            let verb = required(command, "verb", &id, &location, problems);
            if !verb.is_empty() && !verbs.contains(&verb) {
                problems.push(Problem {
                    location: location.clone(),
                    detail: format!(
                        "`{id}` uses the verb `{verb}`, which `verbs.yaml` does not define; \
                         spec §7 keeps the verb registry small and curated on purpose"
                    ),
                });
            }
            // A transform operates on whatever the pipeline carries and names no target, which
            // the registry writes as `target: null` (spec §53, ADR-0012). Writing nothing at all
            // is a different thing and is reported: an absent key is a question nobody answered.
            let target = required(command, "target", &id, &location, problems);
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

            let mode = required(command, "argument_mode", &id, &location, problems);
            let should_be_expression = expression_heads.is_some_and(|heads| heads.contains(&verb));
            if !mode.is_empty()
                && expression_heads.is_some()
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

/// The heads `language.yaml` parses in expression mode, or `None` when there is no
/// `language.yaml` to read.
///
/// `argument_modes` is a sequence of modes, each naming itself and the heads it claims
/// (ADR-0009); the expression mode's `heads` is the list ADR-0009 fixes. Reading it as anything
/// else yields an empty set, and every check fed from here then passes whatever it is given.
fn expression_heads(documents: &BTreeMap<PathBuf, Yaml>, path: &Path) -> Option<BTreeSet<String>> {
    let document = documents.get(path)?;
    Some(
        sequence(document, "argument_modes")
            .into_iter()
            .filter(|mode| string_at(mode, "name").as_deref() == Some("expression"))
            .flat_map(|mode| string_sequence(mode, "heads"))
            .collect(),
    )
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

/// Checks that every example a command advertises actually parses.
///
/// Spec §36.5 lists "docs examples stop parsing" as a contract-drift failure, and spec §50 makes
/// executable examples a release requirement for every advertised capability. An example nobody
/// runs is documentation that has quietly become fiction, and this is the cheapest moment to
/// catch it — before anyone has typed it.
#[must_use]
pub fn check_examples(root: &Path) -> Vec<Problem> {
    let directory = root.join("docs").join("spec").join("commands");
    let mut problems = Vec::new();

    for path in yaml_files(&directory) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(document) = serde_yaml_ng::from_str::<Yaml>(&text) else {
            continue;
        };
        let location = relative(root, &path);

        for command in sequence(&document, "commands") {
            let id = string_at(command, "id").unwrap_or_default();
            for example in string_sequence(command, "examples") {
                let parsed = ono_parser::parse(&example);
                if !parsed.has_errors() && parsed.is_complete() {
                    continue;
                }
                let complaint = parsed.diagnostics().first().map_or_else(
                    || "the line is unfinished".to_owned(),
                    |diagnostic| format!("{}: {}", diagnostic.code().code(), diagnostic.message()),
                );
                problems.push(Problem {
                    location: location.clone(),
                    detail: format!("`{id}` documents an example that does not parse — `{example}` — {complaint}"),
                });
            }
        }
    }
    problems
}

/// Checks the spatial registry of spec v0.4 §41 — `docs/spec/spatial/`.
///
/// §41.3 makes these four documents the source of `help spatial`, completion, map legends, SDK
/// enums and conformance tests, and §41's Intent says why: without machine contracts, the
/// renderer, the providers, the parser and the documentation drift into different definitions of
/// the world. This is the check that they agree with each other — every space's parent resolves,
/// every type comes from the one vocabulary, every schema exists, every landmark threshold names
/// a setting the subsystem declares. ADR-0126 puts the files here; ADR-0128 fixes their shape.
///
/// A missing directory is not a failure: registries arrive with the phase that needs them
/// (AGENTS.md §14).
#[must_use]
pub fn check_spatial_registry(root: &Path) -> Vec<Problem> {
    let directory = root.join("docs").join("spec").join("spatial");
    if !directory.is_dir() {
        return Vec::new();
    }
    let mut problems = Vec::new();
    let mut read = |name: &str| -> Option<(String, Yaml)> {
        let path = directory.join(name);
        let location = relative(root, &path);
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_yaml_ng::from_str::<Yaml>(&text) {
                Ok(document) => Some((location, document)),
                // A parse failure is already reported by the generic sweep in `check_contracts`.
                Err(_) => None,
            },
            Err(error) => {
                problems.push(Problem {
                    location,
                    detail: format!(
                        "spec v0.4 §41 requires this registry and it cannot be read: {error}"
                    ),
                });
                None
            }
        }
    };

    let schemas = schema_ids(root);
    let subsystem = read("spatial.yaml");
    let spaces = read("spaces.yaml");
    let relations = read("relations.yaml");
    let landmarks = read("landmarks.yaml");

    let (
        Some((_, subsystem)),
        Some((spaces_at, spaces)),
        Some((relations_at, relations)),
        Some((landmarks_at, landmarks)),
    ) = (subsystem, spaces, relations, landmarks)
    else {
        return problems;
    };

    let types: BTreeSet<String> = subsystem
        .get("object_types")
        .map(|value| {
            string_sequence(value, "aggregates")
                .into_iter()
                .chain(string_sequence(value, "objects"))
                .collect()
        })
        .unwrap_or_default();
    let confidence: BTreeSet<String> = string_sequence(&subsystem, "confidence")
        .into_iter()
        // §41.2's own example spells this one, and it means "exact where the provider observed
        // the edge, the provider's own claim otherwise".
        .chain(std::iter::once("exact_or_provider_declared".to_owned()))
        .collect();
    let directions: BTreeSet<String> = string_sequence(&subsystem, "directions")
        .into_iter()
        .collect();
    let cost_classes: BTreeSet<String> = sequence(&subsystem, "cost_classes")
        .into_iter()
        .filter_map(|entry| string_at(entry, "name"))
        .collect();
    let settings: BTreeMap<String, Yaml> = sequence(&subsystem, "settings")
        .into_iter()
        .filter_map(|entry| Some((string_at(entry, "key")?, entry.clone())))
        .collect();

    problems.extend(check_spaces(&spaces_at, &spaces, &types, &schemas));
    problems.extend(check_relations(
        &relations_at,
        &relations,
        &types,
        &confidence,
        &directions,
        &cost_classes,
    ));
    problems.extend(check_landmarks(&landmarks_at, &landmarks, &settings));
    problems
}

/// Checks the spatial registry against the implementation that serves it.
///
/// The registry-internal checks of [`check_spatial_registry`] say the four documents agree with
/// each other; this one says they agree with `ono-spatial-core`, which is the half that keeps a
/// contract from becoming a description of what someone once intended. It is the same rule
/// `crates/ono-cli/tests/provider_conformance.rs` applies to `docs/spec/providers/`: drift in either
/// direction is a failure (ADR-0126, ADR-0128).
#[must_use]
pub fn check_spatial_implementation(root: &Path) -> Vec<Problem> {
    let directory = root.join("docs").join("spec").join("spatial");
    if !directory.is_dir() {
        return Vec::new();
    }
    let read = |name: &str| -> Option<(String, Yaml)> {
        let path = directory.join(name);
        let text = std::fs::read_to_string(&path).ok()?;
        let document = serde_yaml_ng::from_str::<Yaml>(&text).ok()?;
        Some((relative(root, &path), document))
    };
    let (
        Some((subsystem_at, subsystem)),
        Some((spaces_at, spaces)),
        Some((relations_at, relations)),
    ) = (
        read("spatial.yaml"),
        read("spaces.yaml"),
        read("relations.yaml"),
    )
    else {
        return Vec::new();
    };

    let mut problems = check_vocabularies(&subsystem_at, &subsystem);
    problems.extend(check_geography(&spaces_at, &spaces));
    problems.extend(check_relation_table(&relations_at, &relations));
    problems
}

/// Every schema id declared under `docs/spec/schemas/`.
fn schema_ids(root: &Path) -> BTreeSet<String> {
    yaml_files(&root.join("docs").join("spec").join("schemas"))
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(path).ok()?;
            let document = serde_yaml_ng::from_str::<Yaml>(&text).ok()?;
            string_at(&document, "id")
        })
        .collect()
}

/// Holds `spaces.yaml` and the table in `ono_spatial_core::space` to each other, field for field.
///
/// The canonical geography exists twice — once as the contract §41.1 requires, once as the table
/// the shell navigates — and the whole point of §41 is that those two are one thing. Comparing
/// only the ids would let a space become non-enterable in the code while the registry still
/// promised `enter`, which is exactly the kind of drift that reaches a user as a lie.
fn check_geography(location: &str, document: &Yaml) -> Vec<Problem> {
    let mut problems = Vec::new();
    let declared: BTreeMap<String, &Yaml> = sequence(document, "spaces")
        .into_iter()
        .filter_map(|space| Some((string_at(space, "id")?, space)))
        .collect();
    let implemented: BTreeMap<&str, &ono_spatial_core::CanonicalSpace> = ono_spatial_core::spaces()
        .iter()
        .map(|space| (space.id, space))
        .collect();

    problems.extend(drift(
        location,
        "spaces.yaml",
        &declared.keys().cloned().collect(),
        &implemented.keys().map(|id| (*id).to_owned()).collect(),
    ));

    for (id, space) in &declared {
        let Some(implemented) = implemented.get(id.as_str()) else {
            continue;
        };
        let fields: [(&str, String, String); 6] = [
            (
                "label",
                string_at(space, "label").unwrap_or_default(),
                implemented.label.to_owned(),
            ),
            (
                "parent",
                string_at(space, "parent").unwrap_or_default(),
                implemented.parent.unwrap_or("null").to_owned(),
            ),
            (
                "object_type",
                string_at(space, "object_type").unwrap_or_default(),
                implemented.object_type.as_str().to_owned(),
            ),
            (
                "member_type",
                string_at(space, "member_type").unwrap_or_default(),
                implemented
                    .member_type
                    .map_or_else(|| "null".to_owned(), |kind| kind.as_str().to_owned()),
            ),
            (
                "schema",
                string_at(space, "schema").unwrap_or_default(),
                implemented.schema.unwrap_or("null").to_owned(),
            ),
            (
                "status",
                string_at(space, "status").unwrap_or_else(|| "stable".to_owned()),
                implemented.status.as_str().to_owned(),
            ),
        ];
        for (field, declared, served) in fields {
            if declared != served {
                problems.push(Problem {
                    location: location.to_owned(),
                    detail: format!(
                        "`{id}` declares `{field}: {declared}` and `ono-spatial-core` serves \
                         `{served}`"
                    ),
                });
            }
        }
        if space.get("enterable").and_then(Yaml::as_bool) != Some(implemented.enterable) {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` and `ono-spatial-core` disagree about whether the place is a \
                     destination; `enterable` is what `enter` and completion read (§41.1)"
                ),
            });
        }
        for (field, declared, served) in [
            (
                "commands",
                string_sequence(space, "commands"),
                implemented
                    .commands
                    .iter()
                    .map(|command| (*command).to_owned())
                    .collect::<Vec<_>>(),
            ),
            (
                "summary_fields",
                string_sequence(space, "summary_fields"),
                implemented
                    .summary_fields
                    .iter()
                    .map(|field| (*field).to_owned())
                    .collect::<Vec<_>>(),
            ),
        ] {
            if declared != served {
                problems.push(Problem {
                    location: location.to_owned(),
                    detail: format!(
                        "`{id}` declares `{field}: {declared:?}` and `ono-spatial-core` serves \
                         `{served:?}`"
                    ),
                });
            }
        }
    }
    problems
}

/// Holds `relations.yaml` and `ono_spatial_core::relations()` to each other, field for field.
///
/// A relation is a name a user types (§6.4) and a legend a map draws (§22); both are generated
/// from the registry (§41.3), so a label that differs between the file and the table would put a
/// word in the help that `follow` does not accept.
fn check_relation_table(location: &str, document: &Yaml) -> Vec<Problem> {
    let mut problems = Vec::new();
    let declared: BTreeMap<String, &Yaml> = sequence(document, "relations")
        .into_iter()
        .filter_map(|relation| Some((string_at(relation, "id")?, relation)))
        .collect();
    let implemented: BTreeMap<&str, &ono_spatial_core::RelationSpec> =
        ono_spatial_core::relations()
            .iter()
            .map(|relation| (relation.id, relation))
            .collect();

    problems.extend(drift(
        location,
        "relations.yaml",
        &declared.keys().cloned().collect(),
        &implemented.keys().map(|id| (*id).to_owned()).collect(),
    ));

    for (id, relation) in &declared {
        let Some(implemented) = implemented.get(id.as_str()) else {
            continue;
        };
        let fields: [(&str, String, String); 9] = [
            (
                "source",
                string_at(relation, "source").unwrap_or_default(),
                implemented.source.as_str().to_owned(),
            ),
            (
                "target",
                string_at(relation, "target").unwrap_or_default(),
                implemented.target.as_str().to_owned(),
            ),
            (
                "direction",
                string_at(relation, "direction").unwrap_or_default(),
                implemented.direction.as_str().to_owned(),
            ),
            (
                "canonical_label",
                string_at(relation, "canonical_label").unwrap_or_default(),
                implemented.canonical_label.to_owned(),
            ),
            (
                "inverse_label",
                string_at(relation, "inverse_label").unwrap_or_default(),
                implemented.inverse_label.to_owned(),
            ),
            (
                "canonical_group",
                string_at(relation, "canonical_group").unwrap_or_default(),
                implemented.canonical_group.to_owned(),
            ),
            (
                "inverse_group",
                string_at(relation, "inverse_group").unwrap_or_default(),
                implemented.inverse_group.to_owned(),
            ),
            (
                "confidence",
                string_at(relation, "confidence").unwrap_or_default(),
                implemented.confidence.as_str().to_owned(),
            ),
            (
                "cost_class",
                string_at(relation, "cost_class").unwrap_or_default(),
                implemented.cost_class.as_str().to_owned(),
            ),
        ];
        for (field, declared, served) in fields {
            if declared != served {
                problems.push(Problem {
                    location: location.to_owned(),
                    detail: format!(
                        "`{id}` declares `{field}: {declared}` and `ono-spatial-core` serves \
                         `{served}`"
                    ),
                });
            }
        }
    }
    problems
}

/// The fourteen landmark reasons spec v0.4 §3.7 requires, in the order it lists them.
///
/// "Built-in landmark reasons MUST include" — the set is closed for built-ins, because a reason
/// the renderer cannot name is a highlight with no explanation, and §3.7 requires a landmark to
/// always expose its reason.
const LANDMARK_REASONS: [&str; 14] = [
    "high_cpu",
    "high_memory",
    "failed",
    "restarting",
    "recently_changed",
    "public_listener",
    "privileged",
    "storage_pressure",
    "connection_spike",
    "new_object",
    "removed_object",
    "security_boundary",
    "remote_boundary",
    "user_pinned",
];

fn check_spaces(
    location: &str,
    document: &Yaml,
    types: &BTreeSet<String>,
    schemas: &BTreeSet<String>,
) -> Vec<Problem> {
    let mut problems = Vec::new();
    let spaces = sequence(document, "spaces");
    let ids: BTreeSet<String> = spaces
        .iter()
        .filter_map(|space| string_at(space, "id"))
        .collect();
    let mut seen = BTreeSet::new();
    let mut roots = Vec::new();

    for space in spaces {
        let Some(id) = string_at(space, "id") else {
            problems.push(Problem {
                location: location.to_owned(),
                detail: "a space has no `id` (spec v0.4 §41.1)".to_owned(),
            });
            continue;
        };
        if !seen.insert(id.clone()) {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!("`{id}` is declared twice; space ids identify a place"),
            });
        }
        for required in ["label", "object_type", "commands", "summary_fields"] {
            if space.get(required).is_none_or(Yaml::is_null) {
                problems.push(Problem {
                    location: location.to_owned(),
                    detail: format!("`{id}` declares no `{required}` (spec v0.4 §41.1)"),
                });
            }
        }
        if space.get("enterable").and_then(Yaml::as_bool).is_none() {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` must declare `enterable` as a boolean; it is what tells `enter` and \
                     completion whether the place is a destination (spec v0.4 §41.1)"
                ),
            });
        }
        if !string_sequence(space, "commands")
            .iter()
            .any(|c| c == "look")
        {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` must list `look` among its commands; `look` describes the place a \
                     user stands in, so every place supports it (spec v0.4 §6.1)"
                ),
            });
        }
        match space.get("parent") {
            None => problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` declares no `parent`; the root's is null, but the field is not \
                     optional (spec v0.4 §41.1)"
                ),
            }),
            Some(parent) if parent.is_null() => roots.push(id.clone()),
            Some(parent) => {
                let parent = parent.as_str().unwrap_or_default();
                if !ids.contains(parent) {
                    problems.push(Problem {
                        location: location.to_owned(),
                        detail: format!(
                            "`{id}` names `{parent}` as its canonical parent and no space with \
                             that id is declared; a dangling parent makes `up` \
                             non-deterministic (spec v0.4 §11.3)"
                        ),
                    });
                }
            }
        }
        for field in ["object_type", "member_type"] {
            if let Some(name) = space.get(field).and_then(Yaml::as_str)
                && !types.contains(name)
            {
                problems.push(Problem {
                    location: location.to_owned(),
                    detail: format!(
                        "`{id}` declares `{field}: {name}`, which is not in the `object_types` \
                         vocabulary of spatial.yaml"
                    ),
                });
            }
        }
        if let Some(schema) = space.get("schema").and_then(Yaml::as_str)
            && !schemas.contains(schema)
        {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` is built from `{schema}` and no schema with that id is declared \
                     under docs/spec/schemas/"
                ),
            });
        }
    }

    if roots.len() != 1 {
        problems.push(Problem {
            location: location.to_owned(),
            detail: format!(
                "exactly one space has no parent — the root every session starts at (spec v0.4 \
                 §7.1, §46.1); found {roots:?}"
            ),
        });
    }
    problems
}

fn check_relations(
    location: &str,
    document: &Yaml,
    types: &BTreeSet<String>,
    confidence: &BTreeSet<String>,
    directions: &BTreeSet<String>,
    cost_classes: &BTreeSet<String>,
) -> Vec<Problem> {
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();

    for relation in sequence(document, "relations") {
        let Some(id) = string_at(relation, "id") else {
            problems.push(Problem {
                location: location.to_owned(),
                detail: "a relation has no `id` (spec v0.4 §41.2)".to_owned(),
            });
            continue;
        };
        if !seen.insert(id.clone()) {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!("`{id}` is declared twice; relation ids are unique"),
            });
        }
        for required in [
            "source",
            "target",
            "direction",
            "canonical_label",
            "inverse_label",
            "canonical_group",
            "inverse_group",
            "confidence",
        ] {
            if relation.get(required).is_none_or(Yaml::is_null) {
                problems.push(Problem {
                    location: location.to_owned(),
                    detail: format!("`{id}` declares no `{required}` (spec v0.4 §41.2)"),
                });
            }
        }
        for field in ["source", "target"] {
            if let Some(name) = relation.get(field).and_then(Yaml::as_str)
                && !types.contains(name)
            {
                problems.push(Problem {
                    location: location.to_owned(),
                    detail: format!(
                        "`{id}` connects `{field}: {name}`, which is not in the `object_types` \
                         vocabulary of spatial.yaml; §42.3 forbids an edge end that resolves to \
                         nothing"
                    ),
                });
            }
        }
        if let Some(direction) = relation.get("direction").and_then(Yaml::as_str)
            && !directions.contains(direction)
        {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` declares `direction: {direction}`, which is not one of the \
                     directions spatial.yaml fixes (spec v0.4 §41.2, §22)"
                ),
            });
        }
        if let Some(value) = relation.get("confidence").and_then(Yaml::as_str)
            && !confidence.contains(value)
        {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` declares `confidence: {value}`, which is not one of the values spec \
                     v0.4 §11.5 fixes"
                ),
            });
        }
        if let Some(class) = relation.get("cost_class").and_then(Yaml::as_str)
            && !cost_classes.contains(class)
        {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` declares `cost_class: {class}`, which is not one of the classes spec \
                     v0.4 §32.1 fixes"
                ),
            });
        }
    }
    problems
}

fn check_landmarks(
    location: &str,
    document: &Yaml,
    settings: &BTreeMap<String, Yaml>,
) -> Vec<Problem> {
    let mut problems = Vec::new();
    let declared: BTreeSet<String> = sequence(document, "landmarks")
        .into_iter()
        .filter_map(|entry| string_at(entry, "reason"))
        .collect();

    for required in LANDMARK_REASONS {
        if !declared.contains(required) {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{required}` is one of the built-in landmark reasons spec v0.4 §3.7 \
                     requires and the registry does not declare it"
                ),
            });
        }
    }
    for reason in &declared {
        if !LANDMARK_REASONS.contains(&reason.as_str()) {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{reason}` is not one of the fourteen built-in reasons of spec v0.4 §3.7; a \
                     further reason comes from a KUANG/11 package and identifies its source \
                     (§26.5) rather than joining the built-ins"
                ),
            });
        }
    }

    for entry in sequence(document, "landmarks") {
        let reason = string_at(entry, "reason").unwrap_or_default();
        let Some(threshold) = entry.get("threshold").filter(|value| !value.is_null()) else {
            continue;
        };
        for required in ["metric", "comparison", "default", "setting"] {
            if threshold.get(required).is_none_or(Yaml::is_null) {
                problems.push(Problem {
                    location: location.to_owned(),
                    detail: format!(
                        "the threshold of `{reason}` declares no `{required}`; spec v0.4 §26.3 \
                         requires thresholds to be inspectable and configurable"
                    ),
                });
            }
        }
        let Some(key) = threshold.get("setting").and_then(Yaml::as_str) else {
            continue;
        };
        match settings.get(key) {
            None => problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "the threshold of `{reason}` is configured by `{key}`, which spatial.yaml \
                     does not declare as a setting (spec v0.4 §26.3, §47)"
                ),
            }),
            Some(setting) => {
                let declared = setting.get("default");
                let used = threshold.get("default");
                if declared != used {
                    problems.push(Problem {
                        location: location.to_owned(),
                        detail: format!(
                            "the threshold of `{reason}` defaults to {used:?} and `{key}` \
                             defaults to {declared:?}; one threshold cannot have two defaults"
                        ),
                    });
                }
            }
        }
    }
    problems
}

/// Checks the closed vocabularies of `spatial.yaml` against the types that implement them.
///
/// This is the drift check that `docs/spec/providers/` gets from
/// `crates/ono-cli/tests/provider_conformance.rs`, in the direction the spatial registry needs
/// it: a name the registry knows and the implementation does not is a space, relation or
/// landmark nothing can serve, and a name the implementation knows and the registry does not is
/// undocumented surface. §41.3 generates help, completion, map legends and SDK enums from these
/// lists, so the two cannot be allowed to disagree (ADR-0126, ADR-0128).
fn check_vocabularies(location: &str, subsystem: &Yaml) -> Vec<Problem> {
    use ono_spatial_core::neighborhood::{Completeness, Freshness, PermissionState};
    use ono_spatial_core::{
        Confidence, CostClass, Direction, IdentityTier, LandmarkReason, Movement, ScopeKind,
        SpatialType,
    };

    let mut problems = Vec::new();
    let object_types: BTreeSet<String> = subsystem
        .get("object_types")
        .map(|value| {
            string_sequence(value, "aggregates")
                .into_iter()
                .chain(string_sequence(value, "objects"))
                .collect()
        })
        .unwrap_or_default();

    let vocabularies: [(&str, BTreeSet<String>, Vec<&'static str>); 10] = [
        (
            "object_types",
            object_types,
            SpatialType::ALL.iter().map(|v| v.as_str()).collect(),
        ),
        (
            "identity_tiers",
            vocabulary(subsystem, "identity_tiers"),
            IdentityTier::ALL.iter().map(|v| v.as_str()).collect(),
        ),
        (
            "scope_kinds",
            vocabulary(subsystem, "scope_kinds"),
            ScopeKind::ALL.iter().map(|v| v.as_str()).collect(),
        ),
        (
            "movements",
            vocabulary(subsystem, "movements"),
            Movement::ALL.iter().map(|v| v.as_str()).collect(),
        ),
        (
            "confidence",
            vocabulary(subsystem, "confidence"),
            Confidence::ALL.iter().map(|v| v.as_str()).collect(),
        ),
        (
            "directions",
            vocabulary(subsystem, "directions"),
            Direction::ALL.iter().map(|v| v.as_str()).collect(),
        ),
        (
            "permission_states",
            vocabulary(subsystem, "permission_states"),
            PermissionState::ALL.iter().map(|v| v.as_str()).collect(),
        ),
        (
            "freshness_states",
            vocabulary(subsystem, "freshness_states"),
            Freshness::ALL.iter().map(|v| v.as_str()).collect(),
        ),
        (
            "completeness",
            vocabulary(subsystem, "completeness"),
            Completeness::ALL.iter().map(|v| v.as_str()).collect(),
        ),
        (
            "cost_classes",
            vocabulary(subsystem, "cost_classes"),
            CostClass::ALL.iter().map(|v| v.as_str()).collect(),
        ),
    ];

    for (what, declared, implemented) in vocabularies {
        let implemented: BTreeSet<String> =
            implemented.into_iter().map(ToOwned::to_owned).collect();
        problems.extend(drift(location, what, &declared, &implemented));
    }

    // The geography and the relation vocabulary themselves, not only the names they are built
    // from: this is what makes `docs/spec/spatial/spaces.yaml` and the table in
    // `ono_spatial_core::space` one thing rather than two that happen to agree today.
    problems.extend(drift(
        location,
        "landmarks.yaml reasons",
        &LANDMARK_REASONS
            .iter()
            .map(|reason| (*reason).to_owned())
            .collect(),
        &LandmarkReason::ALL
            .iter()
            .map(|reason| reason.as_str().to_owned())
            .collect(),
    ));

    problems
}

/// Both directions of drift between a declaration and an implementation.
fn drift(
    location: &str,
    what: &str,
    declared: &BTreeSet<String>,
    implemented: &BTreeSet<String>,
) -> Vec<Problem> {
    let mut problems = Vec::new();
    for name in declared.difference(implemented) {
        problems.push(Problem {
            location: location.to_owned(),
            detail: format!(
                "`{what}` declares `{name}` and `ono-spatial-core` does not implement it; a \
                 declared name nothing serves is a promise nobody keeps"
            ),
        });
    }
    for name in implemented.difference(declared) {
        problems.push(Problem {
            location: location.to_owned(),
            detail: format!(
                "`ono-spatial-core` implements `{name}` and `{what}` does not declare it; a \
                 served name no file declares is undocumented surface"
            ),
        });
    }
    problems
}

/// The names one vocabulary declares in `spatial.yaml`, whether it spells them as a plain list or
/// as a list of documented entries.
fn vocabulary(document: &Yaml, key: &str) -> BTreeSet<String> {
    let flat: BTreeSet<String> = string_sequence(document, key).into_iter().collect();
    if !flat.is_empty() {
        return flat;
    }
    sequence(document, key)
        .into_iter()
        .filter_map(|entry| string_at(entry, "name").or_else(|| string_at(entry, "id")))
        .collect()
}

/// Checks the §42 spatial claims every provider that feeds the spatial index must declare.
///
/// §42 lists eight required claims and gives no vocabulary for most of them; ADR-0132 fixes the
/// shape and this function enforces it. The checks that matter are the ones a reader cannot make
/// by eye:
///
/// - a provider may claim a **weaker** identity tier than the types it serves allow, never a
///   stronger one — a claim is a promise about every object the provider exposes, so the ceiling
///   is the weakest of them (§10.1, §42.1);
/// - the declared canonical-parent chain is exactly the chain `ono_spatial_core::parent_rules`
///   and the geography implement, so `up` cannot mean one thing in the contract and another in
///   the shell (§11.3);
/// - every relation named is one `relations.yaml` declares and one that actually touches a type
///   the provider serves (§32, §42.3);
/// - denied information has somewhere to go: a provider that can be refused declares
///   `permission_denied` or `unknown`, because §42.4 forbids a false empty collection;
/// - every landmark metric is a field of a schema the provider declares (§26, §42).
#[must_use]
pub fn check_provider_claims(root: &Path) -> Vec<Problem> {
    let providers = root.join("docs").join("spec").join("providers");
    if !providers.is_dir() {
        return Vec::new();
    }
    let mut problems = Vec::new();

    let subsystem = std::fs::read_to_string(
        root.join("docs")
            .join("spec")
            .join("spatial")
            .join("spatial.yaml"),
    )
    .ok()
    .and_then(|text| serde_yaml_ng::from_str::<Yaml>(&text).ok());
    let Some(subsystem) = subsystem else {
        return problems;
    };
    let claims_block = subsystem
        .get("provider_claims")
        .cloned()
        .unwrap_or_default();
    let required = string_sequence(&claims_block, "required");
    let freshness: BTreeSet<String> = vocabulary(&claims_block, "freshness");
    let relations: BTreeSet<String> = ono_spatial_core::relations()
        .iter()
        .map(|relation| relation.id.to_owned())
        .collect();
    let fields = schema_fields(root);

    for path in yaml_files(&providers) {
        let location = relative(root, &path);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(document) = serde_yaml_ng::from_str::<Yaml>(&text) else {
            continue;
        };
        for provider in sequence(&document, "providers") {
            let id = string_at(provider, "id").unwrap_or_default();
            let served: BTreeSet<ono_spatial_core::SpatialType> =
                string_sequence(provider, "targets")
                    .iter()
                    .flat_map(|target| ono_spatial_core::types_of_target(target))
                    .copied()
                    .collect();
            let Some(claims) = provider.get("spatial") else {
                if !served.is_empty() {
                    problems.push(Problem {
                        location: location.clone(),
                        detail: format!(
                            "`{id}` serves the spatial object types {}, so spec v0.4 §42 requires \
                             a `spatial:` block with its eight claims",
                            names(&served)
                        ),
                    });
                }
                continue;
            };
            if served.is_empty() {
                problems.push(Problem {
                    location: location.clone(),
                    detail: format!(
                        "`{id}` declares §42 spatial claims but none of its targets names a \
                         spatial object type, so nothing it serves reaches the spatial index"
                    ),
                });
                continue;
            }
            for key in &required {
                if claims.get(key.as_str()).is_none() {
                    problems.push(Problem {
                        location: location.clone(),
                        detail: format!("`{id}` declares no `{key}` among its §42 spatial claims"),
                    });
                }
            }
            problems.extend(check_identity_claim(&location, &id, claims, &served));
            problems.extend(check_parent_claim(&location, &id, claims, &served));
            problems.extend(check_relation_claim(
                &location, &id, claims, &served, &relations,
            ));
            problems.extend(check_state_claims(&location, &id, claims, &freshness));
            problems.extend(check_metric_claim(
                &location, &id, claims, provider, &fields,
            ));
        }
    }
    problems
}

/// The spatial types, as a reader-friendly list.
fn names(types: &BTreeSet<ono_spatial_core::SpatialType>) -> String {
    types
        .iter()
        .map(|kind| kind.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// §42.1: the claimed tier may be weaker than every served type allows, never stronger.
fn check_identity_claim(
    location: &str,
    id: &str,
    claims: &Yaml,
    served: &BTreeSet<ono_spatial_core::SpatialType>,
) -> Vec<Problem> {
    use ono_spatial_core::IdentityTier;
    let Some(text) = string_at(claims, "identity_strategy") else {
        return Vec::new();
    };
    let Some(claimed) = IdentityTier::from_name(&text) else {
        return vec![Problem {
            location: location.to_owned(),
            detail: format!(
                "`{id}` claims the identity strategy `{text}`, which is not one of the three \
                 tiers of spec v0.4 §10.1"
            ),
        }];
    };
    // `IdentityTier` is ordered strongest first, so the weakest ceiling is the largest value.
    let Some(ceiling) = served.iter().map(|kind| kind.identity_tier()).max() else {
        return Vec::new();
    };
    if claimed < ceiling {
        return vec![Problem {
            location: location.to_owned(),
            detail: format!(
                "`{id}` claims `{claimed}` identity, but it serves {} and the weakest of those \
                 allows only `{ceiling}`. A provider may claim a weaker identity than its types \
                 allow, never a stronger one (spec v0.4 §10.1, §42.1)",
                names(served)
            ),
        }];
    }
    Vec::new()
}

/// §11.3: the declared chain `up` follows is the chain the shell follows.
fn check_parent_claim(
    location: &str,
    id: &str,
    claims: &Yaml,
    served: &BTreeSet<ono_spatial_core::SpatialType>,
) -> Vec<Problem> {
    let mut problems = Vec::new();
    let Some(declared) = claims.get("canonical_parent") else {
        return problems;
    };
    let entries: BTreeMap<String, Vec<String>> = declared
        .as_mapping()
        .map(|mapping| {
            mapping
                .iter()
                .filter_map(|(key, value)| {
                    let key = key.as_str()?.to_owned();
                    let chain = value
                        .as_sequence()
                        .map(|list| {
                            list.iter()
                                .filter_map(|entry| entry.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default();
                    Some((key, chain))
                })
                .collect()
        })
        .unwrap_or_default();

    let expected: BTreeMap<String, Vec<String>> = served
        .iter()
        .map(|kind| (kind.as_str().to_owned(), parent_chain(*kind)))
        .collect();

    for (kind, chain) in &expected {
        match entries.get(kind) {
            Some(declared) if declared == chain => {}
            Some(declared) => problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` declares the canonical parent of a {kind} as {declared:?}; `up` \
                     follows {chain:?} (spec v0.4 §11.3)"
                ),
            }),
            None => problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` serves {kind} objects but declares no canonical parent for them \
                     (spec v0.4 §42)"
                ),
            }),
        }
    }
    for kind in entries.keys() {
        if !expected.contains_key(kind) {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` declares a canonical parent for {kind}, which none of its targets \
                     names (spec v0.4 §42)"
                ),
            });
        }
    }
    problems
}

/// The chain `up` follows for `kind`: its canonical-parent relations, then the collection space
/// it falls back to (§11.3).
fn parent_chain(kind: ono_spatial_core::SpatialType) -> Vec<String> {
    ono_spatial_core::parent_rules(kind)
        .iter()
        .map(|rule| rule.relation.to_owned())
        .chain(ono_spatial_core::space::collection_for(kind).map(|space| space.id.to_owned()))
        .collect()
}

/// §32, §42.3: a declared relation exists and touches a type the provider serves.
fn check_relation_claim(
    location: &str,
    id: &str,
    claims: &Yaml,
    served: &BTreeSet<ono_spatial_core::SpatialType>,
    relations: &BTreeSet<String>,
) -> Vec<Problem> {
    let mut problems = Vec::new();
    for name in string_sequence(claims, "relationships") {
        if !relations.contains(&name) {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` claims the relation `{name}`, which \
                     docs/spec/spatial/relations.yaml does not declare"
                ),
            });
            continue;
        }
        let Some(spec) = ono_spatial_core::relation::spec(&name) else {
            continue;
        };
        if !served
            .iter()
            .any(|kind| kind.is_a(spec.source) || kind.is_a(spec.target))
        {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` claims the relation `{name}`, which runs between {} and {}; it \
                     serves neither (spec v0.4 §42.3)",
                    spec.source, spec.target
                ),
            });
        }
    }
    problems
}

/// §35.2, §42.4, §32.1: the state, freshness and cost vocabularies.
fn check_state_claims(
    location: &str,
    id: &str,
    claims: &Yaml,
    freshness: &BTreeSet<String>,
) -> Vec<Problem> {
    use ono_spatial_core::{CostClass, neighborhood::PermissionState};
    let mut problems = Vec::new();

    if let Some(declared) = string_at(claims, "freshness")
        && !freshness.contains(&declared)
    {
        problems.push(Problem {
            location: location.to_owned(),
            detail: format!(
                "`{id}` claims the freshness strategy `{declared}`, which \
                 docs/spec/spatial/spatial.yaml does not declare"
            ),
        });
    }
    if let Some(declared) = string_at(claims, "cost_class")
        && CostClass::from_name(&declared).is_none()
    {
        problems.push(Problem {
            location: location.to_owned(),
            detail: format!(
                "`{id}` claims the cost class `{declared}`, which is not one of the classes of \
                 spec v0.4 §32.1"
            ),
        });
    }
    if claims.get("events").is_some_and(|value| !value.is_bool()) {
        problems.push(Problem {
            location: location.to_owned(),
            detail: format!("`{id}` must answer `events` with a boolean (spec v0.4 §42)"),
        });
    }

    let states = string_sequence(claims, "permissions");
    if states.is_empty() {
        problems.push(Problem {
            location: location.to_owned(),
            detail: format!(
                "`{id}` names no permission state it can report; §42.4 requires denied \
                 information to have somewhere to go other than an empty collection"
            ),
        });
    }
    for state in &states {
        if PermissionState::from_name(state).is_none() {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` claims the permission state `{state}`, which is not one of the six \
                     of spec v0.4 §35.2"
                ),
            });
        }
    }
    problems
}

/// §26, §42: a landmark metric is a field of a schema the provider declares.
fn check_metric_claim(
    location: &str,
    id: &str,
    claims: &Yaml,
    provider: &Yaml,
    fields: &BTreeMap<String, BTreeSet<String>>,
) -> Vec<Problem> {
    let mut problems = Vec::new();
    let known: BTreeSet<&String> = string_sequence(provider, "schemas")
        .iter()
        .filter_map(|schema| fields.get(schema))
        .flatten()
        .collect();
    let metrics = string_sequence(claims, "landmark_metrics");
    if metrics.is_empty() {
        problems.push(Problem {
            location: location.to_owned(),
            detail: format!(
                "`{id}` names no landmark-relevant metric; §42 requires the claim, and §26 \
                 builds every landmark rule from one"
            ),
        });
    }
    for metric in &metrics {
        if !known.contains(metric) {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` claims the landmark metric `{metric}`, which is not a field of any \
                     schema it declares"
                ),
            });
        }
    }
    problems
}

/// The field names of every schema under `docs/spec/schemas/`, by schema id.
fn schema_fields(root: &Path) -> BTreeMap<String, BTreeSet<String>> {
    yaml_files(&root.join("docs").join("spec").join("schemas"))
        .into_iter()
        .filter_map(|path| {
            let text = std::fs::read_to_string(path).ok()?;
            let document = serde_yaml_ng::from_str::<Yaml>(&text).ok()?;
            Some((
                string_at(&document, "id")?,
                mapping_keys(&document, "fields"),
            ))
        })
        .collect()
}

// --- KUANG/11 contracts ↔ the runtime that serves them (spec §36.5, §31.7) ---------------------

/// A minimal `kuang-package/1` manifest, into which one section is probed.
///
/// It carries exactly the sections spec §31.7 makes mandatory, so a probe of any other section
/// fails on the probed key and on nothing else.
const PROBE_MANIFEST: &str = "\
format: kuang-package/1
package:
  id: dev.example.probe
  name: probe
  version: 0.1.0
  description: A manifest the contract check probes with.
  publisher: dev.example
  license: MIT
compatibility:
  kuang_api: \">=11.1 <12\"
  ono_language: \">=0.2\"
  platforms: [linux-amd64]
roles: [provider]
network:
  outbound: none
";

/// A name no manifest section declares, used to make the parser list the ones it accepts.
const PROBE_KEY: &str = "zz-probe-key";

/// Holds `docs/spec/kuang/` against `crates/ono-kuang-*` (spec §36.5).
///
/// The seven KUANG/11 contracts are the public surface of the extension runtime, and until this
/// existed they reached `spec-check` only through the generic sweep — which proves they are
/// non-empty valid YAML and nothing else. Every other registry under `docs/spec/` is held against
/// the code that serves it; this is that check for the last one.
///
/// Four of the seven can be compared exactly today: the capability families, the error taxonomy,
/// the lifecycle states and the manifest's closed sections. `contributions.v1.yaml`,
/// `protocol.v1.yaml` and `assistants.v1.yaml` describe surfaces this build implements in part;
/// checking them is later work rather than a check that would pass by not looking.
#[must_use]
pub fn check_kuang_contracts(root: &Path) -> Vec<Problem> {
    let directory = root.join("docs").join("spec").join("kuang");
    if !directory.is_dir() {
        return Vec::new();
    }
    let read = |name: &str| -> Option<(String, Yaml)> {
        let path = directory.join(name);
        let text = std::fs::read_to_string(&path).ok()?;
        let document = serde_yaml_ng::from_str::<Yaml>(&text).ok()?;
        Some((relative(root, &path), document))
    };
    let mut problems = Vec::new();
    if let Some((location, document)) = read("capabilities.v1.yaml") {
        problems.extend(check_kuang_capabilities(&location, &document));
    }
    if let Some((location, document)) = read("errors.v1.yaml") {
        problems.extend(check_kuang_errors(&location, &document));
    }
    if let Some((location, document)) = read("lifecycle.v1.yaml") {
        problems.extend(check_kuang_lifecycle(&location, &document));
    }
    if let Some((location, document)) = read("manifest.v1.yaml") {
        problems.extend(check_kuang_manifest(&location, &document));
    }
    problems
}

/// §31.16's families, their risk, elevation and scope keys, against `Capability`.
fn check_kuang_capabilities(location: &str, document: &Yaml) -> Vec<Problem> {
    use ono_kuang_protocol::Capability;

    let mut problems = Vec::new();
    let mut declared = BTreeSet::new();
    for family in sequence(document, "families") {
        let Some(id) = string_at(family, "id") else {
            continue;
        };
        declared.insert(id.clone());
        let Some(capability) = Capability::from_id(&id) else {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` is declared as a capability family and `ono_kuang_protocol::Capability` \
                     has no such variant"
                ),
            });
            continue;
        };
        let kebab = |text: &str| text.to_lowercase();
        if let Some(risk) = string_at(family, "risk")
            && kebab(&format!("{:?}", capability.risk())) != risk
        {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` declares risk `{risk}` and the runtime carries `{}`",
                    kebab(&format!("{:?}", capability.risk()))
                ),
            });
        }
        if let Some(elevation) = string_at(family, "elevation")
            && kebab(&format!("{:?}", capability.elevation())) != elevation
        {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` declares elevation `{elevation}` and the runtime carries `{}`",
                    kebab(&format!("{:?}", capability.elevation()))
                ),
            });
        }
        let served: BTreeSet<&str> = capability.scope_keys().iter().map(|key| key.name).collect();
        let scope = family
            .get("scope")
            .and_then(Yaml::as_mapping)
            .map(|mapping| {
                mapping
                    .iter()
                    .filter_map(|(key, value)| key.as_str().map(|key| (key, value)))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for (key, shape) in &scope {
            let Some(declared_key) = capability
                .scope_keys()
                .iter()
                .find(|declared| declared.name == *key)
            else {
                problems.push(Problem {
                    location: location.to_owned(),
                    detail: format!(
                        "`{id}` declares the scope key `{key}` and the runtime's family has none"
                    ),
                });
                continue;
            };
            if let Some(enforcement) = string_at(shape, "enforcement")
                && format!("{:?}", declared_key.enforcement).to_lowercase() != enforcement
            {
                problems.push(Problem {
                    location: location.to_owned(),
                    detail: format!(
                        "`{id}.{key}` declares enforcement `{enforcement}` and the runtime \
                         enforces it as `{}`",
                        format!("{:?}", declared_key.enforcement).to_lowercase()
                    ),
                });
            }
        }
        for key in served {
            if !scope.iter().any(|(declared, _)| *declared == key) {
                problems.push(Problem {
                    location: location.to_owned(),
                    detail: format!(
                        "the runtime scopes `{id}` by `{key}` and the contract declares no such key"
                    ),
                });
            }
        }
    }
    for capability in Capability::ALL {
        if !declared.contains(capability.id()) {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "the runtime carries the capability family `{}` and the contract declares it \
                     nowhere",
                    capability.id()
                ),
            });
        }
    }
    problems
}

/// §31.79's taxonomy against `KuangErrorCode`.
fn check_kuang_errors(location: &str, document: &Yaml) -> Vec<Problem> {
    use ono_kuang_protocol::KuangErrorCode;

    let mut problems = Vec::new();
    let mut declared = BTreeSet::new();
    for entry in sequence(document, "errors") {
        let (Some(code), Some(name)) = (string_at(entry, "code"), string_at(entry, "name")) else {
            continue;
        };
        declared.insert(name.clone());
        let Some(known) = KuangErrorCode::from_name(&name) else {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!("`{name}` is declared and `KuangErrorCode` has no such condition"),
            });
            continue;
        };
        if known.code() != code {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{name}` is declared as `{code}` and the runtime renders it `{}`",
                    known.code()
                ),
            });
        }
        if let Some(kind) = string_at(entry, "kind")
            && format!("{:?}", known.kind()).to_lowercase() != kind
        {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{name}` is declared kind `{kind}` and the runtime classifies it `{}`",
                    format!("{:?}", known.kind()).to_lowercase()
                ),
            });
        }
    }
    for code in KuangErrorCode::ALL {
        if !declared.contains(code.name()) {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "the runtime raises `{}` and the contract declares it nowhere",
                    code.name()
                ),
            });
        }
    }
    problems
}

/// §31.8's states against `PluginState`.
fn check_kuang_lifecycle(location: &str, document: &Yaml) -> Vec<Problem> {
    use ono_kuang_protocol::PluginState;

    let mut problems = Vec::new();
    let mut declared: BTreeSet<String> = BTreeSet::new();
    for state in sequence(document, "states") {
        let Some(id) = string_at(state, "id") else {
            continue;
        };
        declared.insert(id.clone());
        let Some(known) = PluginState::ALL.iter().find(|known| known.as_str() == id) else {
            continue;
        };
        if let Some(Yaml::Bool(ran)) = state.get("code_has_run")
            && *ran != known.code_has_run()
        {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{id}` is declared with `code_has_run: {ran}` and the runtime answers `{}` \
                     (spec §31.8)",
                    known.code_has_run()
                ),
            });
        }
    }
    let served: BTreeSet<String> = PluginState::ALL
        .iter()
        .map(|state| state.as_str().to_owned())
        .collect();
    for state in declared.difference(&served) {
        problems.push(Problem {
            location: location.to_owned(),
            detail: format!("`{state}` is a declared package state and `PluginState` has none"),
        });
    }
    for state in served.difference(&declared) {
        problems.push(Problem {
            location: location.to_owned(),
            detail: format!("`PluginState` carries `{state}` and the contract declares it nowhere"),
        });
    }
    problems
}

/// §31.7's closed sections against the fields the package parser accepts.
///
/// The parser is the authority on its own field names, and it is asked rather than mirrored: a
/// section probed with a key nothing declares answers with the list of keys it does accept, so
/// the comparison needs no second copy of the manifest shape to go stale.
fn check_kuang_manifest(location: &str, document: &Yaml) -> Vec<Problem> {
    let mut problems = Vec::new();
    for section in sequence(document, "sections") {
        let Some(name) = string_at(section, "name") else {
            continue;
        };
        let declared: BTreeSet<String> = section
            .get("fields")
            .and_then(Yaml::as_mapping)
            .map(|mapping| {
                mapping
                    .keys()
                    .filter_map(|key| key.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        if declared.is_empty() {
            continue;
        }
        let Some(accepted) = manifest_section_fields(&name) else {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "the contract declares `{name}` as a closed section and the package parser \
                     does not model it, so an unknown key in it cannot fail closed (spec §31.7)"
                ),
            });
            continue;
        };
        for field in declared.difference(&accepted) {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "`{name}.{field}` is declared and the package parser refuses it as an unknown \
                     field"
                ),
            });
        }
        for field in accepted.difference(&declared) {
            problems.push(Problem {
                location: location.to_owned(),
                detail: format!(
                    "the package parser accepts `{name}.{field}` and the contract declares it \
                     nowhere"
                ),
            });
        }
    }
    problems
}

/// Which keys the package parser accepts in `section`, asked of the parser itself.
///
/// `None` when the section is not a closed struct — the parser then has no list to give, which
/// is itself the finding above.
fn manifest_section_fields(section: &str) -> Option<BTreeSet<String>> {
    let mut manifest: Yaml = serde_yaml_ng::from_str(PROBE_MANIFEST).ok()?;
    let mapping = manifest.as_mapping_mut()?;
    let key = Yaml::String(section.to_owned());
    let entry = mapping
        .entry(key)
        .or_insert_with(|| Yaml::Mapping(serde_yaml_ng::Mapping::new()));
    if !entry.is_mapping() {
        *entry = Yaml::Mapping(serde_yaml_ng::Mapping::new());
    }
    entry
        .as_mapping_mut()?
        .insert(Yaml::String(PROBE_KEY.to_owned()), Yaml::Number(1.into()));
    let document = serde_yaml_ng::to_string(&manifest).ok()?;
    let error = ono_kuang_protocol::Manifest::parse(&document).err()?;
    let message = error.message();
    // serde writes "expected one of `a`, `b`" for three or more and "expected `a` or `b`" for
    // two; the backticked names are the same either way.
    let listed = message.split(", expected ").nth(1)?;
    let fields: BTreeSet<String> = listed
        .split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect();
    (!fields.is_empty()).then_some(fields)
}
