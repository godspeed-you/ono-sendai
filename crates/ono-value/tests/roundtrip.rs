//! Property-style serialization and unit round trips (spec §35.2).
//!
//! The generator is a deterministic pseudo-random sequence written here on purpose: a test that
//! depends on an external RNG or on wall-clock entropy is not reproducible, and an unreproducible
//! failure teaches nothing.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use ono_core::ErrorCode;
use ono_value::{
    ByteSize, ByteUnit, Decimal, Duration, DurationUnit, ErrorValue, FieldDef, FieldType,
    IpNetwork, MapValue, Percent, Provenance, RecordValue, RegexValue, Schema, SchemaId,
    SchemaRegistry, Uuid, Value, builtin_schemas, from_json, to_json,
};

/// SplitMix64: four lines, no dependency, and the same sequence on every machine forever.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next_u64() % bound
    }
}

fn sample_scalar(rng: &mut Rng) -> Value {
    match rng.below(19) {
        0 => Value::Null,
        1 => Value::Bool(rng.below(2) == 1),
        2 => Value::Int(i128::from(rng.next_u64() as i64)),
        3 => Value::Int(i128::from(i64::MAX) * 3 + i128::from(rng.below(97))),
        4 => Value::Float(f64::from(rng.next_u64() as i32) / 64.0),
        5 => Value::Decimal(Decimal::new(i128::from(rng.next_u64() as i64), 4).unwrap()),
        6 => Value::String(format!("value-{}", rng.next_u64()).into()),
        7 => Value::Bytes(Bytes::from(vec![0xff, 0x00, 0xfe, rng.below(256) as u8])),
        8 => Value::Path(Arc::from(Path::new("/proc/1/status"))),
        9 => Value::Timestamp(
            jiff::Timestamp::from_nanosecond(i128::from(rng.next_u64() as u32) * 1_000).unwrap(),
        ),
        10 => Value::Duration(Duration::from_nanoseconds(
            i128::from(rng.next_u64() as i32),
        )),
        11 => Value::ByteSize(ByteSize::from_bytes(u128::from(rng.next_u64()))),
        12 => Value::Percent(Percent::new(f64::from(rng.below(10_000) as u32) / 100.0)),
        13 => Value::Regex(Arc::new(RegexValue::new("^ono-[0-9]+$").unwrap())),
        14 => Value::Uuid(Uuid::from_bytes(
            rng.next_u64().to_be_bytes().repeat(2).try_into().unwrap(),
        )),
        15 => Value::Ip(std::net::IpAddr::from([192, 0, 2, rng.below(256) as u8])),
        16 => Value::IpNetwork(IpNetwork::new(std::net::IpAddr::from([192, 0, 2, 0]), 24).unwrap()),
        17 => Value::Port(rng.below(65_536) as u16),
        _ => Value::Error(Arc::new(
            ErrorValue::new(ErrorCode::IoPermissionDenied, "access denied")
                .with_help("requires root or read capability")
                .with_source(ErrorValue::new(
                    ErrorCode::ProviderUnavailable,
                    "procfs is not mounted",
                )),
        )),
    }
}

fn sample_value(rng: &mut Rng, depth: u32) -> Value {
    if depth == 0 {
        return sample_scalar(rng);
    }
    match rng.below(4) {
        0 => {
            let len = rng.below(4);
            Value::List((0..len).map(|_| sample_value(rng, depth - 1)).collect())
        }
        1 => {
            let mut map = MapValue::new();
            for index in 0..rng.below(4) {
                map.insert(
                    format!("key{index}").as_str().into(),
                    sample_value(rng, depth - 1),
                );
            }
            Value::Map(Arc::new(map))
        }
        _ => sample_scalar(rng),
    }
}

#[test]
fn should_round_trip_every_generated_value_through_json() {
    let mut rng = Rng(0x0D15_EA5E_0BAD_F00D);
    let registry = SchemaRegistry::new();

    for iteration in 0..2_000 {
        let value = sample_value(&mut rng, 2);
        let json = to_json(&value);
        let back = from_json(&json, &registry).unwrap_or_else(|error| {
            panic!("iteration {iteration}: {value:?} failed to decode: {error}")
        });

        assert_eq!(
            back, value,
            "iteration {iteration}: JSON must round trip {value:?}"
        );
    }
}

#[test]
fn should_round_trip_every_generated_value_through_a_json_string() {
    let mut rng = Rng(0xC0FF_EE00_1234_5678);
    let registry = SchemaRegistry::new();

    for iteration in 0..500 {
        let value = sample_value(&mut rng, 2);
        let text = ono_value::to_json_string(&value).unwrap();
        let back = ono_value::from_json_str(&text, &registry).unwrap_or_else(|error| {
            panic!("iteration {iteration}: {text} failed to decode: {error}")
        });

        assert_eq!(back, value, "iteration {iteration}: text was {text}");
    }
}

#[test]
fn should_round_trip_a_record_with_extras_and_provenance() {
    let schema = builtin_schemas()
        .get(&SchemaId::new("ono.user", 1))
        .expect("ono.user/1 is built in");
    let provenance = Provenance::local("linux.nss", schema.id().clone())
        .observed_at(jiff::Timestamp::from_nanosecond(1_700_000_000_000_000_000).unwrap())
        .from_source("/etc/passwd");
    let record = RecordValue::builder(schema, provenance)
        .set("uid", Value::Int(1000))
        .unwrap()
        .set("name", Value::String("william".into()))
        .unwrap()
        .set_extra("dev.example.badge", Value::Int(3))
        .build();
    let value = Value::Record(Arc::new(record));

    let back = from_json(&to_json(&value), builtin_schemas()).unwrap();

    assert_eq!(back, value);
}

#[test]
fn should_refuse_to_decode_a_record_whose_schema_is_unknown() {
    let schema = Arc::new(
        Schema::builder(SchemaId::new("ono.test.thing", 1), "Thing")
            .field(FieldDef::new("id", FieldType::Int).required())
            .build()
            .unwrap(),
    );
    let provenance = Provenance::local("test", schema.id().clone());
    let value = Value::Record(Arc::new(
        RecordValue::builder(schema, provenance)
            .set("id", Value::Int(1))
            .unwrap()
            .build(),
    ));

    let error = from_json(&to_json(&value), &SchemaRegistry::new())
        .expect_err("an unregistered schema cannot be rebuilt");

    assert_eq!(error.code(), ErrorCode::ResolveTargetNotFound);
}

#[test]
fn should_keep_undecodable_bytes_through_a_json_round_trip() {
    let value = Value::Bytes(Bytes::from_static(&[0xff, 0xfe, 0x00, 0x80]));
    let registry = SchemaRegistry::new();

    let back = from_json(&to_json(&value), &registry).unwrap();

    assert_eq!(
        back, value,
        "spec §12.2: undecodable bytes must never be lost"
    );
}

#[test]
fn should_round_trip_every_byte_unit_through_its_own_rendering() {
    for unit in ByteUnit::ALL {
        for multiple in [1_u128, 2, 7, 1023] {
            let size = ByteSize::from_bytes(multiple * unit.factor());

            let rendered = size.render_in(*unit);
            let parsed = ByteSize::parse(&rendered)
                .unwrap_or_else(|error| panic!("{rendered} did not parse back: {error}"));

            assert_eq!(parsed, size, "{rendered} must parse back to the same size");
        }
    }
}

#[test]
fn should_round_trip_every_duration_unit_through_its_own_rendering() {
    for unit in DurationUnit::ALL {
        for multiple in [-1_i128, 1, 3, 907] {
            let duration = Duration::from_nanoseconds(multiple * unit.nanoseconds());

            let rendered = duration.render_in(*unit);
            let parsed = Duration::parse(&rendered)
                .unwrap_or_else(|error| panic!("{rendered} did not parse back: {error}"));

            assert_eq!(
                parsed, duration,
                "{rendered} must parse back to the same duration"
            );
        }
    }
}

#[test]
fn should_round_trip_the_exact_rendering_of_every_generated_semantic_scalar() {
    let mut rng = Rng(0x5EED_1234_ABCD_9876);

    for _ in 0..1_000 {
        let size = ByteSize::from_bytes(u128::from(rng.next_u64()));
        assert_eq!(ByteSize::parse(&size.exact()).unwrap(), size);

        let duration = Duration::from_nanoseconds(i128::from(rng.next_u64() as i64));
        assert_eq!(Duration::parse(&duration.exact()).unwrap(), duration);

        let percent = Percent::new(f64::from(rng.next_u64() as i32) / 128.0);
        assert_eq!(Percent::parse(&percent.to_string()).unwrap(), percent);

        let decimal = Decimal::new(i128::from(rng.next_u64() as i64), rng.below(9) as u32).unwrap();
        assert_eq!(Decimal::parse(&decimal.to_string()).unwrap(), decimal);
    }
}

#[test]
fn should_round_trip_a_uuid_through_its_text_form() {
    let uuid = Uuid::parse("0191f0e2-7c4a-7b3d-8e91-2a5c6f7d8e9f").unwrap();

    assert_eq!(uuid.to_string(), "0191f0e2-7c4a-7b3d-8e91-2a5c6f7d8e9f");
    assert_eq!(Uuid::parse(&uuid.to_string()).unwrap(), uuid);
}

#[test]
fn should_round_trip_an_ip_network_through_its_text_form() {
    for text in ["192.0.2.0/24", "10.0.0.0/8", "2001:db8::/32", "::/0"] {
        let network = IpNetwork::parse(text).unwrap();
        assert_eq!(network.to_string(), text);
    }
}
