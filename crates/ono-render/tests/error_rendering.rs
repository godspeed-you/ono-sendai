//! Error rendering, exactly as spec §16.2 writes it: terse by default, rich on demand.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the way a test does"
)]

use std::path::Path;

use ono_core::ErrorCode;
use ono_render::{Detail, Layout, Presentation, Theme};
use ono_value::{ErrorValue, Value, ValueRef};

fn denied() -> ErrorValue {
    ErrorValue::new(ErrorCode::IoPermissionDenied, "access denied")
        .with_target(ValueRef::path(Path::new("/etc/shadow")))
        .with_help("requires root or read capability")
        .with_metadata("errno", Value::Int(13))
        .with_source(ErrorValue::new(
            ErrorCode::ProviderUnavailable,
            "procfs is not mounted",
        ))
}

#[test]
fn should_render_an_error_terse_by_default() {
    let lines = Layout::new(80).render_error(
        &denied(),
        Detail::Terse,
        &Theme::default(),
        Presentation::Pipe,
    );
    assert_eq!(
        lines,
        [
            "access denied: /etc/shadow",
            "requires root or read capability"
        ],
        "spec §16.2 writes exactly these two lines"
    );
}

#[test]
fn should_render_the_whole_causal_chain_when_asked() {
    let lines = Layout::new(120).render_error(
        &denied(),
        Detail::Full,
        &Theme::default(),
        Presentation::Pipe,
    );
    let text = lines.join("\n");
    assert!(text.contains("Ono-Sendai-E0302"), "got {text}");
    assert!(text.contains("io.permission_denied"), "got {text}");
    assert!(text.contains("errno = 13"), "got {text}");
    assert!(
        text.contains("procfs is not mounted"),
        "`inspect @error` reveals the causal chain, got {text}"
    );
}

#[test]
fn should_never_render_an_error_wider_than_the_terminal() {
    for width in [40, 80, 200] {
        for line in Layout::new(width).render_error(
            &denied(),
            Detail::Full,
            &Theme::default(),
            Presentation::Pipe,
        ) {
            assert!(
                unicode_width::UnicodeWidthStr::width(line.as_str()) <= width,
                "at {width}: {line:?}"
            );
        }
    }
}

#[test]
fn should_emit_no_escape_sequences_when_the_destination_takes_no_colour() {
    for detail in [Detail::Terse, Detail::Full] {
        for line in
            Layout::new(80).render_error(&denied(), detail, &Theme::default(), Presentation::Plain)
        {
            assert!(!line.contains('\u{1b}'), "{line:?}");
        }
    }
}

#[test]
fn should_paint_an_error_on_a_terminal() {
    let lines = Layout::new(80).render_error(
        &denied(),
        Detail::Terse,
        &Theme::default(),
        Presentation::Terminal,
    );
    assert!(lines[0].contains('\u{1b}'), "got {:?}", lines[0]);
}

#[test]
fn should_make_an_escape_sequence_in_an_error_message_inert() {
    let hostile = ErrorValue::new(ErrorCode::IoNotFound, "missing: \u{1b}[2Joops");
    let lines = Layout::new(80).render_error(
        &hostile,
        Detail::Terse,
        &Theme::default(),
        Presentation::Pipe,
    );
    assert!(!lines[0].contains('\u{1b}'), "got {:?}", lines[0]);
}
