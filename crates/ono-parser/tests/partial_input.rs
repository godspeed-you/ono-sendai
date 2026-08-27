//! Incremental parse: every prefix of a valid line must parse as unfinished, never as wrong.
//!
//! Spec §24.4 requires the editor to parse a line while it is being typed, and ADR-0009 makes
//! that observable through the `parse.incomplete` / `parse.syntax` split.

#![allow(
    clippy::panic,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "a failed precondition in a test should abort the test loudly"
)]

use ono_core::ErrorCode;
use ono_parser::parse;

const LINES: &[&str] = &[
    "ls -la",
    "git commit -m \"wip\"",
    "get process 4419 --tree",
    "get process | where cpu > 20 | sort cpu desc | take 10",
    "get file ./src --recursive | where size > 1MiB | select path size modified",
    "get socket | where state == established | group remote.host",
    "where memory >= 512MiB and modified < now() - 7d",
    "where name ~= /postgres|redis/i",
    "where user in [\"root\", \"postgres\"]",
    "where remote?.port == 443 and not disabled",
    "cat a.txt > out.txt",
    "cat a.txt >> out.txt",
    "wc -l < in.txt",
    "cmd 2>&1 | grep x",
    "cmd 2>> err.log",
    "sleep 5 &",
    "a && b || c",
    "echo (get process | count)",
    "echo \"count is $(get process | count) at $env.HOME\"",
    "ono:get process | to json > out.json",
    "let hot = get process | where cpu > 50",
    "let n: Int = 5",
    "fn hot-processes(limit: Float = 20) -> Stream<Process> { get process | where cpu > limit }",
    "if a > 1 { restart service nginx } else if a > 0 { warn } else { stop }",
    "for p in $hot { echo $p }",
    "while a < 10 { step }",
    "match state { \"running\" => ok, failed => { restart }, _ => skip }",
    "try { risky } catch err { report $err }",
    "get service | where state == failed | each { restart service @ }",
    "use ono:process",
    "return 1",
];

#[test]
fn should_parse_every_prefix_of_a_valid_line_without_calling_it_wrong() {
    for line in LINES {
        for end in line
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(line.len()))
        {
            let prefix = &line[..end];
            let parsed = parse(prefix);
            for diagnostic in parsed.diagnostics() {
                assert_ne!(
                    diagnostic.code(),
                    ErrorCode::ParseSyntax,
                    "prefix {prefix:?} of {line:?} must not be a syntax error: {diagnostic:?}"
                );
                assert!(
                    diagnostic.span().end() as usize <= prefix.len(),
                    "prefix {prefix:?} produced an out-of-range span"
                );
            }
        }
    }
}

#[test]
fn should_parse_every_complete_line_without_any_diagnostic() {
    for line in LINES {
        let parsed = parse(line);
        assert!(
            parsed.diagnostics().is_empty(),
            "{line:?} must parse cleanly, got {:?}",
            parsed.diagnostics()
        );
        assert!(parsed.is_complete(), "{line:?} must be complete");
        assert!(!parsed.has_errors(), "{line:?} must be free of errors");
    }
}

#[test]
fn should_report_the_line_as_incomplete_while_a_construct_is_still_open() {
    for (prefix, expected_open) in [
        ("get process | where cpu > 20 |", true),
        ("echo (get process", true),
        ("git commit -m \"wip", true),
        ("get process | where cpu > 20", false),
    ] {
        let parsed = parse(prefix);
        assert_eq!(
            !parsed.is_complete(),
            expected_open,
            "{prefix:?} reported the wrong completeness: {:?}",
            parsed.diagnostics()
        );
    }
}

#[test]
fn should_keep_the_token_stream_aligned_with_the_source_for_every_prefix() {
    for line in LINES {
        for end in line
            .char_indices()
            .map(|(index, _)| index)
            .chain(std::iter::once(line.len()))
        {
            let prefix = &line[..end];
            let mut previous_end = 0;
            for token in ono_parser::tokens(prefix) {
                assert!(
                    token.span.start() >= previous_end,
                    "tokens of {prefix:?} must be ordered and non-overlapping"
                );
                assert!(
                    token.span.end() as usize <= prefix.len(),
                    "token {token:?} of {prefix:?} runs past the end of the source"
                );
                previous_end = token.span.end();
            }
        }
    }
}
