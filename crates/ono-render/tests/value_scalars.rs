//! Scalar rendering, exactly as spec §13.4 writes it.
//!
//! The value model owns what a value *is*; this crate owns what it *looks like*. These tests
//! assert the looks and nothing else — and that looking at a value never changes it.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the way a test does"
)]

use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use jiff::Timestamp;
use jiff::tz::TimeZone;
use ono_core::ErrorCode;
use ono_render::{Presentation, Renderer, Theme, Token};
use ono_value::{ByteSize, Duration, ErrorValue, Percent, Value};

fn at(text: &str) -> Timestamp {
    text.parse().unwrap()
}

fn utc_at(now: &str) -> Renderer {
    Renderer::in_zone(TimeZone::UTC).at(at(now))
}

#[test]
fn should_render_a_byte_size_in_its_human_unit() {
    let cell = Renderer::in_zone(TimeZone::UTC)
        .cell(&Value::ByteSize(ByteSize::from_bytes(1_288_490_188)));
    assert_eq!(cell.text(), "1.20 GiB");
}

#[test]
fn should_render_a_long_duration_in_days_and_hours() {
    let cell = Renderer::in_zone(TimeZone::UTC)
        .cell(&Value::Duration(Duration::parse("4d 3h 12m").unwrap()));
    assert_eq!(cell.text(), "4d 03h");
}

#[test]
fn should_render_a_short_duration_in_milliseconds() {
    let cell =
        Renderer::in_zone(TimeZone::UTC).cell(&Value::Duration(Duration::parse("843ms").unwrap()));
    assert_eq!(cell.text(), "843ms");
}

#[test]
fn should_render_a_percentage_with_its_sign() {
    let cell = Renderer::in_zone(TimeZone::UTC).cell(&Value::Percent(Percent::new(24.8)));
    assert_eq!(cell.text(), "24.8%");
}

#[test]
fn should_render_a_timestamp_as_a_time_when_it_falls_on_the_reader_s_day() {
    let cell = utc_at("2026-08-26T18:00:00Z").cell(&Value::Timestamp(at("2026-08-26T14:32:05Z")));
    assert_eq!(cell.text(), "14:32:05");
}

#[test]
fn should_render_a_timestamp_as_a_month_and_time_when_it_falls_in_the_reader_s_year() {
    let cell = utc_at("2026-08-26T18:00:00Z").cell(&Value::Timestamp(at("2026-03-11T14:32:05Z")));
    assert_eq!(cell.text(), "Mar 11 14:32");
}

#[test]
fn should_render_a_timestamp_as_a_date_when_it_is_older_than_the_reader_s_year() {
    let cell = utc_at("2026-08-26T18:00:00Z").cell(&Value::Timestamp(at("2024-03-11T14:32:05Z")));
    assert_eq!(cell.text(), "2024-03-11");
}

#[test]
fn should_render_a_timestamp_in_full_when_no_reference_instant_was_given() {
    let cell = Renderer::in_zone(TimeZone::UTC).cell(&Value::Timestamp(at("2026-03-11T14:32:05Z")));
    assert_eq!(cell.text(), "2026-03-11 14:32:05 +00:00");
}

#[test]
fn should_render_a_timestamp_in_the_reader_s_zone() {
    let berlin = TimeZone::get("Europe/Berlin").unwrap();
    let cell = Renderer::in_zone(berlin)
        .at(at("2026-08-26T18:00:00Z"))
        .cell(&Value::Timestamp(at("2026-08-26T14:32:05Z")));
    assert_eq!(
        cell.text(),
        "16:32:05",
        "Berlin is two hours ahead in August"
    );
}

#[test]
fn should_keep_the_full_instant_available_after_it_has_been_rendered() {
    let value = Value::Timestamp(at("2026-03-11T14:32:05.182Z"));
    let short = utc_at("2026-08-26T18:00:00Z").cell(&value);
    assert_eq!(short.text(), "Mar 11 14:32");
    assert_eq!(
        value,
        Value::Timestamp(at("2026-03-11T14:32:05.182Z")),
        "spec §13.1: laying a value out must never change it"
    );
}

#[test]
fn should_render_an_ip_address_in_its_canonical_form() {
    let renderer = Renderer::in_zone(TimeZone::UTC);
    assert_eq!(
        renderer
            .cell(&Value::Ip("2001:db8:0:0:0:0:0:1".parse().unwrap()))
            .text(),
        "2001:db8::1"
    );
    assert_eq!(
        renderer
            .cell(&Value::Ip("192.0.2.7".parse().unwrap()))
            .text(),
        "192.0.2.7"
    );
}

#[test]
fn should_render_null_as_the_word_null() {
    let cell = Renderer::in_zone(TimeZone::UTC).cell(&Value::Null);
    assert_eq!(
        cell.text(),
        "null",
        "spec §10.5: absence is information and must be visible"
    );
    assert_eq!(cell.token(), Token::ValueNull);
    assert!(!cell.text().is_empty());
}

#[test]
fn should_paint_every_scalar_with_its_own_semantic_token() {
    let renderer = Renderer::in_zone(TimeZone::UTC);
    let cases: [(Value, Token); 6] = [
        (Value::Int(1), Token::ValueNumber),
        (Value::ByteSize(ByteSize::from_bytes(1)), Token::ValueUnit),
        (Value::string("nginx"), Token::ValueString),
        (Value::Path(Arc::from(Path::new("/etc"))), Token::Path),
        (
            Value::Timestamp(at("2026-03-11T14:32:05Z")),
            Token::Timestamp,
        ),
        (Value::Null, Token::ValueNull),
    ];
    for (value, token) in cases {
        assert_eq!(renderer.cell(&value).token(), token, "for {value}");
    }
}

#[test]
fn should_make_an_escape_sequence_in_a_value_inert() {
    let hostile = Value::string("nginx\u{1b}[2Joops");
    let cell = Renderer::in_zone(TimeZone::UTC).cell(&hostile);
    assert!(
        !cell.text().contains('\u{1b}'),
        "spec §49: rendered data must never be able to drive the terminal, got {:?}",
        cell.text()
    );
    assert!(cell.text().contains("\\u{1b}"), "got {:?}", cell.text());
    assert_eq!(
        hostile,
        Value::string("nginx\u{1b}[2Joops"),
        "spec §49: the raw value is retained, only the display is neutralised"
    );
}

#[test]
fn should_render_bytes_as_hex_rather_than_a_lossy_decode() {
    let cell = Renderer::in_zone(TimeZone::UTC)
        .cell(&Value::Bytes(Bytes::from_static(&[0xff, 0x00, 0xfe])));
    assert_eq!(cell.text(), "ff00fe");
}

#[test]
fn should_render_an_error_value_with_its_code() {
    let error = ErrorValue::new(ErrorCode::IoPermissionDenied, "access denied").into_value();
    let cell = Renderer::in_zone(TimeZone::UTC).cell(&error);
    assert_eq!(cell.text(), "io.permission_denied: access denied");
    assert_eq!(cell.token(), Token::ErrorCode);
}

#[test]
fn should_emit_no_escape_sequence_when_the_destination_takes_no_colour() {
    let theme = Theme::default();
    for presentation in [
        Presentation::Plain,
        Presentation::Pipe,
        Presentation::Redirect,
        Presentation::Script,
    ] {
        let painted = theme.paint("1.20 GiB", Token::ValueUnit, presentation);
        assert!(!painted.contains('\u{1b}'), "for {presentation:?}");
    }
}
