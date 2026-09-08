//! Time selectors (v0.5 §4.4) and their resolution.
//!
//! Two rules shape this module. §4.4: "ambiguous local times during DST transitions MUST require
//! disambiguation by offset when both instants exist" — so resolution has three outcomes rather
//! than one, and a fold produces both instants by name. §39.2: pure logic takes time as a
//! parameter — so [`TimeSelector::resolve`] is handed the zone and `now`, and nothing here reads
//! a clock. That is what makes a historical answer reproducible in a test (§47.1).

use jiff::civil;
use jiff::tz::{AmbiguousOffset, TimeZone};
use jiff::{Timestamp, Zoned};
use ono_value::{Duration, ErrorValue};

use crate::error;
use crate::id::EventId;

/// One of the five forms §4.4 defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeSelector {
    /// An RFC 3339 instant, carrying its own offset: `2026-08-31T12:17:00+02:00`.
    Absolute(Timestamp),
    /// A local date and time, read in the session zone: `2026-08-31 12:17:00`.
    LocalDateTime(civil::DateTime),
    /// A local time today, read in the session zone: `12:17:00`.
    LocalTime(civil::Time),
    /// A span back from now: `-10m`. §4.4 makes a future relative selector invalid.
    Relative(Duration),
    /// An event to resolve through: `event @e42` (§12.2).
    Event(EventId),
}

/// Where a selector resolved to (§4.4, §25.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeResolution {
    /// One instant, unambiguously.
    Resolved(Timestamp),
    /// A local wall time that exists twice because a DST fold covers it.
    ///
    /// §4.4 requires disambiguation by offset when both instants exist, so both are named and
    /// the caller refuses with `temporal.invalid_time` rather than guessing.
    Ambiguous {
        /// The instant under the offset in force before the fold.
        earlier: Timestamp,
        /// The instant under the offset in force after it.
        later: Timestamp,
    },
    /// A local wall time that never happened because a DST gap skipped it.
    ///
    /// The two instants bracket the skipped span: the requested wall time falls between them and
    /// belongs to neither.
    Skipped {
        /// The instant the requested wall time maps to under the offset after the jump.
        gap_from: Timestamp,
        /// The instant it maps to under the offset before it.
        gap_until: Timestamp,
    },
}

impl TimeResolution {
    /// The single instant, where there is one.
    #[must_use]
    pub const fn instant(&self) -> Option<Timestamp> {
        match self {
            TimeResolution::Resolved(instant) => Some(*instant),
            TimeResolution::Ambiguous { .. } | TimeResolution::Skipped { .. } => None,
        }
    }
}

/// Where `at event @e42` finds the instant of an event (§12.2).
///
/// A ledger implements it. The selector does not reach for one itself, because §39.2 keeps this
/// crate free of anything that goes and looks.
pub trait EventAnchors {
    /// The instant of `id`, or `None` where the ledger holds no such event.
    fn instant_of(&self, id: &EventId) -> Option<Timestamp>;
}

impl TimeSelector {
    /// Reads one of §4.4's five forms.
    ///
    /// # Errors
    ///
    /// Returns `temporal.invalid_time` (§34) for anything §4.4 does not define, and for a
    /// relative selector naming the future, which §4.4 forbids for historical context.
    pub fn parse(text: &str) -> Result<Self, ErrorValue> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(error::invalid_time(text, "a time selector cannot be empty"));
        }

        if let Some(rest) = trimmed.strip_prefix("event ") {
            return EventId::parse(rest.trim())
                .map(TimeSelector::Event)
                .ok_or_else(|| {
                    error::invalid_time(
                        text,
                        "`event` must be followed by an event reference like `@e42`",
                    )
                });
        }
        if trimmed.starts_with('@') {
            return EventId::parse(trimmed)
                .map(TimeSelector::Event)
                .ok_or_else(|| error::invalid_time(text, "that is not an event reference"));
        }

        if trimmed.starts_with('-') || trimmed.starts_with('+') {
            let span = Duration::parse(trimmed.trim_start_matches('+')).map_err(|_| {
                error::invalid_time(text, "a relative selector is a span like `-10m`")
            })?;
            return relative(text, span);
        }

        if looks_like_a_date(trimmed) {
            if let Ok(instant) = trimmed.parse::<Timestamp>() {
                return Ok(TimeSelector::Absolute(instant));
            }
            let normalised = trimmed.replacen(' ', "T", 1);
            if let Ok(local) = normalised.parse::<civil::DateTime>() {
                return Ok(TimeSelector::LocalDateTime(local));
            }
            return Err(error::invalid_time(
                text,
                "§4.4 accepts `2026-08-31T12:17:00+02:00` or `2026-08-31 12:17:00`",
            ));
        }

        if let Ok(local) = parse_local_time(trimmed) {
            return Ok(TimeSelector::LocalTime(local));
        }

        // A bare span such as `10m` parses as a duration and names the future, which §4.4 makes
        // invalid; saying so is more use than "unrecognised".
        if let Ok(span) = Duration::parse(trimmed) {
            return relative(text, span);
        }

        Err(error::invalid_time(
            text,
            "§4.4 defines `2026-08-31T12:17:00+02:00`, `2026-08-31 12:17:00`, `12:17:00`, `-10m` and `event @e42`",
        ))
    }

    /// Resolves the selector against a zone, an instant that stands for now, and a ledger.
    ///
    /// Pure by contract: `zone` and `now` are parameters and this crate never reads either from
    /// the system (§39.2).
    ///
    /// # Errors
    ///
    /// Returns `temporal.invalid_time` where a relative selector names the future, where an
    /// event reference resolves to nothing, or where the arithmetic leaves the representable
    /// range.
    pub fn resolve(
        &self,
        zone: &TimeZone,
        now: Timestamp,
        anchors: &dyn EventAnchors,
    ) -> Result<TimeResolution, ErrorValue> {
        match self {
            TimeSelector::Absolute(instant) => Ok(TimeResolution::Resolved(*instant)),
            TimeSelector::Relative(span) => {
                if span.nanoseconds() > 0 {
                    return Err(error::invalid_time(
                        &span.exact(),
                        "§4.4: relative future selectors are invalid for historical context",
                    ));
                }
                let nanos = now
                    .as_nanosecond()
                    .checked_add(span.nanoseconds())
                    .ok_or_else(|| {
                        error::invalid_time(
                            &span.exact(),
                            "that span reaches outside recorded time",
                        )
                    })?;
                Timestamp::from_nanosecond(nanos)
                    .map(TimeResolution::Resolved)
                    .map_err(|_| {
                        error::invalid_time(
                            &span.exact(),
                            "that span reaches outside recorded time",
                        )
                    })
            }
            TimeSelector::LocalDateTime(local) => Ok(resolve_local(zone, *local)),
            TimeSelector::LocalTime(local) => {
                let today = Zoned::new(now, zone.clone()).date();
                Ok(resolve_local(zone, today.to_datetime(*local)))
            }
            TimeSelector::Event(id) => anchors
                .instant_of(id)
                .map(TimeResolution::Resolved)
                .ok_or_else(|| {
                    error::invalid_time(&id.to_string(), "no event with that reference is retained")
                }),
        }
    }

    /// Whether the selector needs a ledger to resolve (§12.2).
    #[must_use]
    pub const fn needs_anchor(&self) -> bool {
        matches!(self, TimeSelector::Event(_))
    }
}

/// Reads a local wall time in `zone`, reporting a fold or a gap rather than picking one (§25.6).
fn resolve_local(zone: &TimeZone, local: civil::DateTime) -> TimeResolution {
    match zone.to_ambiguous_timestamp(local).offset() {
        AmbiguousOffset::Unambiguous { offset } => offset
            .to_timestamp(local)
            .map_or(TimeResolution::Resolved(Timestamp::UNIX_EPOCH), |instant| {
                TimeResolution::Resolved(instant)
            }),
        AmbiguousOffset::Fold { before, after } => {
            match (before.to_timestamp(local), after.to_timestamp(local)) {
                (Ok(first), Ok(second)) => TimeResolution::Ambiguous {
                    earlier: first.min(second),
                    later: first.max(second),
                },
                // A fold whose own offsets cannot be applied to the wall time it folds is not
                // reachable through the tz database; treating it as unambiguous keeps the
                // caller answering rather than panicking.
                (Ok(only), Err(_)) | (Err(_), Ok(only)) => TimeResolution::Resolved(only),
                (Err(_), Err(_)) => TimeResolution::Resolved(Timestamp::UNIX_EPOCH),
            }
        }
        AmbiguousOffset::Gap { before, after } => {
            match (before.to_timestamp(local), after.to_timestamp(local)) {
                (Ok(first), Ok(second)) => TimeResolution::Skipped {
                    gap_from: first.min(second),
                    gap_until: first.max(second),
                },
                (Ok(only), Err(_)) | (Err(_), Ok(only)) => TimeResolution::Resolved(only),
                (Err(_), Err(_)) => TimeResolution::Resolved(Timestamp::UNIX_EPOCH),
            }
        }
    }
}

/// A relative selector, refusing one that names the future (§4.4).
fn relative(text: &str, span: Duration) -> Result<TimeSelector, ErrorValue> {
    if span.nanoseconds() > 0 {
        return Err(error::invalid_time(
            text,
            "§4.4: relative future selectors are invalid for historical context",
        ));
    }
    Ok(TimeSelector::Relative(span))
}

/// Whether the text opens with `YYYY-MM-DD` and a separator, which is what makes it a date form.
///
/// The separator has to be there: §4.4 defines no date-only selector, and a bare `2026-08-31`
/// would otherwise become midnight — a time the user did not ask for.
fn looks_like_a_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() > 10
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && matches!(bytes[10], b'T' | b't' | b' ')
}

/// Reads `12:17` or `12:17:00`, and nothing that trails other words.
fn parse_local_time(text: &str) -> Result<civil::Time, ()> {
    if text.contains(char::is_whitespace) {
        return Err(());
    }
    let padded = if text.matches(':').count() == 1 {
        format!("{text}:00")
    } else {
        text.to_owned()
    };
    padded.parse::<civil::Time>().map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_recognise_a_date_form_only_when_the_text_opens_with_one() {
        assert!(looks_like_a_date("2026-08-31 12:17:00"));
        assert!(looks_like_a_date("2026-08-31T12:17:00+02:00"));
        assert!(!looks_like_a_date("12:17:00"));
        assert!(!looks_like_a_date("-10m"));
        assert!(!looks_like_a_date("2026-8-31 12:17"));
        assert!(
            !looks_like_a_date("2026-08-31"),
            "§4.4 has no date-only form"
        );
    }

    #[test]
    fn should_pad_a_minute_precision_time_when_it_is_read() {
        assert_eq!(
            parse_local_time("12:17"),
            Ok(civil::Time::new(12, 17, 0, 0).unwrap_or_default())
        );
        assert!(parse_local_time("12:17 tomorrow").is_err());
        assert!(parse_local_time("noon").is_err());
    }
}
