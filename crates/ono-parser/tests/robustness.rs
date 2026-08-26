//! Deterministic pseudo-random inputs and the parse budget of spec §34.

#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use std::time::Instant;

use ono_parser::{parse, tokens};

/// A fixed-seed xorshift so the corpus is identical on every machine and every run.
struct Xorshift(u64);

impl Xorshift {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn pick<'a>(&mut self, items: &'a [&'a str]) -> &'a str {
        items[(self.next() % items.len() as u64) as usize]
    }
}

const ALPHABET: &[&str] = &[
    " ",
    "|",
    "||",
    "&&",
    "&",
    ";",
    "\n",
    "(",
    ")",
    "[",
    "]",
    "{",
    "}",
    ",",
    ":",
    "\"",
    "'",
    "#",
    "$",
    "$name",
    "@",
    "@-1",
    "@3",
    "ls",
    "-la",
    "where",
    "select",
    "get",
    "process",
    "cpu",
    ">",
    ">>",
    "<",
    "2>",
    "2>&1",
    "==",
    "!=",
    "~=",
    "!~=",
    "<=",
    ">=",
    "+",
    "-",
    "*",
    "/",
    "%",
    "=",
    "=>",
    "->",
    ".",
    "?.",
    "not",
    "and",
    "or",
    "in",
    "true",
    "false",
    "null",
    "let",
    "fn",
    "if",
    "else",
    "for",
    "while",
    "match",
    "try",
    "catch",
    "return",
    "break",
    "continue",
    "use",
    "20",
    "512MiB",
    "7d",
    "95%",
    "0x1f",
    "1.5",
    "\"a\\q\"",
    "'raw'",
    "/re/i",
    "\u{1F600}",
    "\\",
];

#[test]
fn should_survive_a_pseudo_random_corpus_without_panicking() {
    let mut random = Xorshift(0x2026_0826_0000_0009);
    for _ in 0..4000 {
        let length = (random.next() % 24) as usize;
        let mut source = String::new();
        for _ in 0..length {
            source.push_str(random.pick(ALPHABET));
        }
        let parsed = parse(&source);
        for diagnostic in parsed.diagnostics() {
            assert!(
                diagnostic.span().end() as usize <= source.len(),
                "{source:?} produced an out-of-range span {:?}",
                diagnostic.span()
            );
        }
        let mut previous_end = 0;
        for token in tokens(&source) {
            assert!(token.span.start() >= previous_end, "{source:?}");
            assert!(token.span.end() as usize <= source.len(), "{source:?}");
            previous_end = token.span.end();
        }
    }
}

#[test]
fn should_not_overflow_the_stack_when_the_input_nests_deeply() {
    for opener in ["(", "[", "{", "((((((((", "$name."] {
        let source = opener.repeat(2000);
        let parsed = parse(&source);
        for diagnostic in parsed.diagnostics() {
            assert!(
                diagnostic.span().end() as usize <= source.len(),
                "deeply nested input produced an out-of-range span"
            );
        }
    }
}

#[test]
fn should_stay_far_inside_the_parse_budget_when_an_ordinary_line_is_parsed() {
    let line = "get process | where cpu > 20 and memory >= 512MiB | sort cpu desc | take 10";
    // Warm up so the measurement is not dominated by first-call effects.
    for _ in 0..50 {
        let _ = parse(line);
    }
    let started = Instant::now();
    for _ in 0..1000 {
        let parsed = parse(line);
        assert!(parsed.diagnostics().is_empty());
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed.as_millis() < 500,
        "1000 parses of an ordinary line took {elapsed:?}; spec §34 budgets under 5 ms for one"
    );
}
