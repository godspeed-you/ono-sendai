//! The command half of the conformance suite: every command's declared examples, run through the
//! real binary and held to the output the command declares (issue #149, ADR-0935).
//!
//! `docs/contracts/commands/*.yaml` says what each command produces — `output:` — and shows how it
//! is used — `examples:`. Nothing compared the two: the v0.2 §11.3 pre-flight check trusts the
//! declaration, and `spec-check` compares the registry with declarations only, so `timeline`
//! answered with a record of another schema for as long as it existed. This module turns the
//! examples into executable evidence, written into `crates/ono-cli/tests/command_conformance.rs`
//! beside the provider suite and checked by the same `check_committed`.
//!
//! An example runs only when its contracts say it is safe to run in a hermetic scratch shell:
//! every command it names is declared, reads rather than changes (`verbs.yaml`), needs no
//! privilege, uses no capability that mutates, observes a live source or reaches the network
//! (`capabilities.yaml`), does not follow a source that never ends, writes nothing through a
//! redirection, refers to no earlier session's results, and declares no output schema a later
//! phase still owes (`schemas/deferred.yaml`). Everything else is skipped with the contract's own
//! reason. What the contracts cannot see — an example that names an object a fresh environment does
//! not have, or a command that runs only as a statement of its own — is listed in
//! `docs/contracts/conformance/command_examples.yaml` with a reason, and an entry that names no
//! documented example, or one the contracts already skip, is refused as stale.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

use ono_parser::ast::{
    Argument, CurrentSelector, Expr, ParenInner, Pipeline, Stage, StageHead, Statement, StrPart,
};
use serde_yaml_ng::Value as Yaml;

use crate::reference::{GenerateError, Page};

/// Where the generated command suite lives.
pub const SUITE: &str = "crates/ono-cli/tests/command_conformance.rs";

/// The register of examples the contracts would run and the environment cannot.
pub const EXEMPTIONS: &str = "docs/contracts/conformance/command_examples.yaml";

/// One declared command, as much of it as the classification reads.
struct Command {
    id: String,
    verb: String,
    target: Option<String>,
    output: String,
    privilege: String,
    capability: Option<String>,
    examples: Vec<String>,
}

/// What `capabilities.yaml` says about one provider capability.
struct Capability {
    risk: String,
    network: bool,
}

/// What `verbs.yaml` says about one verb.
struct Verb {
    mutating: bool,
    role: String,
}

/// Everything the classification reads.
struct Contracts {
    commands: Vec<Command>,
    by_spelling: BTreeMap<(String, Option<String>), usize>,
    verbs: BTreeMap<String, Verb>,
    capabilities: BTreeMap<String, Capability>,
    deferred: BTreeSet<String>,
}

/// What happens to one example.
enum Verdict {
    /// It runs, and its values are held to the declaration.
    Run,
    /// The contracts say it cannot run hermetically, and why.
    Skipped(String),
    /// The exemption register says so, and why.
    Exempt(String),
}

/// Generates the command conformance suite.
///
/// # Errors
///
/// Returns a [`GenerateError`] when a registry cannot be read, or when the exemption register
/// names an example no command documents, one the contracts already skip, or one twice.
pub fn generate(root: &Path) -> Result<Page, GenerateError> {
    let spec = root.join("docs").join("contracts");
    let contracts = read_contracts(&spec)?;
    let exemptions = read_exemptions(root)?;

    let mut used: BTreeSet<(String, String)> = BTreeSet::new();
    let mut tests = String::new();
    let mut not_run: Vec<(String, String, String)> = Vec::new();
    let mut unexercised: Vec<(String, String)> = Vec::new();
    let mut executed = 0usize;
    for command in &contracts.commands {
        let mut ran_one = false;
        let mut last_reason = String::new();
        for (index, example) in command.examples.iter().enumerate() {
            let key = (command.id.clone(), example.clone());
            let verdict = match contract_reason(&contracts, example) {
                Some(reason) => {
                    if exemptions.contains_key(&key) {
                        return Err(GenerateError {
                            detail: format!(
                                "{EXEMPTIONS} exempts `{example}` of `{}`, which its contracts \
                                 already skip — {reason}. A stale exemption hides the day the \
                                 example becomes runnable; remove it",
                                command.id
                            ),
                        });
                    }
                    Verdict::Skipped(reason)
                }
                None => match exemptions.get(&key) {
                    Some(reason) => {
                        used.insert(key.clone());
                        Verdict::Exempt(reason.clone())
                    }
                    None => Verdict::Run,
                },
            };
            match verdict {
                Verdict::Run => {
                    ran_one = true;
                    executed += 1;
                    write_case(&mut tests, command, index + 1, example);
                }
                Verdict::Skipped(reason) => {
                    last_reason.clone_from(&reason);
                    not_run.push((command.id.clone(), example.clone(), reason));
                }
                Verdict::Exempt(reason) => {
                    last_reason = format!("exempt: {reason}");
                    not_run.push((command.id.clone(), example.clone(), last_reason.clone()));
                }
            }
        }
        if !ran_one {
            let reason = if command.examples.is_empty() {
                "documents no example".to_owned()
            } else {
                last_reason
            };
            unexercised.push((command.id.clone(), reason));
        }
    }
    for key in exemptions.keys() {
        if !used.contains(key) {
            return Err(GenerateError {
                detail: format!(
                    "{EXEMPTIONS} exempts `{}` of `{}`, and no command documents that example. \
                     A stale exemption is a reason nobody can check; remove it",
                    key.1, key.0
                ),
            });
        }
    }

    let mut contents = String::new();
    contents.push_str(HEADER);
    let _ = writeln!(
        contents,
        "//! {executed} examples run; {} are not run, each for the reason listed at the end of this \
         file.",
        not_run.len()
    );
    let _ = writeln!(contents, "//!");
    let _ = writeln!(
        contents,
        "//! Commands no example of which runs ({}):",
        unexercised.len()
    );
    let _ = writeln!(contents, "//!");
    for (id, reason) in &unexercised {
        let _ = writeln!(contents, "//! - `{id}` — {}", one_line(reason));
    }
    contents.push_str(PRELUDE);
    contents.push_str(&tests);
    let _ = writeln!(
        contents,
        "// Not run, with the reason the contracts or {EXEMPTIONS} give:"
    );
    let _ = writeln!(contents, "//");
    for (id, example, reason) in &not_run {
        let _ = writeln!(
            contents,
            "// - `{id}` `{}` — {}",
            one_line(example),
            one_line(reason)
        );
    }
    Ok(Page {
        path: SUITE.to_owned(),
        contents,
    })
}

const HEADER: &str = "\
//! The command conformance suite of issue #149: every documented example a command's contracts
//! let run hermetically, run through the real `ono` in a scratch environment, with every value it
//! produces held to the command's declared `output` (ADR-0935). Generated from
//! `docs/contracts/commands/*.yaml`, `verbs.yaml`, `capabilities.yaml`, `schemas/deferred.yaml`
//! and `conformance/command_examples.yaml` by `cargo xtask conformance`.
//!
//! Do not edit by hand: your changes will be overwritten and the gate will fail. An example that
//! cannot run here is either skipped by its contracts or exempted in
//! `docs/contracts/conformance/command_examples.yaml`, with the reason.
//!
";

const PRELUDE: &str = "
// The examples are the full product's. The core build of #127 answers the rest as unavailable,
// so this suite is the full build's (ADR-0910, ADR-0925).
#![cfg(feature = \"full\")]
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = \"a test states its preconditions directly (AGENTS.md §16)\"
)]

mod conformance_harness;

use conformance_harness as harness;

";

/// One generated case.
fn write_case(body: &mut String, command: &Command, index: usize, example: &str) {
    let _ = writeln!(body, "/// `{}`", one_line(example));
    let _ = writeln!(body, "#[rustfmt::skip]");
    let _ = writeln!(body, "#[test]");
    let _ = writeln!(
        body,
        "fn should_produce_what_{}_declares_when_example_{index}_runs() {{",
        sanitise(&command.id)
    );
    let _ = writeln!(
        body,
        "    harness::assert_example_conforms(&harness::ExampleCase {{"
    );
    let _ = writeln!(body, "        command: {:?},", command.id);
    let _ = writeln!(body, "        example: {example:?},");
    let _ = writeln!(body, "        output: {:?},", command.output);
    let _ = writeln!(body, "    }});");
    let _ = writeln!(body, "}}\n");
}

/// Why the contracts say `example` cannot run hermetically, or `None` when they let it run.
fn contract_reason(contracts: &Contracts, example: &str) -> Option<String> {
    let parsed = ono_parser::parse(example);
    if parsed.has_errors() || !parsed.is_complete() {
        return Some("does not parse, which `spec-check` reports on its own".to_owned());
    }
    let mut reasons = Vec::new();
    for statement in &parsed.program().statements {
        statement_reasons(contracts, statement, &mut reasons);
    }
    reasons.into_iter().next()
}

fn statement_reasons(contracts: &Contracts, statement: &Statement, reasons: &mut Vec<String>) {
    match statement {
        Statement::Pipeline(pipeline) => pipeline_reasons(contracts, pipeline, reasons),
        Statement::Let(binding) => pipeline_reasons(contracts, &binding.value, reasons),
        _ => reasons.push(
            "is a statement other than a pipeline, which the classification does not read"
                .to_owned(),
        ),
    }
}

fn pipeline_reasons(contracts: &Contracts, pipeline: &Pipeline, reasons: &mut Vec<String>) {
    let lists = std::iter::once(&pipeline.head).chain(pipeline.tail.iter().map(|tail| &tail.list));
    for list in lists {
        for stage in &list.stages {
            stage_reasons(contracts, stage, reasons);
        }
    }
}

fn stage_reasons(contracts: &Contracts, stage: &Stage, reasons: &mut Vec<String>) {
    if !stage.redirections.is_empty() {
        reasons.push("writes through a redirection".to_owned());
    }
    match &stage.head {
        StageHead::Command(name) => {
            let target = stage.arguments.iter().find_map(|argument| match argument {
                Argument::Word(word) => Some(word.text.clone()),
                _ => None,
            });
            match (
                name.namespace.as_ref(),
                resolve(contracts, &name.name, target),
            ) {
                (None, Some(command)) => command_reasons(contracts, command, reasons),
                _ => reasons.push(format!(
                    "runs `{}`, which no command contract declares — an external program or a \
                     shell keyword, whose effects nothing states",
                    name.name
                )),
            }
        }
        StageHead::Value(expr) => expr_reasons(contracts, expr, reasons),
        StageHead::Error(_) => reasons.push("has a stage that does not parse".to_owned()),
    }
    for argument in &stage.arguments {
        match argument {
            Argument::Word(word) => {
                if word.text.starts_with('@') {
                    reasons.push(format!(
                        "refers to `{}`, a result or event of a session the scratch shell does \
                         not have",
                        word.text
                    ));
                }
            }
            Argument::Option(option) => {
                if let Some(value) = &option.value {
                    expr_reasons(contracts, value, reasons);
                }
            }
            Argument::Value(expr) => expr_reasons(contracts, expr, reasons),
            Argument::Error(_) => reasons.push("has an argument that does not parse".to_owned()),
        }
    }
}

fn expr_reasons(contracts: &Contracts, expr: &Expr, reasons: &mut Vec<String>) {
    match expr {
        Expr::CurrentValue(current) => {
            if matches!(
                current.selector,
                CurrentSelector::Previous(_) | CurrentSelector::Item(_)
            ) {
                reasons.push(
                    "refers to an earlier result, which the scratch shell does not have".to_owned(),
                );
            }
        }
        Expr::Str(literal) => {
            for part in &literal.parts {
                if let StrPart::Expr(inner) = part {
                    expr_reasons(contracts, inner, reasons);
                }
            }
        }
        Expr::List(list) => {
            for item in &list.items {
                expr_reasons(contracts, item, reasons);
            }
        }
        Expr::Record(record) => {
            for field in &record.fields {
                expr_reasons(contracts, &field.value, reasons);
            }
        }
        Expr::Block(block) => {
            for statement in &block.statements {
                statement_reasons(contracts, statement, reasons);
            }
        }
        Expr::Paren(paren) => match &paren.inner {
            ParenInner::Pipeline(pipeline) => pipeline_reasons(contracts, pipeline, reasons),
            ParenInner::Expr(inner) => expr_reasons(contracts, inner, reasons),
        },
        Expr::Unary(unary) => expr_reasons(contracts, &unary.operand, reasons),
        Expr::Binary(binary) => {
            expr_reasons(contracts, &binary.lhs, reasons);
            expr_reasons(contracts, &binary.rhs, reasons);
        }
        Expr::Field(access) => expr_reasons(contracts, &access.base, reasons),
        Expr::Index(index) => {
            expr_reasons(contracts, &index.base, reasons);
            expr_reasons(contracts, &index.index, reasons);
        }
        Expr::Call(call) => {
            expr_reasons(contracts, &call.callee, reasons);
            for argument in &call.arguments {
                expr_reasons(contracts, argument, reasons);
            }
        }
        _ => {}
    }
}

/// The command a stage head names: `verb target` where a contract declares that pair, else the
/// verb on its own.
fn resolve<'c>(
    contracts: &'c Contracts,
    verb: &str,
    target: Option<String>,
) -> Option<&'c Command> {
    let index = target
        .and_then(|target| {
            contracts
                .by_spelling
                .get(&(verb.to_owned(), Some(target)))
                .copied()
        })
        .or_else(|| contracts.by_spelling.get(&(verb.to_owned(), None)).copied())?;
    contracts.commands.get(index)
}

/// Why one command a stage names cannot run hermetically, by its contracts alone.
fn command_reasons(contracts: &Contracts, command: &Command, reasons: &mut Vec<String>) {
    match contracts.verbs.get(&command.verb) {
        None => reasons.push(format!(
            "uses the verb `{}`, which verbs.yaml does not declare",
            command.verb
        )),
        Some(verb) if verb.mutating => reasons.push(format!(
            "runs `{}`, whose verb `{}` changes the system (verbs.yaml)",
            command.id, command.verb
        )),
        Some(verb) if verb.role == "stream producer" => reasons.push(format!(
            "runs `{}`, whose verb `{}` follows a live source that does not end (verbs.yaml)",
            command.id, command.verb
        )),
        Some(_) => {}
    }
    if command.privilege != "none" {
        reasons.push(format!(
            "runs `{}`, which declares privilege `{}`",
            command.id, command.privilege
        ));
    }
    if let Some(id) = &command.capability {
        match contracts.capabilities.get(id) {
            None => reasons.push(format!(
                "runs `{}`, whose capability `{id}` capabilities.yaml does not declare",
                command.id
            )),
            Some(capability) => {
                if capability.risk != "read" {
                    reasons.push(format!(
                        "runs `{}`, whose capability `{id}` is `{}` rather than `read`",
                        command.id, capability.risk
                    ));
                }
                if capability.network {
                    reasons.push(format!(
                        "runs `{}`, whose capability `{id}` reaches the network",
                        command.id
                    ));
                }
            }
        }
    }
    for schema in schema_ids(&command.output) {
        if contracts.deferred.contains(&schema) {
            reasons.push(format!(
                "runs `{}`, which declares `{schema}` — a schema schemas/deferred.yaml says a later \
                 phase writes",
                command.id
            ));
        }
    }
}

/// Every `ono.<name>/<version>` schema id a declared type names.
fn schema_ids(declared: &str) -> Vec<String> {
    declared
        .split(|character: char| !(character.is_ascii_alphanumeric() || "./-_".contains(character)))
        .filter(|token| token.contains('/') && token.contains('.'))
        .map(str::to_owned)
        .collect()
}

fn read_contracts(spec: &Path) -> Result<Contracts, GenerateError> {
    let mut commands = Vec::new();
    let directory = spec.join("commands");
    if directory.is_dir() {
        for path in yaml_files(&directory)? {
            let document = load(&path)?;
            for command in sequence(&document, "commands") {
                commands.push(Command {
                    id: string_at(command, "id").unwrap_or_default(),
                    verb: string_at(command, "verb").unwrap_or_default(),
                    target: string_at(command, "target").filter(|target| target != "null"),
                    output: string_at(command, "output").unwrap_or_default(),
                    privilege: string_at(command, "privilege").unwrap_or_default(),
                    capability: string_at(command, "provider_capability")
                        .filter(|capability| capability != "null"),
                    examples: string_sequence(command, "examples"),
                });
            }
        }
    }
    let by_spelling = commands
        .iter()
        .enumerate()
        .map(|(index, command)| ((command.verb.clone(), command.target.clone()), index))
        .collect();

    let mut verbs = BTreeMap::new();
    if let Some(document) = load_optional(&spec.join("verbs.yaml"))? {
        for verb in sequence(&document, "verbs") {
            let Some(name) = string_at(verb, "verb") else {
                continue;
            };
            verbs.insert(
                name,
                Verb {
                    mutating: verb.get("mutating").and_then(Yaml::as_bool) != Some(false),
                    role: string_at(verb, "pipeline_role").unwrap_or_default(),
                },
            );
        }
    }

    let mut capabilities = BTreeMap::new();
    if let Some(document) = load_optional(&spec.join("capabilities.yaml"))? {
        for capability in sequence(&document, "provider_capabilities") {
            let Some(id) = string_at(capability, "id") else {
                continue;
            };
            capabilities.insert(
                id,
                Capability {
                    risk: string_at(capability, "risk").unwrap_or_default(),
                    network: capability.get("network").and_then(Yaml::as_bool) == Some(true),
                },
            );
        }
    }

    let deferred = load_optional(&spec.join("schemas").join("deferred.yaml"))?
        .map(|document| {
            sequence(&document, "deferred")
                .into_iter()
                .filter_map(|entry| string_at(entry, "id"))
                .collect()
        })
        .unwrap_or_default();

    Ok(Contracts {
        commands,
        by_spelling,
        verbs,
        capabilities,
        deferred,
    })
}

/// The exemption register, by `(command, example)`.
fn read_exemptions(root: &Path) -> Result<BTreeMap<(String, String), String>, GenerateError> {
    let mut exemptions = BTreeMap::new();
    let Some(document) = load_optional(&root.join(EXEMPTIONS))? else {
        return Ok(exemptions);
    };
    for entry in sequence(&document, "exemptions") {
        let command = string_at(entry, "command").unwrap_or_default();
        let example = string_at(entry, "example").unwrap_or_default();
        let reason = string_at(entry, "reason").unwrap_or_default();
        if command.is_empty() || example.is_empty() || reason.is_empty() {
            return Err(GenerateError {
                detail: format!(
                    "{EXEMPTIONS} has an entry without a `command`, an `example` or a `reason` \
                     (`{command}` `{example}`); an exemption nobody can check is not one"
                ),
            });
        }
        if exemptions
            .insert((command.clone(), example.clone()), reason)
            .is_some()
        {
            return Err(GenerateError {
                detail: format!("{EXEMPTIONS} exempts `{example}` of `{command}` twice"),
            });
        }
    }
    Ok(exemptions)
}

/// A Rust identifier fragment for a registry id such as `ono.process.get`.
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

fn load_optional(path: &Path) -> Result<Option<Yaml>, GenerateError> {
    if path.is_file() {
        load(path).map(Some)
    } else {
        Ok(None)
    }
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
