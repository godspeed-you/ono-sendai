//! Turning the engine's JSON into `ono.container/1` and `ono.image/1` records.
//!
//! Everything the engine did not say becomes a null, and nothing becomes a zero or an empty
//! string (spec §35.3). The engine's own sentinels are read as what they mean: a `RepoTags` of
//! `<none>:<none>` is an untagged image, a `Created` of `0` is "not known".

use std::sync::{Arc, OnceLock};

use jiff::Timestamp;
use ono_value::{
    ByteSize, ErrorValue, MapValue, Provenance, RecordValue, Schema, SchemaId, Value,
    builtin_schemas,
};
use serde_json::Value as Json;

use crate::provider::PROVIDER_ID;

/// The `ono.container/1` schema, as `docs/spec/schemas/container.v1.yaml` fixes it.
///
/// ```
/// let schema = ono_provider_container::container_schema();
/// assert_eq!(schema.id().to_string(), "ono.container/1");
/// assert_eq!(schema.identity(), ["id".into()]);
/// ```
#[must_use]
#[allow(
    clippy::expect_used,
    reason = "AGENTS.md section 16 admits `expect` in a provably unreachable state. `ono.container/1` is \
              embedded from docs/spec/schemas/ at compile time and \
              crates/ono-value/tests/builtin_schemas.rs turns red the moment it is not; nothing a \
              user does can reach this branch."
)]
pub fn container_schema() -> Arc<Schema> {
    static SCHEMA: OnceLock<Arc<Schema>> = OnceLock::new();
    Arc::clone(SCHEMA.get_or_init(|| {
        builtin_schemas()
            .get(&SchemaId::new("ono.container", 1))
            .expect("ono.container/1 is one of the schemas the shell ships")
    }))
}

/// The `ono.image/1` schema, as `docs/spec/schemas/image.v1.yaml` fixes it.
///
/// ```
/// let schema = ono_provider_container::image_schema();
/// assert_eq!(schema.id().to_string(), "ono.image/1");
/// ```
#[must_use]
#[allow(
    clippy::expect_used,
    reason = "the same provably unreachable state as `container_schema`"
)]
pub fn image_schema() -> Arc<Schema> {
    static SCHEMA: OnceLock<Arc<Schema>> = OnceLock::new();
    Arc::clone(SCHEMA.get_or_init(|| {
        builtin_schemas()
            .get(&SchemaId::new("ono.image", 1))
            .expect("ono.image/1 is one of the schemas the shell ships")
    }))
}

fn provenance(schema: &Schema, endpoint: &str) -> Provenance {
    Provenance::local(PROVIDER_ID, schema.id().clone())
        .from_source(endpoint)
        .observed_at(Timestamp::now())
}

/// The container record for one entry of `GET /containers/json` or one `GET
/// /containers/{id}/json` answer — the two shapes differ, and both are read.
///
/// # Errors
///
/// `provider.schema_violation` when the JSON carries no `Id`, or when the shipped schema no
/// longer declares a field this provider fills (spec §36.5).
pub(crate) fn container_record(json: &Json, endpoint: &str) -> Result<RecordValue, ErrorValue> {
    let schema = container_schema();
    let id = text(json.get("Id")).ok_or_else(|| violation("a container entry has no `Id`"))?;
    // The listing carries `Names: ["/web"]`; the inspection carries `Name: "/web"`.
    let name = json
        .get("Names")
        .and_then(Json::as_array)
        .and_then(|names| names.first())
        .and_then(Json::as_str)
        .or_else(|| json.get("Name").and_then(Json::as_str))
        .map(|name| name.trim_start_matches('/'))
        .filter(|name| !name.is_empty());
    // The listing carries `Image` (the reference) and `ImageID`; the inspection carries `Image`
    // (the digest) and `Config.Image` (the reference).
    let inspected = json.get("Config").is_some();
    let (image, image_id) = if inspected {
        (
            json.get("Config")
                .and_then(|config| text(config.get("Image"))),
            text(json.get("Image")),
        )
    } else {
        (text(json.get("Image")), text(json.get("ImageID")))
    };
    let state = if inspected {
        json.get("State")
            .and_then(|state| text(state.get("Status")))
    } else {
        text(json.get("State"))
    };
    let labels = if inspected {
        json.get("Config").and_then(|config| config.get("Labels"))
    } else {
        json.get("Labels")
    };

    let record = RecordValue::builder(schema.clone(), provenance(&schema, endpoint))
        .set("id", Value::string(id))?
        .set("name", name.map_or(Value::Null, Value::string))?
        .set("image", image.map_or(Value::Null, Value::string))?
        .set("image_id", image_id.map_or(Value::Null, Value::string))?
        .set("state", Value::string(container_state(state)))?
        .set("created", created(json.get("Created")))?
        .set("labels", label_map(labels))?
        .build();
    Ok(record)
}

/// The image record for one entry of `GET /images/json`.
///
/// # Errors
///
/// `provider.schema_violation` when the JSON carries no `Id`.
pub(crate) fn image_record(json: &Json, endpoint: &str) -> Result<RecordValue, ErrorValue> {
    let schema = image_schema();
    let id = text(json.get("Id")).ok_or_else(|| violation("an image entry has no `Id`"))?;
    let tags: Vec<&str> = json
        .get("RepoTags")
        .and_then(Json::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(Json::as_str)
                .filter(|tag| !tag.is_empty() && *tag != "<none>:<none>")
                .collect()
        })
        .unwrap_or_default();
    let size = json
        .get("Size")
        .and_then(Json::as_u64)
        .map_or(Value::Null, |bytes| {
            Value::ByteSize(ByteSize::from_bytes(u128::from(bytes)))
        });

    let record = RecordValue::builder(schema.clone(), provenance(&schema, endpoint))
        .set("id", Value::string(id))?
        .set(
            "reference",
            tags.first().map_or(Value::Null, |tag| Value::string(tag)),
        )?
        .set(
            "tags",
            if tags.is_empty() {
                Value::Null
            } else {
                Value::list(tags.iter().map(|tag| Value::string(tag)))
            },
        )?
        .set("size", size)?
        .set("created", created(json.get("Created")))?
        .build();
    Ok(record)
}

/// Whether `record` is the image `reference` names: one of its tags, or a prefix of its id.
pub(crate) fn image_matches(record: &RecordValue, reference: &str) -> bool {
    let id_matches = record
        .get("id")
        .and_then(|id| id.as_str().ok())
        .is_some_and(|id| {
            id == reference
                || id
                    .strip_prefix("sha256:")
                    .unwrap_or(id)
                    .starts_with(reference)
        });
    let tag_matches = record
        .get("tags")
        .and_then(|tags| tags.as_list().ok())
        .is_some_and(|tags| {
            tags.iter()
                .any(|tag| tag.as_str().is_ok_and(|tag| tag == reference))
        });
    id_matches || tag_matches
}

fn violation(message: &str) -> ErrorValue {
    ErrorValue::new(ono_core::ErrorCode::ProviderSchemaViolation, message).with_help(
        "the engine's answer is not the Docker Engine API shape this provider reads; this is a \
         defect at the runtime boundary, not in your pipeline",
    )
}

fn text(value: Option<&Json>) -> Option<&str> {
    value.and_then(Json::as_str).filter(|text| !text.is_empty())
}

/// The `state` field: the engine's word where the schema knows it, `unknown` where it does not.
fn container_state(state: Option<&str>) -> &'static str {
    match state {
        Some("created") => "created",
        Some("running") => "running",
        Some("paused") => "paused",
        Some("restarting") => "restarting",
        Some("removing") => "removing",
        Some("exited") => "exited",
        Some("dead") => "dead",
        Some("stopping") => "stopping",
        Some("stopped") => "stopped",
        Some("configured") => "configured",
        _ => "unknown",
    }
}

/// `Created` is Unix seconds in a listing and an RFC 3339 text in an inspection; zero and an
/// unparsable text are both "not known".
fn created(value: Option<&Json>) -> Value {
    match value {
        Some(Json::Number(seconds)) => seconds
            .as_i64()
            .filter(|seconds| *seconds > 0)
            .and_then(|seconds| Timestamp::from_second(seconds).ok())
            .map_or(Value::Null, Value::Timestamp),
        Some(Json::String(text)) => text
            .parse::<Timestamp>()
            .ok()
            .filter(|at| at.as_second() > 0)
            .map_or(Value::Null, Value::Timestamp),
        _ => Value::Null,
    }
}

fn label_map(labels: Option<&Json>) -> Value {
    let Some(labels) = labels.and_then(Json::as_object) else {
        return Value::Null;
    };
    let mut map = MapValue::new();
    for (key, value) in labels {
        if let Some(value) = value.as_str() {
            map.insert(key.clone().into(), Value::string(value));
        }
    }
    Value::Map(Arc::new(map))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]
mod tests {
    use super::*;

    #[test]
    fn should_read_a_listing_entry_and_an_inspection_alike() {
        let listed: Json = serde_json::from_str(
            r#"{"Id":"abc","Names":["/web"],"Image":"nginx:1.27","ImageID":"sha256:0","State":"running","Created":1700000000,"Labels":{"a":"b"}}"#,
        )
        .unwrap();
        let inspected: Json = serde_json::from_str(
            r#"{"Id":"abc","Name":"/web","Image":"sha256:0","Created":"2023-11-14T22:13:20Z","State":{"Status":"running"},"Config":{"Image":"nginx:1.27","Labels":{"a":"b"}}}"#,
        )
        .unwrap();
        let a = container_record(&listed, "unix:///x").unwrap();
        let b = container_record(&inspected, "unix:///x").unwrap();
        for field in [
            "id", "name", "image", "image_id", "state", "created", "labels",
        ] {
            assert_eq!(a.get(field), b.get(field), "field `{field}`");
        }
        assert_eq!(a.get("name").unwrap().as_str().unwrap(), "web");
        assert_eq!(a.get("state").unwrap().as_str().unwrap(), "running");
    }

    #[test]
    fn should_leave_unknown_null_rather_than_invent_it() {
        let json: Json =
            serde_json::from_str(r#"{"Id":"abc","State":"zombie","Created":0}"#).unwrap();
        let record = container_record(&json, "unix:///x").unwrap();
        assert_eq!(record.get("name"), Some(&Value::Null));
        assert_eq!(record.get("image"), Some(&Value::Null));
        assert_eq!(record.get("created"), Some(&Value::Null));
        assert_eq!(record.get("labels"), Some(&Value::Null));
        assert_eq!(record.get("state").unwrap().as_str().unwrap(), "unknown");
    }

    #[test]
    fn should_treat_the_none_tag_as_no_reference() {
        let json: Json =
            serde_json::from_str(r#"{"Id":"sha256:0","RepoTags":["<none>:<none>"],"Size":10}"#)
                .unwrap();
        let record = image_record(&json, "unix:///x").unwrap();
        assert_eq!(record.get("reference"), Some(&Value::Null));
        assert_eq!(record.get("tags"), Some(&Value::Null));
        assert!(image_matches(&record, "sha256:0"));
        assert!(!image_matches(&record, "nginx"));
    }

    #[test]
    fn should_refuse_an_entry_without_an_id() {
        let json: Json = serde_json::from_str(r#"{"Names":["/web"]}"#).unwrap();
        let error = container_record(&json, "unix:///x").unwrap_err();
        assert_eq!(error.code(), ono_core::ErrorCode::ProviderSchemaViolation);
    }
}
