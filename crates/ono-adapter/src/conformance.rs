//! The fixture conformance harness (spec v0.3 §1.47, ADR-0057 point 8): every fixture of
//! every adapter decodes, through the shell's own decoder, to exactly what its sidecar says.

use std::path::Path;

use ono_value::Value;

use crate::contract::{Adapter, AdapterPack, Fixture, Problem};
use crate::decode::{Trace, decode};
use crate::version::Version;

/// Checks every fixture of every adapter in `pack` under `fixtures_root`.
#[must_use]
pub fn check_pack(pack: &AdapterPack, fixtures_root: &Path) -> Vec<Problem> {
    let mut problems = Vec::new();
    for adapter in pack.adapters() {
        let directory = fixtures_root.join(adapter.fixtures());
        let mut entries: Vec<_> = std::fs::read_dir(&directory)
            .map(|entries| entries.filter_map(Result::ok).collect())
            .unwrap_or_default();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "out") {
                continue;
            }
            let name = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            let report = |detail: String| Problem {
                location: format!("{}/{}", pack.id(), adapter.id()),
                detail: format!("fixture `{name}`: {detail}"),
            };
            let (Ok(bytes), Ok(sidecar)) = (
                std::fs::read(&path),
                std::fs::read_to_string(path.with_extension("yaml")),
            ) else {
                problems.push(report("the bytes or the sidecar cannot be read".to_owned()));
                continue;
            };
            let fixture = match Fixture::parse(&sidecar) {
                Ok(fixture) => fixture,
                Err(error) => {
                    problems.push(report(format!("the sidecar does not parse: {error}")));
                    continue;
                }
            };
            problems.extend(
                check_fixture(adapter, &bytes, &fixture)
                    .into_iter()
                    .map(report),
            );
        }
    }
    problems
}

/// Checks one fixture; every string is one disagreement.
#[must_use]
pub fn check_fixture(adapter: &Adapter, bytes: &[u8], fixture: &Fixture) -> Vec<String> {
    let program = fixture
        .invocation()
        .first()
        .cloned()
        .unwrap_or_else(|| adapter.id().to_owned());
    let trace = Trace {
        executable: Path::new("/usr/bin").join(&program),
        version: Version::parse(fixture.tool_version()),
        user_invocation: fixture.invocation().to_vec(),
        actual_invocation: adapter
            .invocations()
            .first()
            .map(|invocation| invocation.plan().argv().to_vec())
            .unwrap_or_default(),
        host: None,
    };
    let decoded = decode(adapter, bytes, &trace, ono_value::builtin_schemas());
    let mut problems = Vec::new();

    if let Some(expected_error) = fixture.expected_error() {
        match decoded {
            Ok(values) => problems.push(format!(
                "expected `{expected_error}` but the decoder produced {} record(s)",
                values.len()
            )),
            Err(error) if error.code().name() != expected_error => problems.push(format!(
                "expected `{expected_error}` but the decoder produced `{}`: {}",
                error.code().name(),
                error.message()
            )),
            Err(_) => {}
        }
        return problems;
    }

    let expected = fixture.expected_records().unwrap_or_default();
    let values = match decoded {
        Ok(values) => values,
        Err(error) => {
            problems.push(format!(
                "expected {} record(s) but the decoder failed with `{}`: {}",
                expected.len(),
                error.code().name(),
                error.message()
            ));
            return problems;
        }
    };
    if values.len() != expected.len() {
        problems.push(format!(
            "expected {} record(s), decoded {}",
            expected.len(),
            values.len()
        ));
    }
    for (index, (value, wanted)) in values.iter().zip(expected).enumerate() {
        let Value::Record(record) = value else {
            problems.push(format!("record {} is not a record", index + 1));
            continue;
        };
        if record.provenance().adapter().is_none() {
            problems.push(format!(
                "record {} carries no adapter provenance",
                index + 1
            ));
        }
        for (field, expectation) in wanted {
            let actual = record.get(field).unwrap_or(&Value::Null);
            if !matches(expectation, actual) {
                problems.push(format!(
                    "record {} field `{field}`: expected {}, decoded {}",
                    index + 1,
                    yaml_text(expectation),
                    canonical(actual)
                ));
            }
        }
    }
    problems
}

/// Whether a decoded value is what the sidecar wrote, comparing canonical text forms.
fn matches(expected: &serde_yaml_ng::Value, actual: &Value) -> bool {
    match (expected, actual) {
        (serde_yaml_ng::Value::Sequence(items), Value::List(values)) => {
            items.len() == values.len()
                && items.iter().zip(values.iter()).all(|(e, a)| matches(e, a))
        }
        (serde_yaml_ng::Value::Sequence(_), _) => false,
        _ => squeeze(&yaml_text(expected)) == squeeze(&canonical(actual)),
    }
}

fn squeeze(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn yaml_text(value: &serde_yaml_ng::Value) -> String {
    match value {
        serde_yaml_ng::Value::Null => "null".to_owned(),
        serde_yaml_ng::Value::Bool(value) => value.to_string(),
        serde_yaml_ng::Value::Number(number) => number.to_string(),
        serde_yaml_ng::Value::String(text) => text.clone(),
        other => format!("{other:?}"),
    }
}

/// A value in the text form the sidecars are written in.
#[must_use]
pub fn canonical(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::Decimal(value) => value.to_string(),
        Value::String(text) => text.to_string(),
        Value::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        Value::Path(path) => path.display().to_string(),
        Value::Timestamp(value) => value.to_string(),
        Value::Duration(value) => value.exact(),
        Value::ByteSize(value) => value.exact(),
        Value::Percent(value) => format!("{}%", value.value()),
        Value::Ip(value) => value.to_string(),
        Value::IpNetwork(value) => value.to_string(),
        Value::Port(value) => value.to_string(),
        Value::List(items) => {
            let parts: Vec<String> = items.iter().map(canonical).collect();
            format!("[{}]", parts.join(", "))
        }
        other => format!("{other:?}"),
    }
}
