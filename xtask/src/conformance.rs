//! Generating the provider conformance suite from the registries (spec §35.3).
//!
//! "Every provider capability gets a generated conformance suite from registry metadata."
//! `docs/spec/providers/*.yaml` says what each provider advertises — its targets, its
//! capabilities, the schemas it emits, its identity strategy and how a bare snapshot of each
//! target behaves. `docs/spec/schemas/*.v1.yaml` fixes the shape of every record. This module
//! turns the two into `crates/ono-cli/tests/provider_conformance.rs`, so that nothing a provider
//! advertises can go unexercised: a target with no declared exercise, a capability that reaches
//! neither a snapshot nor a command, an undeclared schema — each stops generation rather than
//! producing a suite with a hole in it.
//!
//! The generated file is committed and the gate compares it with what the registries produce, the
//! same way `docs/reference/` is checked (spec §36.2, `docs/ACCEPTANCE.md` §4.5).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use serde_yaml_ng::Value as Yaml;

pub use crate::reference::{GenerateError, Page};
pub use crate::scan::Problem;

/// Where the generated suite lives.
const SUITE: &str = "crates/ono-cli/tests/provider_conformance.rs";

/// The exercises a declaration may ask for.
const EXERCISES: [&str; 3] = ["enumerable", "selector_required", "unbounded"];

/// One provider entry, as `docs/spec/providers/*.yaml` declares it.
struct Declaration {
    id: String,
    doc: String,
    targets: Vec<String>,
    capabilities: Vec<String>,
    schemas: Vec<String>,
    exercises: Vec<(String, String)>,
    identity_strategy: Option<String>,
    /// The Rust identifier fragment this entry's tests are named after.
    ident: String,
}

/// One schema, as `docs/spec/schemas/*.v1.yaml` fixes it.
struct SchemaDecl {
    identity: Vec<String>,
    identity_fallback: Vec<String>,
    default_view: Vec<String>,
    fields: Vec<FieldDecl>,
}

struct FieldDecl {
    name: String,
    ty: String,
    required: bool,
    nullable: bool,
    unit: Option<String>,
}

/// Generates the conformance suite from the registries under `root/docs/spec`.
///
/// # Errors
///
/// Returns a [`GenerateError`] when a registry is missing or unreadable, or when a declaration
/// would leave something a provider advertises unexercised.
pub fn generate(root: &Path) -> Result<Vec<Page>, GenerateError> {
    let spec = root.join("docs").join("spec");
    let capabilities = read_capabilities(&spec)?;
    let schemas = read_schemas(&spec)?;
    let commands = read_commands(&spec)?;
    let declarations = read_declarations(&spec)?;

    let mut body = String::new();
    let mut accounts: Vec<String> = Vec::new();
    let mut registry_rows: Vec<String> = Vec::new();

    for declaration in &declarations {
        registry_rows.push(format!(
            "        (\"{}\", &[{}]),\n",
            declaration.id,
            quoted(&declaration.targets)
        ));
        write_surface(&mut body, declaration, &capabilities)?;
        for schema in &declaration.schemas {
            let Some(decl) = schemas.get(schema) else {
                return Err(GenerateError {
                    detail: format!(
                        "`{}` declares the schema `{schema}`, and docs/spec/schemas/ defines no \
                         such id",
                        declaration.id
                    ),
                });
            };
            write_schema_contract(&mut body, declaration, schema, decl);
        }
        for (target, exercise) in &declaration.exercises {
            write_target_case(&mut body, declaration, target, exercise);
        }
        for capability in &declaration.capabilities {
            accounts.push(account(declaration, capability, &capabilities, &commands)?);
        }
    }

    let mut contents = String::new();
    contents.push_str(HEADER);
    contents.push_str(
        "#[rustfmt::skip]\n#[tokio::test]\nasync fn \
                       should_register_exactly_the_providers_the_declarations_name() {\n    \
                       harness::assert_registry(&[\n",
    );
    for row in &registry_rows {
        contents.push_str(row);
    }
    contents.push_str("    ]).await;\n}\n\n");
    contents.push_str(&body);
    contents.push_str(
        "#[rustfmt::skip]\n#[tokio::test]\nasync fn \
         should_account_for_every_capability_the_declarations_name() {\n    \
         harness::assert_accounts(&[\n",
    );
    for row in &accounts {
        contents.push_str(row);
    }
    contents.push_str("    ]).await;\n}\n");

    Ok(vec![Page {
        path: SUITE.to_owned(),
        contents,
    }])
}

/// Writes the generated suite into the tree.
///
/// # Errors
///
/// Returns a [`GenerateError`] when generation fails or the file cannot be written.
pub fn write(root: &Path) -> Result<Vec<String>, GenerateError> {
    let mut written = Vec::new();
    for page in generate(root)? {
        let path = root.join(&page.path);
        std::fs::write(&path, &page.contents).map_err(|error| GenerateError {
            detail: format!("cannot write {}: {error}", path.display()),
        })?;
        written.push(page.path);
    }
    Ok(written)
}

/// Reports whether the committed suite is what the registries produce.
#[must_use]
pub fn check_committed(root: &Path) -> Vec<Problem> {
    let generated = match generate(root) {
        Ok(pages) => pages,
        Err(error) => {
            return vec![Problem {
                location: SUITE.to_owned(),
                detail: format!("cannot be generated: {}", error.detail),
            }];
        }
    };

    let mut problems = Vec::new();
    for page in generated {
        let path = root.join(&page.path);
        match std::fs::read_to_string(&path) {
            Ok(committed) if committed == page.contents => {}
            Ok(_) => problems.push(Problem {
                location: page.path.clone(),
                detail: "does not match what docs/spec/providers/ and docs/spec/schemas/ \
                         produce; run `cargo xtask conformance` (spec §35.3). If the difference \
                         is deliberate, the declaration is where it belongs, not the suite"
                    .to_owned(),
            }),
            Err(_) => problems.push(Problem {
                location: page.path.clone(),
                detail: "is missing; run `cargo xtask conformance`".to_owned(),
            }),
        }
    }
    problems
}

const HEADER: &str = "\
//! The provider conformance suite of spec §35.3, generated from `docs/spec/providers/*.yaml`
//! and `docs/spec/schemas/*.v1.yaml` by `cargo xtask conformance`.
//!
//! Do not edit by hand: your changes will be overwritten and the gate will fail. What a provider
//! advertises is declared in the registry; this file is that declaration turned into questions
//! the running providers have to answer.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = \"a test states its preconditions directly (AGENTS.md §16)\"
)]

mod conformance_harness;

use conformance_harness as harness;

";

/// The surface case: what the registry advertises against what the file declares.
fn write_surface(
    body: &mut String,
    declaration: &Declaration,
    capabilities: &BTreeMap<String, (String, String)>,
) -> Result<(), GenerateError> {
    let mut claims = String::new();
    for capability in &declaration.capabilities {
        let Some((risk, elevation)) = capabilities.get(capability) else {
            return Err(GenerateError {
                detail: format!(
                    "`{}` declares the capability `{capability}`, which \
                     docs/spec/capabilities.yaml does not define",
                    declaration.id
                ),
            });
        };
        let _ = writeln!(
            claims,
            "            harness::CapabilityClaim {{ id: \"{capability}\", risk: \"{risk}\", elevation: \"{elevation}\" }},"
        );
    }
    let _ = writeln!(body, "/// {}", one_line(&declaration.doc));
    let _ = writeln!(body, "#[rustfmt::skip]");
    let _ = writeln!(body, "#[tokio::test]");
    let _ = writeln!(
        body,
        "async fn should_advertise_exactly_what_{}_declares() {{",
        declaration.ident
    );
    let _ = writeln!(body, "    harness::assert_surface(&harness::Surface {{");
    let _ = writeln!(body, "        provider: \"{}\",", declaration.id);
    let _ = writeln!(
        body,
        "        targets: &[{}],",
        quoted(&declaration.targets)
    );
    let _ = writeln!(body, "        capabilities: &[");
    body.push_str(&claims);
    let _ = writeln!(body, "        ],");
    let _ = writeln!(
        body,
        "        schemas: &[{}],",
        quoted(&declaration.schemas)
    );
    let _ = writeln!(body, "    }}).await;");
    let _ = writeln!(body, "}}\n");
    Ok(())
}

/// The schema case: the shape the registry fixes, against the schema the provider carries.
fn write_schema_contract(
    body: &mut String,
    declaration: &Declaration,
    schema: &str,
    decl: &SchemaDecl,
) {
    let _ = writeln!(body, "#[rustfmt::skip]");
    let _ = writeln!(body, "#[tokio::test]");
    let _ = writeln!(
        body,
        "async fn should_shape_{}_the_way_{}_declares_it() {{",
        sanitise(schema),
        declaration.ident
    );
    let _ = writeln!(
        body,
        "    harness::assert_schema_contract(&harness::SchemaContract {{"
    );
    let _ = writeln!(body, "        provider: \"{}\",", declaration.id);
    let _ = writeln!(
        body,
        "        targets: &[{}],",
        quoted(&declaration.targets)
    );
    let _ = writeln!(body, "        schema: \"{schema}\",");
    let _ = writeln!(body, "        identity: &[{}],", quoted(&decl.identity));
    let _ = writeln!(
        body,
        "        identity_fallback: &[{}],",
        quoted(&decl.identity_fallback)
    );
    let _ = writeln!(
        body,
        "        default_view: &[{}],",
        quoted(&decl.default_view)
    );
    let _ = writeln!(body, "        fields: &[");
    for field in &decl.fields {
        let unit = field
            .unit
            .as_ref()
            .map_or_else(|| "None".to_owned(), |unit| format!("Some(\"{unit}\")"));
        let _ = writeln!(
            body,
            "            harness::FieldContract {{ name: \"{name}\", ty: \"{ty}\", required: {required}, nullable: {nullable}, unit: {unit} }},",
            name = field.name,
            ty = field.ty,
            required = field.required,
            nullable = field.nullable,
        );
    }
    let _ = writeln!(body, "        ],");
    let _ = writeln!(body, "    }}).await;");
    let _ = writeln!(body, "}}\n");
}

/// The target case: what a bare snapshot of the target must do.
fn write_target_case(body: &mut String, declaration: &Declaration, target: &str, exercise: &str) {
    let variant = match exercise {
        "enumerable" => "Enumerable",
        "selector_required" => "SelectorRequired",
        _ => "Unbounded",
    };
    let strategy = declaration
        .identity_strategy
        .as_ref()
        .map_or_else(|| "None".to_owned(), |name| format!("Some(\"{name}\")"));
    let _ = writeln!(body, "#[rustfmt::skip]");
    let _ = writeln!(body, "#[tokio::test]");
    let _ = writeln!(
        body,
        "async fn should_answer_for_{}_within_its_contract_when_{}_is_asked() {{",
        sanitise(target),
        declaration.ident
    );
    let _ = writeln!(
        body,
        "    harness::assert_target_conforms(&harness::TargetCase {{"
    );
    let _ = writeln!(body, "        provider: \"{}\",", declaration.id);
    let _ = writeln!(
        body,
        "        targets: &[{}],",
        quoted(&declaration.targets)
    );
    let _ = writeln!(body, "        target: \"{target}\",");
    let _ = writeln!(body, "        exercise: harness::Exercise::{variant},");
    let _ = writeln!(
        body,
        "        schemas: &[{}],",
        quoted(&declaration.schemas)
    );
    let _ = writeln!(body, "        identity_strategy: {strategy},");
    let _ = writeln!(body, "    }}).await;");
    let _ = writeln!(body, "}}\n");
}

/// How one declared capability is exercised, or why generation cannot say.
fn account(
    declaration: &Declaration,
    capability: &str,
    capabilities: &BTreeMap<String, (String, String)>,
    commands: &BTreeMap<String, Vec<String>>,
) -> Result<String, GenerateError> {
    let Some((risk, _)) = capabilities.get(capability) else {
        return Err(GenerateError {
            detail: format!(
                "`{}` declares the capability `{capability}`, which docs/spec/capabilities.yaml \
                 does not define",
                declaration.id
            ),
        });
    };
    let target = capability.split('.').next().unwrap_or_default();
    let through = if risk == "read" && declaration.targets.iter().any(|name| name == target) {
        format!("harness::Through::Snapshot(\"{target}\")")
    } else if let Some(ids) = commands.get(capability).filter(|ids| !ids.is_empty()) {
        format!("harness::Through::Command(&[{}])", quoted(ids))
    } else {
        return Err(GenerateError {
            detail: format!(
                "`{}` declares `{capability}` ({risk}), and nothing would exercise it: it reads \
                 no target this provider serves, and no command in docs/spec/commands/ names it. \
                 A capability nothing reaches is surface nobody keeps (spec §35.3)",
                declaration.id
            ),
        });
    };
    Ok(format!(
        "        harness::Account {{ provider: \"{id}\", targets: &[{targets}], capability: \
         \"{capability}\", risk: \"{risk}\", through: {through} }},\n",
        id = declaration.id,
        targets = quoted(&declaration.targets),
    ))
}

fn read_declarations(spec: &Path) -> Result<Vec<Declaration>, GenerateError> {
    let directory = spec.join("providers");
    let mut declarations: Vec<Declaration> = Vec::new();
    for path in yaml_files(&directory)? {
        let document = load(&path)?;
        for provider in sequence(&document, "providers") {
            let id = string_at(provider, "id").unwrap_or_default();
            let targets = string_sequence(provider, "targets");
            let Some(exercises) = provider.get("conformance") else {
                return Err(GenerateError {
                    detail: format!(
                        "`{id}` declares no `conformance:` block, so nothing says how a snapshot \
                         of {} behaves and the suite of spec §35.3 cannot exercise it",
                        targets.join(", ")
                    ),
                });
            };
            let exercises = declared_exercises(&id, &targets, exercises)?;
            declarations.push(Declaration {
                ident: String::new(),
                exercises,
                identity_strategy: provider
                    .get("spatial")
                    .and_then(|spatial| string_at(spatial, "identity_strategy")),
                doc: string_at(provider, "doc").unwrap_or_default(),
                capabilities: string_sequence(provider, "capabilities"),
                schemas: string_sequence(provider, "schemas"),
                targets,
                id,
            });
        }
    }

    // Three netlink providers share the id `linux.netlink`; the targets they serve are what tell
    // them apart, so a repeated id borrows them for its test names.
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for declaration in &declarations {
        *seen.entry(declaration.id.as_str()).or_default() += 1;
    }
    let repeated: BTreeSet<String> = seen
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(id, _)| id.to_owned())
        .collect();
    for declaration in &mut declarations {
        declaration.ident = if repeated.contains(&declaration.id) {
            format!(
                "{}_serving_{}",
                sanitise(&declaration.id),
                declaration
                    .targets
                    .iter()
                    .map(|target| sanitise(target))
                    .collect::<Vec<_>>()
                    .join("_")
            )
        } else {
            sanitise(&declaration.id)
        };
    }
    Ok(declarations)
}

fn declared_exercises(
    id: &str,
    targets: &[String],
    block: &Yaml,
) -> Result<Vec<(String, String)>, GenerateError> {
    let Some(mapping) = block.as_mapping() else {
        return Err(GenerateError {
            detail: format!("`{id}` declares a `conformance:` block that is not a mapping"),
        });
    };
    let mut exercises: Vec<(String, String)> = Vec::new();
    for (key, value) in mapping {
        let target = key.as_str().unwrap_or_default().to_owned();
        let exercise = value.as_str().unwrap_or_default().to_owned();
        if !targets.contains(&target) {
            return Err(GenerateError {
                detail: format!(
                    "`{id}` declares an exercise for `{target}`, which is not a target it serves"
                ),
            });
        }
        if !EXERCISES.contains(&exercise.as_str()) {
            return Err(GenerateError {
                detail: format!(
                    "`{id}` asks for the exercise `{exercise}` on `{target}`; the suite knows {}",
                    EXERCISES.join(", ")
                ),
            });
        }
        exercises.push((target, exercise));
    }
    for target in targets {
        if !exercises.iter().any(|(name, _)| name == target) {
            return Err(GenerateError {
                detail: format!(
                    "`{id}` serves `{target}` and declares no exercise for it, so the generated \
                     suite would leave a target the provider advertises untouched (spec §35.3)"
                ),
            });
        }
    }
    Ok(exercises)
}

fn read_capabilities(spec: &Path) -> Result<BTreeMap<String, (String, String)>, GenerateError> {
    let document = load(&spec.join("capabilities.yaml"))?;
    let mut capabilities = BTreeMap::new();
    for entry in sequence(&document, "provider_capabilities") {
        let Some(id) = string_at(entry, "id") else {
            continue;
        };
        capabilities.insert(
            id,
            (
                string_at(entry, "risk").unwrap_or_default(),
                string_at(entry, "elevation").unwrap_or_default(),
            ),
        );
    }
    Ok(capabilities)
}

fn read_commands(spec: &Path) -> Result<BTreeMap<String, Vec<String>>, GenerateError> {
    let mut commands: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let directory = spec.join("commands");
    if !directory.is_dir() {
        return Ok(commands);
    }
    for path in yaml_files(&directory)? {
        let document = load(&path)?;
        for command in sequence(&document, "commands") {
            let (Some(id), Some(capability)) = (
                string_at(command, "id"),
                string_at(command, "provider_capability"),
            ) else {
                continue;
            };
            commands.entry(capability).or_default().push(id);
        }
    }
    for ids in commands.values_mut() {
        ids.sort();
    }
    Ok(commands)
}

fn read_schemas(spec: &Path) -> Result<BTreeMap<String, SchemaDecl>, GenerateError> {
    let mut schemas = BTreeMap::new();
    let directory = spec.join("schemas");
    if !directory.is_dir() {
        return Ok(schemas);
    }
    for path in yaml_files(&directory)? {
        let document = load(&path)?;
        let Some(id) = string_at(&document, "id") else {
            continue;
        };
        let mut fields = Vec::new();
        for (name, field) in mapping_entries(&document, "fields") {
            let declared = string_at(field, "type").unwrap_or_default();
            fields.push(FieldDecl {
                ty: type_name(&declared, field).map_err(|error| GenerateError {
                    detail: format!("{}: {}", path.display(), error.detail),
                })?,
                required: field
                    .get("required")
                    .and_then(Yaml::as_bool)
                    .unwrap_or(false),
                nullable: field
                    .get("nullable")
                    .and_then(Yaml::as_bool)
                    .unwrap_or(false),
                unit: unit_name(string_at(field, "unit").as_deref()),
                name,
            });
        }
        schemas.insert(
            id,
            SchemaDecl {
                identity: string_sequence(&document, "identity"),
                identity_fallback: string_sequence(&document, "identity_fallback"),
                default_view: document
                    .get("default_view")
                    .map(|view| string_sequence(view, "columns"))
                    .unwrap_or_default(),
                fields,
            },
        );
    }
    Ok(schemas)
}

/// The type name `ono_value::FieldType::name` produces for one declared type.
///
/// This is the type vocabulary of spec §10.2 read a second time — `ono-value` reads it too, to
/// build the schema a provider carries. Two readings held together by the generated suite is the
/// point: a divergence between them is exactly the drift spec §36.5 asks the gate to catch.
fn type_name(declared: &str, field: &Yaml) -> Result<String, GenerateError> {
    if declared == "enum" {
        let variants = string_sequence(field, "values");
        if variants.is_empty() {
            return Err(GenerateError {
                detail: "an `enum` field declares no `values`".to_owned(),
            });
        }
        return Ok(format!("enum<{}>", variants.join("|")));
    }
    if let Some(inner) = declared
        .strip_prefix("list<")
        .and_then(|rest| rest.strip_suffix('>'))
    {
        return Ok(format!("list<{}>", type_name(inner, &Yaml::Null)?));
    }
    if let Some(target) = declared
        .strip_prefix("ref<")
        .and_then(|rest| rest.strip_suffix('>'))
    {
        return Ok(format!("ref<{target}>"));
    }
    // A structured error is its own value, never a record that describes one (spec §25).
    if declared == "ono.error/1" || declared == "error" {
        return Ok("error".to_owned());
    }
    // Any other bare schema id means the record itself, as against `ref<…>`, which carries only
    // the identity.
    if declared.contains('/') {
        return Ok(format!("record<{declared}>"));
    }
    Ok(match declared {
        "any" | "value" => "any".to_owned(),
        "map" | "record" => "map".to_owned(),
        "bool" | "int" | "float" | "decimal" | "string" | "bytes" | "path" | "timestamp"
        | "duration" | "bytesize" | "percent" | "regex" | "uuid" | "ip" | "ipnetwork" | "port" => {
            declared.to_owned()
        }
        other => {
            return Err(GenerateError {
                detail: format!("`{other}` is not one of the types spec §10.2 defines"),
            });
        }
    })
}

/// The unit `ono_value` keeps for this declaration, which is `None` for a word it does not model.
fn unit_name(declared: Option<&str>) -> Option<String> {
    match declared {
        Some("percent") => Some("percent".to_owned()),
        Some("bytes") => Some("bytes".to_owned()),
        Some("seconds") => Some("seconds".to_owned()),
        Some("count") => Some("count".to_owned()),
        _ => None,
    }
}

fn quoted(values: &[String]) -> String {
    values
        .iter()
        .map(|value| format!("\"{value}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A Rust identifier fragment for a registry id such as `ono.process/1` or `linux.sock-diag`.
fn sanitise(name: &str) -> String {
    let mut ident = String::new();
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            ident.push(character.to_ascii_lowercase());
        } else if !ident.ends_with('_') {
            ident.push('_');
        }
    }
    ident.trim_matches('_').to_owned()
}

/// A doc comment is one line, whatever the YAML's folding did to it.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn load(path: &Path) -> Result<Yaml, GenerateError> {
    let text = std::fs::read_to_string(path).map_err(|error| GenerateError {
        detail: format!("cannot read {}: {error}", path.display()),
    })?;
    serde_yaml_ng::from_str(&text).map_err(|error| GenerateError {
        detail: format!("{} is not valid YAML: {error}", path.display()),
    })
}

fn yaml_files(directory: &Path) -> Result<Vec<std::path::PathBuf>, GenerateError> {
    let entries = std::fs::read_dir(directory).map_err(|error| GenerateError {
        detail: format!("cannot read {}: {error}", directory.display()),
    })?;
    let mut files: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|suffix| suffix == "yaml"))
        .collect();
    files.sort();
    Ok(files)
}

fn sequence<'a>(document: &'a Yaml, key: &str) -> Vec<&'a Yaml> {
    document
        .get(key)
        .and_then(Yaml::as_sequence)
        .map(|entries| entries.iter().collect())
        .unwrap_or_default()
}

fn string_at(document: &Yaml, key: &str) -> Option<String> {
    document
        .get(key)
        .and_then(Yaml::as_str)
        .map(|text| text.trim().to_owned())
}

fn string_sequence(document: &Yaml, key: &str) -> Vec<String> {
    sequence(document, key)
        .into_iter()
        .filter_map(|entry| entry.as_str().map(|text| text.trim().to_owned()))
        .collect()
}

fn mapping_entries<'a>(document: &'a Yaml, key: &str) -> Vec<(String, &'a Yaml)> {
    document
        .get(key)
        .and_then(Yaml::as_mapping)
        .map(|mapping| {
            mapping
                .iter()
                .filter_map(|(name, value)| name.as_str().map(|name| (name.to_owned(), value)))
                .collect()
        })
        .unwrap_or_default()
}
