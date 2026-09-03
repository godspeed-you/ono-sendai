//! The canonical object schemas of spec §28, loaded from the contracts that define them.
//!
//! Spec §27 makes the machine-readable registries the public contract, and AGENTS.md §5 places
//! `docs/spec/` above the implementation in the authority order. These schemas are therefore
//! **parsed from `docs/spec/schemas/*.v1.yaml`**, embedded at compile time, rather than restated
//! in Rust beside them.
//!
//! Restating them was the first thing tried, and it drifted within a single phase: the file
//! schema and the contract disagreed about which fields identify an interface, and nothing could
//! have noticed, because each was checked only against itself. One source of truth removes the
//! whole class of problem rather than adding a check for it (spec §36.4, §36.5).
//!
//! Parsing happens once, lazily, behind a `OnceLock`, so it costs nothing until a provider is
//! actually used and never appears in the startup budget of spec §34.

use std::sync::{Arc, OnceLock};

use ono_core::ErrorCode;
use serde_yaml_ng::Value as Yaml;

use crate::error::ErrorValue;
use crate::schema::{FieldDef, FieldType, Schema, SchemaId, SchemaRegistry, Unit};

/// Every schema contract, embedded at compile time.
///
/// A schema whose file is missing is a compile error rather than an empty registry at run time,
/// which is the point of embedding them.
const CONTRACTS: &[&str] = &[
    include_str!("../../../docs/spec/schemas/action-result.v1.yaml"),
    include_str!("../../../docs/spec/schemas/assistant.v1.yaml"),
    include_str!("../../../docs/spec/schemas/assistant-action.v1.yaml"),
    include_str!("../../../docs/spec/schemas/assistant-turn.v1.yaml"),
    include_str!("../../../docs/spec/schemas/capability-grant.v1.yaml"),
    include_str!("../../../docs/spec/schemas/evidence.v1.yaml"),
    include_str!("../../../docs/spec/schemas/finding.v1.yaml"),
    include_str!("../../../docs/spec/schemas/model-provider.v1.yaml"),
    include_str!("../../../docs/spec/schemas/plugin.v1.yaml"),
    include_str!("../../../docs/spec/schemas/plugin-audit-event.v1.yaml"),
    include_str!("../../../docs/spec/schemas/plugin-inspection.v1.yaml"),
    include_str!("../../../docs/spec/schemas/plugin-package.v1.yaml"),
    include_str!("../../../docs/spec/schemas/plugin-runtime.v1.yaml"),
    include_str!("../../../docs/spec/schemas/recommendation.v1.yaml"),
    include_str!("../../../docs/spec/schemas/verification-result.v1.yaml"),
    include_str!("../../../docs/spec/schemas/block-device.v1.yaml"),
    include_str!("../../../docs/spec/schemas/cgroup.v1.yaml"),
    include_str!("../../../docs/spec/schemas/command.v1.yaml"),
    include_str!("../../../docs/spec/schemas/commit.v1.yaml"),
    include_str!("../../../docs/spec/schemas/change-summary.v1.yaml"),
    include_str!("../../../docs/spec/schemas/config-setting.v1.yaml"),
    include_str!("../../../docs/spec/schemas/container-event.v1.yaml"),
    include_str!("../../../docs/spec/schemas/container.v1.yaml"),
    include_str!("../../../docs/spec/schemas/context.v1.yaml"),
    include_str!("../../../docs/spec/schemas/device.v1.yaml"),
    include_str!("../../../docs/spec/schemas/dns-record.v1.yaml"),
    include_str!("../../../docs/spec/schemas/endpoint.v1.yaml"),
    include_str!("../../../docs/spec/schemas/env-var.v1.yaml"),
    include_str!("../../../docs/spec/schemas/error.v1.yaml"),
    include_str!("../../../docs/spec/schemas/file-event.v1.yaml"),
    include_str!("../../../docs/spec/schemas/file.v1.yaml"),
    include_str!("../../../docs/spec/schemas/filesystem.v1.yaml"),
    include_str!("../../../docs/spec/schemas/git-status-entry.v1.yaml"),
    include_str!("../../../docs/spec/schemas/graph-edge.v1.yaml"),
    include_str!("../../../docs/spec/schemas/graph-node.v1.yaml"),
    include_str!("../../../docs/spec/schemas/graph.v1.yaml"),
    include_str!("../../../docs/spec/schemas/host-key.v1.yaml"),
    include_str!("../../../docs/spec/schemas/client-key.v1.yaml"),
    include_str!("../../../docs/spec/schemas/group-event.v1.yaml"),
    include_str!("../../../docs/spec/schemas/group.v1.yaml"),
    include_str!("../../../docs/spec/schemas/host.v1.yaml"),
    include_str!("../../../docs/spec/schemas/host-event.v1.yaml"),
    include_str!("../../../docs/spec/schemas/http-exchange.v1.yaml"),
    include_str!("../../../docs/spec/schemas/image.v1.yaml"),
    include_str!("../../../docs/spec/schemas/interface-address.v1.yaml"),
    include_str!("../../../docs/spec/schemas/interface-event.v1.yaml"),
    include_str!("../../../docs/spec/schemas/interface.v1.yaml"),
    include_str!("../../../docs/spec/schemas/job.v1.yaml"),
    include_str!("../../../docs/spec/schemas/journal-event.v1.yaml"),
    include_str!("../../../docs/spec/schemas/link-event.v1.yaml"),
    include_str!("../../../docs/spec/schemas/link-place.v1.yaml"),
    include_str!("../../../docs/spec/schemas/link.v1.yaml"),
    include_str!("../../../docs/spec/schemas/mount-event.v1.yaml"),
    include_str!("../../../docs/spec/schemas/landmark.v1.yaml"),
    include_str!("../../../docs/spec/schemas/log-record.v1.yaml"),
    include_str!("../../../docs/spec/schemas/hidden-summary.v1.yaml"),
    include_str!("../../../docs/spec/schemas/map-cluster.v1.yaml"),
    include_str!("../../../docs/spec/schemas/map-edge.v1.yaml"),
    include_str!("../../../docs/spec/schemas/map-node.v1.yaml"),
    include_str!("../../../docs/spec/schemas/mount.v1.yaml"),
    include_str!("../../../docs/spec/schemas/namespace.v1.yaml"),
    include_str!("../../../docs/spec/schemas/navigation-step.v1.yaml"),
    include_str!("../../../docs/spec/schemas/neighbor.v1.yaml"),
    include_str!("../../../docs/spec/schemas/neighborhood.v1.yaml"),
    include_str!("../../../docs/spec/schemas/neighborhood-group.v1.yaml"),
    include_str!("../../../docs/spec/schemas/mount-boundary.v1.yaml"),
    include_str!("../../../docs/spec/schemas/open-file.v1.yaml"),
    include_str!("../../../docs/spec/schemas/place-view.v1.yaml"),
    include_str!("../../../docs/spec/schemas/probe-result.v1.yaml"),
    include_str!("../../../docs/spec/schemas/package.v1.yaml"),
    include_str!("../../../docs/spec/schemas/package-source.v1.yaml"),
    include_str!("../../../docs/spec/schemas/process-detail.v1.yaml"),
    include_str!("../../../docs/spec/schemas/process-event.v1.yaml"),
    include_str!("../../../docs/spec/schemas/process.v1.yaml"),
    include_str!("../../../docs/spec/schemas/provider.v1.yaml"),
    include_str!("../../../docs/spec/schemas/route-event.v1.yaml"),
    include_str!("../../../docs/spec/schemas/route.v1.yaml"),
    include_str!("../../../docs/spec/schemas/service-event.v1.yaml"),
    include_str!("../../../docs/spec/schemas/service.v1.yaml"),
    include_str!("../../../docs/spec/schemas/session.v1.yaml"),
    include_str!("../../../docs/spec/schemas/socket-event.v1.yaml"),
    include_str!("../../../docs/spec/schemas/socket.v1.yaml"),
    include_str!("../../../docs/spec/schemas/spatial-neighbor.v1.yaml"),
    include_str!("../../../docs/spec/schemas/spatial-change.v1.yaml"),
    include_str!("../../../docs/spec/schemas/spatial-map.v1.yaml"),
    include_str!("../../../docs/spec/schemas/spatial-place.v1.yaml"),
    include_str!("../../../docs/spec/schemas/spatial-relation.v1.yaml"),
    include_str!("../../../docs/spec/schemas/system.v1.yaml"),
    include_str!("../../../docs/spec/schemas/user-event.v1.yaml"),
    include_str!("../../../docs/spec/schemas/user.v1.yaml"),
];

/// The schemas every provider and command can rely on.
///
/// ```
/// use ono_value::{SchemaId, builtin_schemas};
/// let process = builtin_schemas()
///     .get(&SchemaId::new("ono.process", 1))
///     .expect("the process schema is built in");
/// assert_eq!(process.name(), "Process");
/// assert!(process.field("pid").is_some());
/// ```
#[must_use]
pub fn builtin_schemas() -> &'static SchemaRegistry {
    static REGISTRY: OnceLock<SchemaRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut registry = SchemaRegistry::new();
        for contract in CONTRACTS {
            match parse_schema(contract) {
                Ok(schema) => {
                    // Two contracts declaring one id is a contract bug, and `spec-check` reports
                    // it by name. Here the first wins, so a duplicate cannot make the registry
                    // unusable for every other schema.
                    let _ = registry.register(schema);
                }
                // A contract that does not parse is caught by the test below and by
                // `cargo xtask spec-check`; at run time the remaining schemas still load, so one
                // malformed file cannot take the whole shell down with it.
                Err(_) => continue,
            }
        }
        registry
    })
}

/// The `ono.action-result/1` schema, which every mutating command produces (spec §11.5).
///
/// # Panics
///
/// Panics only if the embedded contract for it is malformed, which the test suite and
/// `cargo xtask spec-check` both prevent from reaching a release.
#[must_use]
pub fn action_result_schema() -> Arc<Schema> {
    static FALLBACK: OnceLock<Arc<Schema>> = OnceLock::new();
    builtin_schemas()
        .get(&SchemaId::new("ono.action-result", 1))
        .unwrap_or_else(|| {
            // Unreachable while the contract is valid, which the test below and `spec-check` both
            // require. A degraded schema is still better than a panic in a shell someone is using.
            Arc::clone(FALLBACK.get_or_init(|| {
                Arc::new(
                    Schema::builder(SchemaId::new("ono.action-result", 1), "ActionResult")
                        .field(FieldDef::new("operation", FieldType::String).required())
                        .build()
                        .unwrap_or_else(|_| {
                            Schema::empty(SchemaId::new("ono.action-result", 1), "ActionResult")
                        }),
                )
            }))
        })
}

/// Reads one contract into a [`Schema`].
fn parse_schema(contract: &str) -> Result<Schema, ErrorValue> {
    let document: Yaml = serde_yaml_ng::from_str(contract).map_err(|error| {
        ErrorValue::new(
            ErrorCode::ProviderSchemaViolation,
            format!("a schema contract is not valid YAML: {error}"),
        )
    })?;

    let id: SchemaId = text(&document, "id")
        .ok_or_else(|| violation("a schema contract has no `id`"))?
        .parse()?;
    let name =
        text(&document, "name").ok_or_else(|| violation("a schema contract has no `name`"))?;

    let mut builder = Schema::builder(id.clone(), &name);
    if let Some(summary) = text(&document, "summary") {
        builder = builder.doc(&summary);
    }

    let fields = document
        .get("fields")
        .and_then(Yaml::as_mapping)
        .ok_or_else(|| violation(&format!("`{id}` declares no fields")))?;

    for (key, definition) in fields {
        let Some(field_name) = key.as_str() else {
            continue;
        };
        let declared = text(definition, "type")
            .ok_or_else(|| violation(&format!("`{id}.{field_name}` has no type")))?;
        let mut field = FieldDef::new(field_name, parse_type(&declared, definition)?);

        if definition.get("required").and_then(Yaml::as_bool) == Some(true) {
            field = field.required();
        }
        if definition.get("nullable").and_then(Yaml::as_bool) == Some(true) {
            field = field.nullable();
        }
        if let Some(unit) = text(definition, "unit").and_then(|unit| parse_unit(&unit)) {
            field = field.with_unit(unit);
        }
        if let Some(doc) = text(definition, "doc") {
            field = field.with_doc(&doc);
        }
        builder = builder.field(field);
    }

    let identity: Vec<String> = list(&document, "identity");
    if !identity.is_empty() {
        builder = builder.identity(identity.iter().map(String::as_str));
    }
    let fallback: Vec<String> = list(&document, "identity_fallback");
    if !fallback.is_empty() {
        builder = builder.identity_fallback(fallback.iter().map(String::as_str));
    }
    let columns: Vec<String> = document
        .get("default_view")
        .map(|view| list(view, "columns"))
        .unwrap_or_default();
    if !columns.is_empty() {
        builder = builder.default_view(columns.iter().map(String::as_str));
    }

    builder.build()
}

/// Reads the type vocabulary of spec §10.2 as `docs/spec/schemas/` spells it.
fn parse_type(declared: &str, definition: &Yaml) -> Result<FieldType, ErrorValue> {
    // `enum` carries its members beside it, because a closed set is not a type on its own.
    if declared == "enum" {
        let variants = list(definition, "values");
        if variants.is_empty() {
            return Err(violation("an `enum` field declares no `values`"));
        }
        let borrowed: Vec<&str> = variants.iter().map(String::as_str).collect();
        return Ok(FieldType::enumeration(&borrowed));
    }
    if let Some(inner) = declared
        .strip_prefix("list<")
        .and_then(|rest| rest.strip_suffix('>'))
    {
        return Ok(FieldType::list(parse_type(inner, &Yaml::Null)?));
    }
    if let Some(target) = declared
        .strip_prefix("ref<")
        .and_then(|rest| rest.strip_suffix('>'))
    {
        return Ok(FieldType::Ref(target.parse()?));
    }
    // `ono.error/1` is the shape of the error *value*, not of a record that happens to describe
    // one: a structured error is its own `Value` variant (spec §25), so a field declared with it
    // accepts an error and nothing else. Writing the schema id in the contract keeps the field's
    // shape documented in one place; mapping it here keeps the runtime type honest.
    if declared == "ono.error/1" {
        return Ok(FieldType::Error);
    }
    // Any other bare schema id means the record itself, as against `ref<…>`, which carries only
    // its identity.
    if declared.contains('/') {
        return Ok(FieldType::Record(declared.parse()?));
    }

    Ok(match declared {
        "any" | "value" => FieldType::Any,
        "bool" => FieldType::Bool,
        "int" => FieldType::Int,
        "float" => FieldType::Float,
        "decimal" => FieldType::Decimal,
        "string" => FieldType::String,
        "bytes" => FieldType::Bytes,
        "path" => FieldType::Path,
        "timestamp" => FieldType::Timestamp,
        "duration" => FieldType::Duration,
        "bytesize" => FieldType::ByteSize,
        "percent" => FieldType::Percent,
        "regex" => FieldType::Regex,
        "uuid" => FieldType::Uuid,
        "ip" => FieldType::Ip,
        "ipnetwork" => FieldType::IpNetwork,
        "port" => FieldType::Port,
        "map" | "record" => FieldType::Map,
        "error" => FieldType::Error,
        other => {
            return Err(violation(&format!(
                "`{other}` is not one of the types spec §10.2 defines"
            )));
        }
    })
}

fn parse_unit(declared: &str) -> Option<Unit> {
    match declared {
        "percent" => Some(Unit::Percent),
        "bytes" => Some(Unit::Bytes),
        "seconds" => Some(Unit::Seconds),
        "count" => Some(Unit::Count),
        _ => None,
    }
}

fn violation(message: &str) -> ErrorValue {
    ErrorValue::new(ErrorCode::ProviderSchemaViolation, message.to_owned())
}

fn text(document: &Yaml, key: &str) -> Option<String> {
    document.get(key).and_then(Yaml::as_str).map(str::to_owned)
}

fn list(document: &Yaml, key: &str) -> Vec<String> {
    document
        .get(key)
        .and_then(Yaml::as_sequence)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_load_every_embedded_contract_without_a_single_failure() {
        // A contract that does not parse degrades gracefully at run time, which is right, and
        // would therefore be invisible. This is where it is made visible.
        for contract in CONTRACTS {
            let parsed = parse_schema(contract);
            assert!(
                parsed.is_ok(),
                "a committed schema contract does not load: {:?}\n{}",
                parsed.err().map(|error| error.message().to_owned()),
                contract.lines().take(4).collect::<Vec<_>>().join("\n")
            );
        }
        assert_eq!(
            builtin_schemas().len(),
            CONTRACTS.len(),
            "every contract must reach the registry; a missing one means two declared the same id"
        );
    }
}
