//! The observable surface of a `Value`: its type name, its accessors and its provenance.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use ono_core::ErrorCode;
use ono_value::{
    ByteSize, Decimal, Duration, IpNetwork, Link, MapValue, Percent, Provenance, RegexValue,
    SchemaId, Uuid, Value,
};

#[test]
fn should_name_every_type_stably() {
    let cases: Vec<(Value, &str)> = vec![
        (Value::Null, "null"),
        (Value::Bool(true), "bool"),
        (Value::Int(1), "int"),
        (Value::Float(1.0), "float"),
        (Value::Decimal(Decimal::from_int(1)), "decimal"),
        (Value::String("x".into()), "string"),
        (Value::Bytes(Bytes::from_static(b"x")), "bytes"),
        (Value::Path(Arc::from(Path::new("/tmp"))), "path"),
        (
            Value::Timestamp(jiff::Timestamp::from_nanosecond(0).unwrap()),
            "timestamp",
        ),
        (Value::Duration(Duration::ZERO), "duration"),
        (Value::ByteSize(ByteSize::from_bytes(1)), "bytesize"),
        (Value::Percent(Percent::new(1.0)), "percent"),
        (
            Value::Regex(Arc::new(RegexValue::new("a").unwrap())),
            "regex",
        ),
        (Value::Uuid(Uuid::from_bytes([0; 16])), "uuid"),
        (Value::Ip(std::net::IpAddr::from([127, 0, 0, 1])), "ip"),
        (
            Value::IpNetwork(IpNetwork::parse("192.0.2.0/24").unwrap()),
            "ipnetwork",
        ),
        (Value::Port(80), "port"),
        (Value::List(Arc::from(vec![])), "list"),
        (Value::Map(Arc::new(MapValue::new())), "map"),
    ];

    for (value, name) in cases {
        assert_eq!(value.type_name(), name, "wrong type name for {value:?}");
    }
}

#[test]
fn should_report_a_type_mismatch_instead_of_panicking_when_an_accessor_is_wrong() {
    let value = Value::String("not a number".into());

    let error = value.as_int().expect_err("a string is not an int");

    assert_eq!(error.code(), ErrorCode::TypeMismatch);
    assert!(
        error.to_string().contains("string"),
        "the message must say what it got, got {error}"
    );
    assert!(
        error.to_string().contains("int"),
        "the message must say what it wanted, got {error}"
    );
}

#[test]
fn should_read_a_float_from_an_integer_because_every_integer_is_one() {
    assert_eq!(Value::Int(3).as_float().unwrap(), 3.0);
    assert_eq!(Value::Float(3.5).as_float().unwrap(), 3.5);
}

#[test]
fn should_expose_the_scalars_the_accessors_promise() {
    assert!(Value::Bool(true).as_bool().unwrap());
    assert_eq!(Value::Int(7).as_int().unwrap(), 7);
    assert_eq!(Value::String("x".into()).as_str().unwrap(), "x");
    assert_eq!(
        Value::Path(Arc::from(Path::new("/etc"))).as_path().unwrap(),
        Path::new("/etc")
    );
    assert_eq!(Value::Port(80).as_port().unwrap(), 80);
    assert_eq!(
        Value::ByteSize(ByteSize::from_bytes(9))
            .as_byte_size()
            .unwrap(),
        ByteSize::from_bytes(9)
    );
}

#[test]
fn should_treat_a_null_as_less_than_anything_known_when_ordering() {
    assert_eq!(
        Value::Null.compare_to(&Value::Null).unwrap(),
        std::cmp::Ordering::Equal
    );
    assert_eq!(
        Value::Null.compare_to(&Value::Int(0)).unwrap(),
        std::cmp::Ordering::Less
    );
    assert_eq!(
        Value::Int(0).compare_to(&Value::Null).unwrap(),
        std::cmp::Ordering::Greater
    );
}

#[test]
fn should_keep_map_equality_independent_of_insertion_order() {
    let mut first = MapValue::new();
    first.insert("a".into(), Value::Int(1));
    first.insert("b".into(), Value::Int(2));
    let mut second = MapValue::new();
    second.insert("b".into(), Value::Int(2));
    second.insert("a".into(), Value::Int(1));

    assert_eq!(first, second);
}

#[test]
fn should_keep_map_iteration_in_insertion_order() {
    let mut map = MapValue::new();
    map.insert("z".into(), Value::Int(1));
    map.insert("a".into(), Value::Int(2));

    let keys: Vec<&str> = map.keys().collect();

    assert_eq!(keys, vec!["z", "a"], "rendering order is insertion order");
}

#[test]
fn should_render_provenance_the_way_inspect_shows_it() {
    let provenance = Provenance::local("linux.procfs", SchemaId::new("ono.process", 1))
        .observed_at(jiff::Timestamp::from_nanosecond(1_700_000_000_182_000_000).unwrap())
        .from_source("/proc/4419/status + /proc/4419/stat");

    let rendered = provenance.render();

    assert!(rendered.contains("provider"), "got:\n{rendered}");
    assert!(rendered.contains("linux.procfs"), "got:\n{rendered}");
    assert!(rendered.contains("observed"), "got:\n{rendered}");
    assert!(
        rendered.contains("/proc/4419/status + /proc/4419/stat"),
        "got:\n{rendered}"
    );
    assert!(rendered.contains("local"), "got:\n{rendered}");
    assert!(rendered.contains("ono.process/1"), "got:\n{rendered}");
}

#[test]
fn should_report_a_remote_link_in_provenance() {
    let provenance = Provenance::remote("linux.procfs", "prod-db", SchemaId::new("ono.process", 1));

    assert_eq!(provenance.link(), &Link::Remote("prod-db".into()));
    assert!(provenance.render().contains("prod-db"));
}

#[test]
fn should_leave_confidence_unset_until_a_provider_states_it() {
    let provenance = Provenance::local("linux.procfs", SchemaId::new("ono.process", 1));

    assert_eq!(
        provenance.confidence(),
        None,
        "unknown confidence is null, never a fabricated 1.0"
    );
    assert_eq!(provenance.with_confidence(0.5).confidence(), Some(0.5));
}

#[test]
fn should_parse_a_schema_id_from_its_rendered_form() {
    let id: SchemaId = "ono.process/1".parse().unwrap();

    assert_eq!(id.name(), "ono.process");
    assert_eq!(id.version(), 1);
    assert_eq!(id.to_string(), "ono.process/1");
}

#[test]
fn should_reject_a_schema_id_without_a_version() {
    let error = "ono.process"
        .parse::<SchemaId>()
        .expect_err("a schema id carries a version");

    assert_eq!(error.code(), ErrorCode::ParseSyntax);
}

#[test]
fn should_match_a_regex_value_against_text() {
    let regex = RegexValue::new("^ono-[0-9]+$").unwrap();

    assert!(regex.is_match("ono-42"));
    assert!(!regex.is_match("ono-x"));
    assert_eq!(regex.source(), "^ono-[0-9]+$");
}

#[test]
fn should_reject_an_invalid_regex_with_a_parse_error() {
    let error = RegexValue::new("[unclosed").expect_err("that pattern does not compile");

    assert_eq!(error.code(), ErrorCode::ParseSyntax);
}
