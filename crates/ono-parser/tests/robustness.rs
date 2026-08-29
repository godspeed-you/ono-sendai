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

#[test]
fn should_refuse_deeply_nested_blocks_rather_than_overflowing_the_stack() {
    // A block contains statements and a statement contains a block, so statement nesting recurses
    // just as expression nesting does. Unguarded, `if true { if true { … } }` a couple of thousand
    // deep aborted the process — and this parser runs on every keystroke in the editor, so that is
    // one pasted line away from killing a login shell.
    //
    // The earlier test named for this repeated `{` 2000 times, which never enters block recursion
    // at all: it passed while the thing it named was broken.
    for depth in [500usize, 3_000, 20_000] {
        let source = format!(
            "if true {}{}",
            "{ if true ".repeat(depth),
            "}".repeat(depth + 1)
        );
        let parsed = ono_parser::parse(&source);
        // What matters is that it returns at all, with a tree and a diagnostic rather than a
        // crash. Whether it is called incomplete or wrong is the parser's business.
        assert!(
            !parsed.program().statements.is_empty() || !parsed.diagnostics().is_empty(),
            "a {depth}-deep nesting produced neither a tree nor a diagnostic"
        );
    }
}

#[test]
fn should_refuse_deeply_nested_function_bodies_and_loops_too() {
    for opener in ["fn f() ", "while true ", "for x in [1] "] {
        let source = format!("{}{}{}", opener, "{ ".repeat(5_000), "}".repeat(5_000));
        let parsed = ono_parser::parse(&source);
        assert!(
            !parsed.diagnostics().is_empty() || !parsed.program().statements.is_empty(),
            "{opener} nested deeply produced nothing at all"
        );
    }
}

#[test]
fn should_stay_linear_on_a_wall_of_unbalanced_parentheses() {
    // A pasted line is attacker-sized input (ADR-0015: parser super-linear behaviour on hostile
    // input), and the editor re-parses per keystroke. Twenty thousand unmatched parentheses
    // were quadratic — 25 seconds in a debug build — which is a denial of service typed with
    // one key held down. The bound here is generous enough for any machine and two orders
    // below where the quadratic curve lands.
    let hostile = "(".repeat(20_000);
    let start = Instant::now();
    let parsed = parse(&hostile);
    let elapsed = start.elapsed();
    assert!(
        parsed.has_errors(),
        "an unbalanced wall is an error, not a hang"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "20k unbalanced parens took {elapsed:?}; the parser must degrade linearly"
    );
}

#[test]
fn should_refuse_deeply_nested_index_and_call_suffixes_rather_than_overflowing_the_stack() {
    // Found by the §35.6 parser fuzz target (ADR-0313) on its first campaign, in a mutation of
    // the `for x in [1, 2, 3] { … }` seed: `[1` repeated aborted the process at around five
    // thousand levels.
    //
    // The depth counter was released too early. `parse_primary` raises it around its own body
    // and lowers it before returning; the postfix loop then recurses into the index expression
    // — `1[1[1[…` — from outside that window, so a chain of suffixes nested without the counter
    // ever rising. The two earlier nesting tests miss it because a repeated `[` never reaches
    // the postfix loop at all: an index needs an operand in front of it.
    for source in [
        "[1".repeat(20_000),
        "f(f".repeat(20_000),
        format!("x{}", "[0]".repeat(20_000)),
        // A prefix operator recursed into itself with the counter never rising either — found
        // by the same target, in a mutation of the `get process … | select …` seed.
        "- ".repeat(20_000),
        "not ".repeat(20_000),
    ] {
        let parsed = ono_parser::parse(&source);
        assert!(
            !parsed.program().statements.is_empty() || !parsed.diagnostics().is_empty(),
            "a chain of suffixes produced neither a tree nor a diagnostic"
        );
        for diagnostic in parsed.diagnostics() {
            assert!(
                diagnostic.span().end() as usize <= source.len(),
                "deeply suffixed input produced an out-of-range span"
            );
        }
    }
}
