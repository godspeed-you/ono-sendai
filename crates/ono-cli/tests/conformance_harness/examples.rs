//! What the generated command suite asks of every example it runs (issue #149, ADR-0935).
//!
//! `crates/ono-cli/tests/command_conformance.rs` carries one case per documented example the
//! contracts let run: the command, the example as written, and the command's declared `output`.
//! The question is asked here, once: run the example through the real `ono` in a scratch
//! environment, observe every value it produces through `inspect` — the public boundary that
//! reports a value's type, its record schema and each field's access and value — and hold each to
//! the declaration and to the field types `docs/contracts/schemas/` fixes.

use std::path::{Path, PathBuf};

use ono_testkit::{Scratch, Shell};
use serde_json::Value as Json;
use serde_yaml_ng::Value as Yaml;

/// One documented example, and the output its command declares.
pub struct ExampleCase {
    /// The command whose contract documents the example.
    pub command: &'static str,
    /// The example, as the contract writes it.
    pub example: &'static str,
    /// The command's declared `output`, as the contract writes it.
    pub output: &'static str,
}

/// Runs the example and holds every value it produces to the declared output.
///
/// # Panics
///
/// When the example fails, prints something that is not an inspection, produces a number of
/// values its declaration does not admit, or produces a value its declaration does not describe.
pub fn assert_example_conforms(case: &ExampleCase) {
    let dir = ono_testkit::scratch();
    let declared = Declared::parse(case.output);
    // A command that declares no output is a sink, so there is nothing to inspect: it is held to
    // running, and to leaving nothing for a stage after it.
    let script = if declared.is_nothing() {
        case.example.to_owned()
    } else {
        format!("{} | inspect | to json", case.example)
    };
    let run = hermetic(&dir).args(["-c", &script]).run();
    assert!(
        run.status().is_success(),
        "`{}` ({}) is a documented example its contracts let run hermetically, and it failed with \
         {}.\nstderr:\n{}\nIf the environment, not the command, is why, exempt it in \
         docs/contracts/conformance/command_examples.yaml with the reason.",
        case.example,
        case.command,
        run.status(),
        run.stderr()
    );
    if declared.is_nothing() {
        return;
    }
    let observed: Json = serde_json::from_str(run.stdout().trim()).unwrap_or_else(|error| {
        panic!(
            "`{script}` printed something other than one JSON document ({error}):\n{}\nstderr:\n{}",
            run.stdout(),
            run.stderr()
        )
    });
    let values = observed
        .as_array()
        .unwrap_or_else(|| panic!("`to json` prints a stream as an array, got {observed}"));

    let mut problems = Vec::new();
    if let Some(problem) = declared.count_problem(values.len()) {
        problems.push(problem);
    }
    for (index, inspection) in values.iter().enumerate() {
        if let Err(problem) = declared.admits(inspection) {
            problems.push(format!("value {index}: {problem}"));
        }
    }
    assert!(
        problems.is_empty(),
        "`{}` declares `output: {}` and `{}` produced something else (issue #149):\n  {}",
        case.command,
        case.output,
        case.example,
        problems.join("\n  ")
    );
}

/// A shell that reads and writes nothing outside `dir` and inherits nothing from the person
/// running the tests but `PATH`, so an example meets the same empty world on every machine.
fn hermetic(dir: &Scratch) -> Shell {
    let at = |name: &str| dir.path().join(name).display().to_string();
    let mut shell = Shell::new()
        .clear_env()
        .cwd(dir.path())
        .env("HOME", dir.path().display().to_string())
        .env("XDG_CONFIG_HOME", at("config"))
        .env("XDG_DATA_HOME", at("data"))
        .env("XDG_CACHE_HOME", at("cache"))
        .env("XDG_STATE_HOME", at("state"))
        .env("ONO_CONFIG_DIR", at("config/ono"))
        .env("ONO_PLUGIN_PATH", at("plugins"))
        .env("NO_COLOR", "1")
        .timeout(std::time::Duration::from_secs(30));
    if let Some(path) = std::env::var_os("PATH") {
        shell = shell.env("PATH", path.to_string_lossy().into_owned());
    }
    shell
}

/// A declared `output`: one or more alternatives, each a single value or a stream of them.
struct Declared {
    alternatives: Vec<(bool, String)>,
}

impl Declared {
    fn parse(output: &str) -> Self {
        let alternatives = output
            .split('|')
            .map(str::trim)
            .filter(|alternative| !alternative.is_empty())
            .map(|alternative| {
                match alternative
                    .strip_prefix("stream<")
                    .and_then(|rest| rest.strip_suffix('>'))
                {
                    Some(element) => (true, element.trim().to_owned()),
                    None => (false, alternative.to_owned()),
                }
            })
            .collect();
        Self { alternatives }
    }

    /// Whether the declaration is `null`: a sink that produces no value at all.
    fn is_nothing(&self) -> bool {
        self.alternatives
            .iter()
            .all(|(stream, ty)| !*stream && ty == "null")
    }

    /// What is wrong with producing `count` values, if anything.
    fn count_problem(&self, count: usize) -> Option<String> {
        if self.alternatives.iter().any(|(stream, _)| *stream) {
            return None;
        }
        let may_be_nothing = self.alternatives.iter().any(|(_, ty)| ty == "null");
        let may_be_one = self.alternatives.iter().any(|(_, ty)| ty != "null");
        match count {
            0 if may_be_nothing => None,
            1 if may_be_one => None,
            _ => Some(format!(
                "produced {count} values where the declaration admits {}",
                match (may_be_nothing, may_be_one) {
                    (true, true) => "none or one",
                    (true, false) => "none",
                    _ => "exactly one",
                }
            )),
        }
    }

    /// Whether one inspected value is one of the declared alternatives.
    fn admits(&self, inspection: &Json) -> Result<(), String> {
        let mut why = Vec::new();
        for (_, ty) in &self.alternatives {
            match admits(ty, inspection) {
                Ok(()) => return Ok(()),
                Err(problem) => why.push(format!("not `{ty}`: {problem}")),
            }
        }
        Err(why.join("; "))
    }
}

/// Whether one inspected value is a value of the declared type `ty`.
fn admits(ty: &str, inspection: &Json) -> Result<(), String> {
    let kind = inspection
        .get("type")
        .and_then(Json::as_str)
        .ok_or_else(|| format!("`inspect` gave no type: {inspection}"))?;
    match ty {
        "any" | "value" => Ok(()),
        "record" => expect_kind(kind, "record"),
        "ono.error/1" => expect_kind(kind, "error"),
        schema if schema.contains('/') => {
            expect_kind(kind, "record")?;
            let claimed = inspection.get("schema").and_then(Json::as_str);
            if claimed != Some(schema) {
                return Err(format!(
                    "a record of `{}`, not of `{schema}`",
                    claimed.unwrap_or("no schema")
                ));
            }
            fields_conform(schema, inspection)
        }
        scalar => expect_kind(kind, scalar),
    }
}

fn expect_kind(kind: &str, expected: &str) -> Result<(), String> {
    if kind == expected {
        Ok(())
    } else {
        Err(format!("a `{kind}`"))
    }
}

/// The record's fields against the schema contract: the same names in the same order, and every
/// value of its declared type, with null only where the contract allows it.
fn fields_conform(schema: &str, inspection: &Json) -> Result<(), String> {
    let contract = schema_contract(schema)?;
    let declared = contract_fields(&contract);
    let observed = inspection
        .get("fields")
        .and_then(Json::as_array)
        .ok_or_else(|| "`inspect` listed no fields".to_owned())?;
    let observed_names: Vec<&str> = observed
        .iter()
        .filter_map(|field| field.get("name").and_then(Json::as_str))
        .collect();
    let declared_names: Vec<&str> = declared.iter().map(|(name, _)| name.as_str()).collect();
    if observed_names != declared_names {
        return Err(format!(
            "its fields are {observed_names:?}, and the contract declares {declared_names:?}"
        ));
    }
    let mut problems = Vec::new();
    for (field, (name, definition)) in observed.iter().zip(&declared) {
        let access = field.get("access").and_then(Json::as_str).unwrap_or("");
        let value = field.get("value").unwrap_or(&Json::Null);
        let outcome = match access {
            "known" => value_conforms(definition, value),
            "unknown" | "absent" => null_allowed(definition),
            "failed" => Ok(()),
            other => Err(format!("an access `{other}` spec §10.5 does not name")),
        };
        if let Err(problem) = outcome {
            problems.push(format!("`{schema}.{name}`: {problem}"));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

/// Whether a null is a lawful value of a field, by `ono_value::Schema::validate`'s rule.
fn null_allowed(definition: &Yaml) -> Result<(), String> {
    let required = definition.get("required").and_then(Yaml::as_bool) == Some(true);
    let nullable = definition.get("nullable").and_then(Yaml::as_bool) == Some(true);
    if required || !nullable {
        Err("null where the contract requires a value".to_owned())
    } else {
        Ok(())
    }
}

/// Whether a value, as `to json` writes it, is a value of the declared field type.
fn value_conforms(definition: &Yaml, value: &Json) -> Result<(), String> {
    let declared = definition
        .get("type")
        .and_then(Yaml::as_str)
        .unwrap_or("any");
    if declared == "enum" {
        let members: Vec<&str> = definition
            .get("values")
            .and_then(Yaml::as_sequence)
            .map(|values| values.iter().filter_map(Yaml::as_str).collect())
            .unwrap_or_default();
        return match value.as_str() {
            Some(member) if members.contains(&member) => Ok(()),
            _ => Err(format!("{value} is not one of {members:?}")),
        };
    }
    type_conforms(declared, value)
}

fn type_conforms(declared: &str, value: &Json) -> Result<(), String> {
    if value.is_null() {
        // A nested null: a list item or a nested record's field the provider did not know.
        return Ok(());
    }
    if let Some(inner) = declared
        .strip_prefix("list<")
        .and_then(|rest| rest.strip_suffix('>'))
    {
        let items = value
            .as_array()
            .ok_or_else(|| format!("{value} is not a list"))?;
        for item in items {
            type_conforms(inner, item)?;
        }
        return Ok(());
    }
    // `ono_value::FieldType::accepts`: "A reference is an identity, and providers legitimately
    // carry it as a name, a number, an identity map or the resolved object itself."
    if declared.starts_with("ref<") {
        return Ok(());
    }
    if declared == "ono.error/1" || declared == "error" {
        return expect(
            value
                .as_object()
                .is_some_and(|map| map.contains_key("code")),
            declared,
            value,
        );
    }
    if declared.contains('/') {
        return nested_record_conforms(declared, value);
    }
    let fits = match declared {
        "any" | "value" => true,
        "bool" => value.is_boolean(),
        "int" => value.is_i64() || value.is_u64() || is_integer_text(value),
        "float" | "decimal" | "percent" => value.is_number() || value.is_string(),
        "bytesize" | "port" => value.is_u64() || value.is_i64() || is_integer_text(value),
        "string" | "path" | "regex" | "uuid" | "ip" | "ipnetwork" | "timestamp" | "duration"
        | "bytes" => value.is_string(),
        "map" | "record" => value.is_object(),
        other => return Err(format!("`{other}` is not a type spec §10.2 defines")),
    };
    expect(fits, declared, value)
}

/// A record nested in a field, as `to json` writes it: a plain object of its declared fields,
/// then the extensions a provider attached (`docs/contracts/commands/data.yaml`, `to`). Every
/// declared field it carries holds a value of the declared type; an extension is not checked,
/// as `ono_value::Schema::validate` does not check one.
fn nested_record_conforms(schema: &str, value: &Json) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{value} is not a `{schema}` record"))?;
    let contract = schema_contract(schema)?;
    for (name, definition) in contract_fields(&contract) {
        if let Some(nested) = object.get(&name).filter(|nested| !nested.is_null()) {
            value_conforms(&definition, nested)
                .map_err(|problem| format!("`{name}`: {problem}"))?;
        }
    }
    Ok(())
}

fn expect(fits: bool, declared: &str, value: &Json) -> Result<(), String> {
    if fits {
        Ok(())
    } else {
        Err(format!("{value} is not a `{declared}`"))
    }
}

fn is_integer_text(value: &Json) -> bool {
    value
        .as_str()
        .is_some_and(|text| text.parse::<i128>().is_ok())
}

/// The contract document of `schema`, read from `docs/contracts/schemas/`.
fn schema_contract(schema: &str) -> Result<Yaml, String> {
    let path = schema_path(schema)
        .ok_or_else(|| format!("`{schema}` is not a schema id a contract file can hold"))?;
    let text = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "`{schema}` has no contract at {} — a declared output nobody wrote down",
            path.display()
        )
    })?;
    serde_yaml_ng::from_str(&text)
        .map_err(|error| format!("{} is not YAML: {error}", path.display()))
}

/// `ono.process/1` lives in `process.v1.yaml`.
fn schema_path(schema: &str) -> Option<PathBuf> {
    let (name, version) = schema.split_once('/')?;
    let stem = name.strip_prefix("ono.")?;
    Some(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/contracts/schemas")
            .join(format!("{stem}.v{version}.yaml")),
    )
}

/// The contract's fields, in the order it declares them.
fn contract_fields(contract: &Yaml) -> Vec<(String, Yaml)> {
    contract
        .get("fields")
        .and_then(Yaml::as_mapping)
        .map(|fields| {
            fields
                .iter()
                .filter_map(|(name, definition)| {
                    Some((name.as_str()?.to_owned(), definition.clone()))
                })
                .collect()
        })
        .unwrap_or_default()
}
