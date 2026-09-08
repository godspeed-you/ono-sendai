//! Time selectors: every form of v0.5 §4.4, every rejection, and the DST cases §4.4 and §25.6
//! single out — "ambiguous local times during DST transitions MUST require disambiguation by
//! offset when both instants exist".
//!
//! Resolution is pure: the zone and `now` are parameters, so these tests are the evidence for
//! §39.2's "no clock in pure logic crates".

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

mod common;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use ono_core::ErrorCode;
use ono_temporal_core::{EventAnchors, EventId, TimeResolution, TimeSelector};

use common::instant;

struct NoAnchors;

impl EventAnchors for NoAnchors {
    fn instant_of(&self, _id: &EventId) -> Option<Timestamp> {
        None
    }
}

struct OneAnchor(EventId, Timestamp);

impl EventAnchors for OneAnchor {
    fn instant_of(&self, id: &EventId) -> Option<Timestamp> {
        (id == &self.0).then_some(self.1)
    }
}

fn berlin() -> TimeZone {
    TimeZone::get("Europe/Berlin").expect("the tz database carries Europe/Berlin")
}

fn resolve(text: &str, now: &str) -> TimeResolution {
    TimeSelector::parse(text)
        .expect("the selector parses")
        .resolve(&berlin(), instant(now), &NoAnchors)
        .expect("the selector resolves")
}

#[test]
fn should_resolve_to_the_named_instant_when_the_selector_is_absolute() {
    assert_eq!(
        resolve("2026-08-31T12:17:00+02:00", "2026-08-31T14:00:00Z"),
        TimeResolution::Resolved(instant("2026-08-31T10:17:00Z")),
        "an RFC 3339 selector carries its own offset and needs no zone"
    );
}

#[test]
fn should_resolve_in_the_session_zone_when_the_selector_is_a_local_date_time() {
    assert_eq!(
        resolve("2026-08-31 12:17:00", "2026-08-31T14:00:00Z"),
        TimeResolution::Resolved(instant("2026-08-31T10:17:00Z")),
        "a local date-time is read in the zone the session was given"
    );
}

#[test]
fn should_resolve_against_today_when_the_selector_is_a_local_time() {
    assert_eq!(
        resolve("12:17:00", "2026-08-31T14:00:00Z"),
        TimeResolution::Resolved(instant("2026-08-31T10:17:00Z")),
        "a bare local time means today, in the session zone (§4.4)"
    );
}

#[test]
fn should_resolve_relative_to_now_when_the_selector_is_a_negative_duration() {
    assert_eq!(
        resolve("-10m", "2026-08-31T12:17:14Z"),
        TimeResolution::Resolved(instant("2026-08-31T12:07:14Z")),
        "§4.2's own example: `at -10m` moves the coordinate back ten minutes"
    );
}

#[test]
fn should_resolve_to_the_anchor_instant_when_the_selector_names_an_event() {
    let anchor = EventId::parse("@e0123456789abcdef01234567").expect("a full event id parses");
    let selector = TimeSelector::parse("event @e0123456789abcdef01234567").expect("it parses");
    let anchors = OneAnchor(anchor, instant("2026-08-31T11:03:00Z"));
    assert_eq!(
        selector
            .resolve(&berlin(), instant("2026-08-31T14:00:00Z"), &anchors)
            .expect("the anchor resolves"),
        TimeResolution::Resolved(instant("2026-08-31T11:03:00Z")),
        "§12.2: `at event` resolves through the event's own instant"
    );
}

#[test]
fn should_refuse_the_selector_when_a_relative_time_names_the_future() {
    let refused = TimeSelector::parse("+10m").expect_err("a future selector is invalid (§4.4)");
    assert_eq!(refused.code(), ErrorCode::TemporalInvalidTime);
    assert!(
        refused.message().contains("+10m"),
        "the message names what was typed: {}",
        refused.message()
    );
}

#[test]
fn should_refuse_the_selector_when_a_bare_duration_names_the_future() {
    assert_eq!(
        TimeSelector::parse("10m")
            .expect_err("an unsigned duration is a future selector")
            .code(),
        ErrorCode::TemporalInvalidTime
    );
}

#[test]
fn should_refuse_the_selector_when_it_is_empty() {
    assert_eq!(
        TimeSelector::parse("   ")
            .expect_err("an empty selector names no time")
            .code(),
        ErrorCode::TemporalInvalidTime
    );
}

#[test]
fn should_refuse_the_selector_when_it_is_not_a_time_at_all() {
    for text in [
        "yesterday",
        "2026-13-45 99:99",
        "event @zz",
        "-10 parsecs",
        "12:17 tomorrow",
    ] {
        let refused = TimeSelector::parse(text);
        assert!(
            refused.is_err(),
            "`{text}` is not a time selector and must be refused"
        );
        assert_eq!(
            refused.err().map(|error| error.code()),
            Some(ErrorCode::TemporalInvalidTime),
            "`{text}` is refused as an invalid time"
        );
    }
}

#[test]
fn should_refuse_the_selector_when_it_names_a_date_without_a_time() {
    assert_eq!(
        TimeSelector::parse("2026-08-31")
            .expect_err("§4.4 has no date-only selector")
            .code(),
        ErrorCode::TemporalInvalidTime
    );
}

#[test]
fn should_report_both_instants_when_a_local_time_falls_in_a_dst_fold() {
    // Europe/Berlin leaves summer time on 2026-10-25: 03:00 CEST becomes 02:00 CET, so 02:30
    // happens twice. §4.4 requires disambiguation by offset, which needs both instants named.
    let resolution = resolve("2026-10-25 02:30:00", "2026-10-25T12:00:00Z");
    assert_eq!(
        resolution,
        TimeResolution::Ambiguous {
            earlier: instant("2026-10-25T00:30:00Z"),
            later: instant("2026-10-25T01:30:00Z"),
        },
        "a folded wall time resolves to two instants, both named"
    );
}

#[test]
fn should_report_the_skipped_span_when_a_local_time_falls_in_a_dst_gap() {
    // Europe/Berlin enters summer time on 2026-03-29: 02:00 CET becomes 03:00 CEST, so 02:30
    // never happens.
    let resolution = resolve("2026-03-29 02:30:00", "2026-03-29T12:00:00Z");
    assert_eq!(
        resolution,
        TimeResolution::Skipped {
            gap_from: instant("2026-03-29T00:30:00Z"),
            gap_until: instant("2026-03-29T01:30:00Z"),
        },
        "a skipped wall time resolves to neither instant, and says which span swallowed it"
    );
}

#[test]
fn should_carry_both_instants_in_the_error_when_an_ambiguous_local_time_reaches_a_command() {
    let TimeResolution::Ambiguous { earlier, later } =
        resolve("2026-10-25 02:30:00", "2026-10-25T12:00:00Z")
    else {
        panic!("the fold is ambiguous");
    };
    let refused =
        ono_temporal_core::error::ambiguous_local_time("2026-10-25 02:30:00", earlier, later);
    assert_eq!(refused.code(), ErrorCode::TemporalInvalidTime);
    for expected in ["2026-10-25T00:30:00Z", "2026-10-25T01:30:00Z"] {
        assert!(
            refused.render_full().contains(expected),
            "the refusal names both instants so an offset can disambiguate: {}",
            refused.render_full()
        );
    }
}

#[test]
fn should_never_read_the_system_clock_when_resolving_a_relative_selector() {
    // The same selector resolved against two different `now` values gives two different answers,
    // which is only possible because `now` is a parameter (§39.2).
    assert_eq!(
        resolve("-1h", "2020-01-01T00:00:00Z"),
        TimeResolution::Resolved(instant("2019-12-31T23:00:00Z"))
    );
    assert_eq!(
        resolve("-1h", "2030-01-01T00:00:00Z"),
        TimeResolution::Resolved(instant("2029-12-31T23:00:00Z"))
    );
}
