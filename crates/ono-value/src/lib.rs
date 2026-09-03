//! The value, record and schema model of the Ono-Sendai shell (spec §10, §16.1, §25, §28).
//!
//! This crate is what makes Ono an object shell rather than a text shell. A provider produces
//! [`RecordValue`]s bound to a [`Schema`]; a pipeline moves [`Value`]s; a renderer turns them
//! into something to look at. None of those layers parses text produced by another (spec §5).
//!
//! # What lives here
//!
//! - [`Value`] — the runtime value of spec §10.2 and §25, cheap to clone because every compound
//!   case sits behind an `Arc`.
//! - [`ByteSize`], [`Duration`], [`Percent`] — semantic scalars that know their dimension, so
//!   `512MiB > 1GiB` is a comparison and `10s + 512MiB` is an error (spec §10.6).
//! - [`Schema`], [`SchemaId`], [`SchemaRegistry`], [`classify_change`] — the object contracts of
//!   spec §27.3 and the evolution rules of spec §10.4.
//! - [`RecordValue`] with [`FieldAccess`] — fields stored by schema position (spec §25.1), read
//!   through the three-way distinction spec §10.5 insists on.
//! - [`Provenance`] — where a record came from, so `inspect` can be trusted (spec §25.2).
//! - [`estimated_size`] — the deterministic approximate retained size of spec v0.4.1 §21.2, the
//!   one figure every byte budget in the shell is spent against, and [`Budget`], the shared
//!   ceiling of §21.1 that spends it.
//! - [`ErrorValue`] — the structured error of spec §16.1, carried as data.
//! - [`ActionResult`] — the acknowledgement a mutating command returns (spec §11.5).
//! - [`builtin_schemas`] — the canonical object schemas of spec §28.
//! - [`to_json`], [`to_yaml`], [`to_csv`], [`to_text`] and [`to_bytes`] with their inverses —
//!   the serializations of spec §7.1, §12.2, §12.4 and §46. Each carries what its format can
//!   carry and returns a structured error for what it cannot, so no conversion is ever a
//!   silent lie.
//! - [`to_json_data`] and [`to_yaml_data`] — the interop serializations of spec §33.5: the data
//!   alone, for a tool that has never heard of Ono. `to json` and `to yaml` write these; the
//!   tagged forms above stay for round trips inside the system.
//!
//! # The three meanings of "nothing"
//!
//! Spec §10.5 requires an absent field, an unknown value and a failed access to stay apart, and
//! this crate keeps them apart in the type system rather than by convention:
//!
//! ```
//! use ono_core::ErrorCode;
//! use ono_value::{ErrorValue, FieldAccess, FieldDef, FieldType, Provenance, RecordValue,
//!                 Schema, SchemaId, Value};
//! use std::sync::Arc;
//!
//! let schema = Arc::new(
//!     Schema::builder(SchemaId::new("ono.demo", 1), "Demo")
//!         .field(FieldDef::new("known", FieldType::Int).required())
//!         .field(FieldDef::new("unknown", FieldType::Int).nullable())
//!         .field(FieldDef::new("unreadable", FieldType::Int).nullable())
//!         .build()?,
//! );
//! let provenance = Provenance::local("demo", schema.id().clone());
//! let record = RecordValue::builder(schema, provenance)
//!     .set("known", Value::Int(1))?
//!     .set("unreadable", ErrorValue::new(ErrorCode::IoPermissionDenied, "denied").into_value())?
//!     .build();
//!
//! assert_eq!(record.access("nowhere"), FieldAccess::Absent);
//! assert_eq!(record.access("unknown"), FieldAccess::Unknown);
//! assert_eq!(record.access("known"), FieldAccess::Known(Value::Int(1)));
//! assert!(record.access("unreadable").is_failed());
//! # Ok::<(), ono_value::ErrorValue>(())
//! ```

#![forbid(unsafe_code)]

mod action;
mod arith;
mod budget;
mod builtin;
mod csv;
mod decimal;
mod error;
mod hex;
mod json;
mod map;
mod net;
mod os_bytes;
mod provenance;
mod raw;
mod record;
mod regex_value;
mod schema;
mod size;
mod text;
mod units;
mod uuid;
mod value;
mod yaml;

pub use action::{ActionResult, ActionStatus};
pub use budget::{
    Budget, COMMAND_CAPTURE_MAX_BYTES, COMMAND_CAPTURE_MAX_ITEMS, Ceiling, Exceeded,
    MATERIALIZE_MAX_BYTES, MATERIALIZE_MAX_ITEMS, MaterializationLimits,
};
pub use builtin::{action_result_schema, builtin_schemas};
pub use csv::{from_csv, to_csv};
pub use decimal::Decimal;
pub use error::{ErrorValue, ValueRef};
pub use json::{from_json, from_json_str, to_json, to_json_data, to_json_string};
pub use map::MapValue;
pub use net::IpNetwork;
pub use provenance::{AdapterTrace, Link, Provenance};
pub use raw::{from_bytes, to_bytes, to_bytes_of};
pub use record::{FieldAccess, FieldStep, RecordBuilder, RecordValue};
pub use regex_value::RegexValue;
pub use schema::{
    Compatibility, FieldDef, FieldType, Schema, SchemaBuilder, SchemaChange, SchemaChangeKind,
    SchemaDiff, SchemaId, SchemaRegistry, Unit, classify_change,
};
pub use size::{MAX_ESTIMATE_DEPTH, estimated_size};
pub use text::{canonical_text, to_text};
pub use units::{ByteSize, ByteUnit, Duration, DurationUnit, Percent};
pub use uuid::Uuid;
pub use value::Value;
pub use yaml::{MAX_YAML_DEPTH, from_yaml, to_yaml, to_yaml_data, yaml_depth};
