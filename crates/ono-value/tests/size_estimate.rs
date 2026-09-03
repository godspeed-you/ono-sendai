//! The deterministic approximate retained-size estimator of spec v0.4.1 §21.2 and §2.4.
//!
//! A count limit alone is not a memory limit when each counted element may carry an arbitrary
//! payload (§65.6). Every byte budget in the shell is spent through this one function, so what it
//! answers has to be stable, defined for every value, and never smaller than the payload it can
//! see.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use ono_core::ErrorCode;
use ono_value::{
    ByteSize, Decimal, Duration, ErrorValue, IpNetwork, MapValue, Percent, Provenance, RecordValue,
    RegexValue, Schema, SchemaId, Uuid, Value, estimated_size,
};

/// One value of every variant the enum declares, so a new variant fails the coverage test rather
/// than silently escaping the estimator.
fn one_of_every_variant() -> Vec<Value> {
    let schema = Arc::new(
        Schema::builder(SchemaId::new("ono.estimate.demo", 1), "Demo")
            .field(ono_value::FieldDef::new("name", ono_value::FieldType::String).nullable())
            .build()
            .unwrap(),
    );
    let provenance = Provenance::local("demo", schema.id().clone());
    let record = RecordValue::builder(schema, provenance)
        .set("name", Value::string("payload"))
        .unwrap()
        .build();

    let mut map = MapValue::new();
    map.insert("key".into(), Value::Int(7));

    vec![
        Value::Null,
        Value::Bool(true),
        Value::Int(1),
        Value::Float(1.5),
        Value::Decimal(Decimal::from_int(3)),
        Value::string("some text"),
        Value::Bytes(Bytes::from_static(b"some bytes")),
        Value::Path(Arc::from(Path::new("/var/log/messages"))),
        Value::Timestamp(jiff::Timestamp::from_nanosecond(0).unwrap()),
        Value::Duration(Duration::ZERO),
        Value::ByteSize(ByteSize::from_bytes(1024)),
        Value::Percent(Percent::new(50.0)),
        Value::Regex(Arc::new(RegexValue::new("a+b").unwrap())),
        Value::Uuid(Uuid::from_bytes([0u8; 16])),
        Value::Ip("127.0.0.1".parse().unwrap()),
        Value::IpNetwork(IpNetwork::new("10.0.0.0".parse().unwrap(), 8).unwrap()),
        Value::Port(22),
        Value::list([Value::Int(1), Value::Int(2)]),
        Value::Map(Arc::new(map)),
        record.into_value(),
        ErrorValue::new(ErrorCode::IoNotFound, "missing").into_value(),
    ]
}

/// A value deep enough to exercise recursion, wide enough to exercise payload, and mixed enough
/// to exercise every compound arm at once.
fn a_corpus_of_values() -> Vec<Value> {
    let mut corpus = one_of_every_variant();

    let mut map = MapValue::new();
    map.insert("list".into(), Value::list((0..32).map(Value::Int)));
    map.insert("text".into(), Value::string(&"x".repeat(4096)));
    map.insert(
        "nested".into(),
        Value::Map(Arc::new(MapValue::from_iter([(
            Arc::from("inner"),
            Value::Bytes(Bytes::from(vec![0u8; 8192])),
        )]))),
    );
    corpus.push(Value::Map(Arc::new(map)));

    let mut deep = Value::Int(0);
    for _ in 0..64 {
        deep = Value::list([deep, Value::string("rung")]);
    }
    corpus.push(deep);

    corpus.push(
        ErrorValue::new(ErrorCode::IoPermissionDenied, "denied")
            .with_help("try again with more authority")
            .with_source(ErrorValue::new(ErrorCode::IoNotFound, "no such file"))
            .with_metadata("path", Value::Path(Arc::from(Path::new("/etc/shadow"))))
            .into_value(),
    );

    corpus
}

#[test]
fn should_answer_the_same_estimate_for_the_same_value_on_every_run() {
    for value in a_corpus_of_values() {
        let first = estimated_size(&value);
        let repeated: BTreeSet<u64> = (0..16).map(|_| estimated_size(&value)).collect();
        assert_eq!(
            repeated,
            BTreeSet::from([first]),
            "the estimate for a {} varied between runs; a budget built on a figure that moves \
             refuses different inputs on different days (spec v0.4.1 §21.2)",
            value.type_name()
        );

        let clone = value.clone();
        assert_eq!(
            estimated_size(&clone),
            first,
            "a clone of a {} estimated differently from the value it was cloned from",
            value.type_name()
        );
    }
}

#[test]
fn should_define_an_estimate_for_every_value_variant() {
    let variants = one_of_every_variant();
    let named: BTreeSet<&str> = variants.iter().map(Value::type_name).collect();
    assert_eq!(
        named.len(),
        variants.len(),
        "the coverage corpus repeats a variant, so some variant is untested"
    );

    for value in &variants {
        assert!(
            estimated_size(value) > 0,
            "a {} estimated as zero bytes; every value occupies its own slot at minimum, and a \
             variant the estimator forgets is a byte budget that cannot see it (spec §21.2)",
            value.type_name()
        );
    }
}

#[test]
fn should_stay_within_the_documented_tolerance_of_the_measured_retained_size() {
    // The one payload figure a test can measure exactly: bytes the test itself allocated.
    let string_bytes = 4096;
    let strings = 1000;
    let text = "x".repeat(string_bytes);
    let list = Value::list((0..strings).map(|_| Value::string(&text)));
    let payload = u64::try_from(string_bytes * strings).unwrap();

    let estimate = estimated_size(&list);
    assert!(
        estimate >= payload,
        "the estimator answered {estimate} for {payload} bytes of string payload it can see; \
         §21.2 forbids intentionally undercounting known payload bytes"
    );
    assert!(
        estimate <= payload * 2,
        "the estimator answered {estimate} for {payload} bytes of payload, more than the \
         documented factor of two; a byte budget that charges double refuses work that fits"
    );

    let bytes = Value::Bytes(Bytes::from(vec![0u8; 1 << 20]));
    let measured = 1u64 << 20;
    let estimate = estimated_size(&bytes);
    assert!(
        estimate >= measured && estimate <= measured * 2,
        "the estimator answered {estimate} for a {measured}-byte blob, outside the documented \
         tolerance"
    );
}

#[test]
fn should_count_shared_payload_once_within_one_estimate() {
    let shared = Value::string(&"x".repeat(1 << 16));
    let alone = estimated_size(&shared);

    let hundred_clones = Value::list((0..100).map(|_| shared.clone()));
    let shared_estimate = estimated_size(&hundred_clones);

    let hundred_copies = Value::list((0..100).map(|_| Value::string(&"x".repeat(1 << 16))));
    let copied_estimate = estimated_size(&hundred_copies);

    assert!(
        shared_estimate < alone * 2,
        "a list of 100 clones of one {alone}-byte string estimated as {shared_estimate}; §21.2 \
         asks for shared `Arc` data not to be double-counted within one traversal"
    );
    assert!(
        copied_estimate > shared_estimate * 50,
        "100 separately allocated strings estimated as {copied_estimate} and 100 clones of one \
         string as {shared_estimate}; sharing is what separates them, so the estimates must differ"
    );
}
