//! Helpers shared by the `ono-spatial-render` test suites (v0.4.1 §39.1, ADR-0427, ADR-0515).

#![allow(
    clippy::expect_used,
    dead_code,
    reason = "a test states its preconditions directly, and not every helper is used by every \
              test binary (AGENTS.md section 16)"
)]

use ono_value::{FieldDef, FieldType, Schema, SchemaId};

pub fn schema(id: &str, fields: &[(&str, FieldType)]) -> std::sync::Arc<Schema> {
    let mut builder = Schema::builder(SchemaId::new(id, 1), id);
    for (name, kind) in fields {
        builder = builder.field(FieldDef::new(name, kind.clone()));
    }
    std::sync::Arc::new(builder.build().expect("a well-formed schema"))
}
