//! The canonical object schemas of spec §28, loaded from the contracts that define them.
//!
//! Spec §27 makes the machine-readable registries the public contract, and AGENTS.md §5 places
//! `docs/contracts/` above the implementation in the authority order. These schemas are therefore
//! **parsed from `docs/contracts/schemas/*.v1.yaml`**, embedded at compile time, rather than restated
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

/// Every schema contract, by file stem, embedded at compile time.
///
/// A schema whose file is missing is a compile error rather than an empty registry at run time,
/// which is the point of embedding them. They are embedded as JSON, transcoded from the YAML by
/// `build.rs`, because reading ninety YAML documents cost a quarter of a cold start (ADR-0571).
const CONTRACTS: &[&str] = &[
    include_str!(concat!(env!("OUT_DIR"), "/schemas/action-result.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/assistant.v1.json")),
    include_str!(concat!(
        env!("OUT_DIR"),
        "/schemas/assistant-action.v1.json"
    )),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/assistant-turn.v1.json")),
    include_str!(concat!(
        env!("OUT_DIR"),
        "/schemas/capability-grant.v1.json"
    )),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/evidence.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/finding.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/model-provider.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/plugin.v1.json")),
    include_str!(concat!(
        env!("OUT_DIR"),
        "/schemas/plugin-audit-event.v1.json"
    )),
    include_str!(concat!(
        env!("OUT_DIR"),
        "/schemas/plugin-inspection.v1.json"
    )),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/plugin-package.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/plugin-runtime.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/recommendation.v1.json")),
    include_str!(concat!(
        env!("OUT_DIR"),
        "/schemas/verification-result.v1.json"
    )),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/block-device.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/cgroup.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/command.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/commit.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/change-summary.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/config-setting.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/container-event.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/container.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/context.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/device.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/dns-record.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/endpoint.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/env-var.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/error.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/file-event.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/file.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/filesystem.v1.json")),
    include_str!(concat!(
        env!("OUT_DIR"),
        "/schemas/git-status-entry.v1.json"
    )),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/graph-edge.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/graph-node.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/graph.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/host-key.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/client-key.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/group-event.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/group.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/host.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/host-event.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/http-exchange.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/image.v1.json")),
    include_str!(concat!(
        env!("OUT_DIR"),
        "/schemas/interface-address.v1.json"
    )),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/interface-event.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/interface.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/job.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/journal-event.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/link-event.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/link-place.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/link.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/mount-event.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/landmark.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/log-record.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/hidden-summary.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/map-cluster.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/map-edge.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/map-node.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/mount.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/namespace.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/navigation-step.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/neighbor.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/neighborhood.v1.json")),
    include_str!(concat!(
        env!("OUT_DIR"),
        "/schemas/neighborhood-group.v1.json"
    )),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/mount-boundary.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/open-file.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/place-view.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/probe-result.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/package.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/package-source.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/process-detail.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/process-event.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/process.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/provider.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/route-event.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/route.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/service-event.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/service.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/session.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/socket-event.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/socket.v1.json")),
    include_str!(concat!(
        env!("OUT_DIR"),
        "/schemas/spatial-neighbor.v1.json"
    )),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/spatial-change.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/spatial-map.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/spatial-place.v1.json")),
    include_str!(concat!(
        env!("OUT_DIR"),
        "/schemas/spatial-relation.v1.json"
    )),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/system.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/user-event.v1.json")),
    include_str!(concat!(env!("OUT_DIR"), "/schemas/user.v1.json")),
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

/// Reads one contract, as the JSON the build script wrote, into a [`Schema`].
fn parse_schema(contract: &str) -> Result<Schema, ErrorValue> {
    let document: Yaml = serde_json::from_str(contract).map_err(|error| {
        ErrorValue::new(
            ErrorCode::ProviderSchemaViolation,
            format!("a schema contract does not read: {error}"),
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

/// Reads the type vocabulary of spec §10.2 as `docs/contracts/schemas/` spells it.
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

#[cfg(test)]
mod embedded_documents {
    use super::CONTRACTS;

    /// One JSON spelling per document, so two value trees compare as text: the mapping order
    /// is the file's on both sides, and `serde_json` keeps it (`preserve_order`).
    fn canonical(value: &serde_yaml_ng::Value) -> String {
        serde_json::to_string(value).expect("a schema document serializes as JSON")
    }

    /// What the binary carries is what `docs/contracts/schemas/` says, value for value (ADR-0571).
    ///
    /// A subset, not an equality: the directory also holds `deferred.yaml`, the register of
    /// schemas a later phase will write, and whether every schema there is embedded is
    /// `spec-check`'s question, not this crate's.
    #[test]
    fn should_embed_every_schema_contract_as_the_spec_states_it() {
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/contracts/schemas");
        let mut on_disk: Vec<String> = std::fs::read_dir(&directory)
            .expect("docs/contracts/schemas/ exists")
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "yaml")
            })
            .map(|path| {
                let text = std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("{} should read: {error}", path.display()));
                let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text)
                    .unwrap_or_else(|error| panic!("{} should be YAML: {error}", path.display()));
                canonical(&value)
            })
            .collect();
        let mut embedded: Vec<String> = CONTRACTS
            .iter()
            .map(|json| {
                let value: serde_yaml_ng::Value =
                    serde_json::from_str(json).expect("the build script writes valid JSON");
                canonical(&value)
            })
            .collect();
        on_disk.sort();
        embedded.sort();
        let missing: Vec<&String> = embedded
            .iter()
            .filter(|document| on_disk.binary_search(document).is_err())
            .collect();
        assert!(
            missing.is_empty(),
            "embedded, but not as on disk: {missing:#?}"
        );
    }
}
