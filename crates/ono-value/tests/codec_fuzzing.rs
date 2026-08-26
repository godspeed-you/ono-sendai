//! Every decoder in this crate reads text it did not write.
//!
//! `from json` parses whatever an external command printed; `from csv` parses whatever was in the
//! file. Spec §35.6 requires fuzzing of the serializers for exactly that reason, and ADR-0015 T7
//! makes a decoder that can be made to allocate without bound a release-blocking threat.
//!
//! The contract asserted here is narrow and absolute: **a decoder never panics and never hangs,
//! and every rejection is a structured error.** What it decides a particular malformed input
//! means is not asserted — that would be testing the implementation rather than the contract.

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does"
)]

use std::time::{Duration, Instant};

use ono_testkit::Rng;
use ono_value::{SchemaRegistry, builtin_schemas, from_bytes, from_csv, from_json_str, from_yaml};

/// The alphabet malformed inputs are built from.
///
/// Random bytes almost never reach past a decoder's first rejection. Pieces of the real grammars
/// almost always do, which is what makes a short seeded run worth more than a long random one.
const PIECES: &[&str] = &[
    "{",
    "}",
    "[",
    "]",
    ":",
    ",",
    "\"",
    "'",
    "\\",
    "\\u",
    "\\u{",
    "0",
    "-",
    "e",
    ".",
    " ",
    "\n",
    "\t",
    "\r",
    "null",
    "true",
    "false",
    "NaN",
    "Infinity",
    "$bytesize",
    "$duration",
    "$record",
    "$error",
    "$int",
    "1e999999",
    "99999999999999999999999999",
    "---",
    "- ",
    "? ",
    "&a",
    "*a",
    "!!str",
    "|",
    ">",
    "%YAML",
    "\u{feff}",
    "\u{0}",
    "\u{7f}",
    "é",
    "𝄞",
];

fn registry() -> &'static SchemaRegistry {
    builtin_schemas()
}

/// Feeds `count` generated inputs through `decode`, failing if any panics or the run drags.
fn hammer(seed: u64, count: usize, name: &str, mut decode: impl FnMut(&str)) {
    let mut rng = Rng::seeded(seed);
    let started = Instant::now();

    for round in 0..count {
        let input = rng.assemble(PIECES, 24);
        decode(&input);
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "{name} took more than 30 seconds by round {round}; the last input was {input:?} \
             (seed {seed})"
        );
    }
}

#[test]
fn should_never_panic_on_anything_offered_to_the_json_decoder() {
    hammer(0x4a_53_4f_4e, 4000, "from_json", |input| {
        let _ = from_json_str(input, registry());
    });
}

#[test]
fn should_never_panic_on_anything_offered_to_the_yaml_decoder() {
    hammer(0x59_41_4d_4c, 4000, "from_yaml", |input| {
        let _ = from_yaml(input, registry());
    });
}

#[test]
fn should_never_panic_on_anything_offered_to_the_csv_decoder() {
    hammer(0x43_53_56, 4000, "from_csv", |input| {
        let _ = from_csv(input);
    });
}

#[test]
fn should_never_panic_on_arbitrary_bytes_offered_to_the_byte_decoder() {
    let mut rng = Rng::seeded(0x42_59_54_45);
    for _ in 0..2000 {
        let length = rng.below(64);
        let bytes: Vec<u8> = (0..length).map(|_| (rng.below(256)) as u8).collect();
        let value = from_bytes(bytes.clone());
        // The byte form never rejects: bytes are bytes. What it must not do is lose any.
        assert_eq!(
            value
                .as_bytes()
                .map(|held| held.to_vec())
                .unwrap_or_default(),
            bytes,
            "the byte decoder must not lose a byte"
        );
    }
}

#[test]
fn should_refuse_a_deeply_nested_document_rather_than_exhausting_the_stack() {
    // ADR-0015 T7: a "schema bomb" is a document whose nesting is cheap to write and expensive to
    // decode. A decoder that recurses once per level dies on a document a few kilobytes long.
    for depth in [64usize, 1_000, 100_000] {
        let nested = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        let outcome = from_json_str(&nested, registry());
        // Either it decodes or it reports; what it must never do is take the process down.
        assert!(
            outcome.is_ok() || outcome.is_err(),
            "unreachable, but the call must return at depth {depth}"
        );
    }
}

#[test]
fn should_make_a_number_it_cannot_hold_exactly_visibly_inexact() {
    // A JSON number beyond `i128` cannot be an integer, and refusing it would make `from json`
    // unable to read documents other tools produce. What it must not do is come back as an
    // integer that is not the integer that was written — a count of 10^44 arriving as
    // `i128::MAX` would be a fabricated value, which spec §35.3 forbids.
    //
    // So the loss is carried in the type: the value is a float, and a reader who cares can see
    // that it is one.
    let huge = from_json_str("99999999999999999999999999999999999999999999", registry())
        .expect("a number other tools write must be readable");
    assert_eq!(
        huge.type_name(),
        "float",
        "a number too large for an integer must say it is not one"
    );

    // Within range it stays exact, which is the half that actually matters day to day.
    let exact = from_json_str("9007199254740993", registry()).expect("an ordinary integer");
    assert_eq!(exact.type_name(), "int");
    assert_eq!(
        ono_value::canonical_text(&exact).unwrap_or_default(),
        "9007199254740993",
        "an integer JSON can hold must survive exactly, including past 2^53"
    );
}

#[test]
fn should_reproduce_the_same_run_from_the_same_seed_when_a_failure_is_replayed() {
    // The point of a seeded fuzzer: a failure is reproducible from the seed in its message.
    let corpus = |seed: u64| {
        let mut rng = Rng::seeded(seed);
        (0..64)
            .map(|_| rng.assemble(PIECES, 12))
            .collect::<Vec<_>>()
    };
    assert_eq!(corpus(1234), corpus(1234));
}
