//! The temporal coordinate in the prompt (spec v0.5 §4.6, §8.6, §45.2).
//!
//! §4.6 makes historical context "visually obvious" and fixes the minimum text form:
//!
//! ```text
//! local/service/nginx:// @12:07:14 [PAST] >
//! ```
//!
//! Two rules shape what is here. §45.2 requires the distinction to survive monochrome and plain
//! text, so the marker is a **word** — `[PAST]`, `[PAST?]` — and the colour a theme adds is
//! decoration over a meaning the characters already carry. §8.6 forbids `[PAST]` from appearing
//! "merely because at least one event exists near that time", so the choice between the two
//! markers is [`ono_temporal_core::TemporalContext::prompt_marker`]'s, read from composed
//! coverage rather than from a count.

use jiff::tz::TimeZone;

/// The two prompt segments historical context adds: the clock and the marker (§4.6).
///
/// `None` in the present, which is every v0.2–v0.4 session and the default of every new one.
#[must_use]
pub fn temporal_segments() -> Option<(String, String)> {
    let context = super::session::coordinate();
    let instant = context.instant()?;
    let marker = context.prompt_marker()?;
    Some((clock_of(instant), marker.to_owned()))
}

/// The same two segments, read without ever waiting for the temporal lock.
///
/// A prompt is drawn between commands, so the lock is free; a prompt drawn while a background
/// temporal command holds it degrades to the present rather than blocking the line editor. It
/// cannot claim the past falsely, because the coordinate it reads is published under the lock by
/// the transition that committed it.
#[must_use]
pub fn marker_segments() -> Option<(String, String)> {
    if super::session::session_state().try_lock().is_err() {
        return None;
    }
    temporal_segments()
}

/// `@12:07:14` — the wall clock of the coordinate, in the session's zone (§25.3).
fn clock_of(instant: jiff::Timestamp) -> String {
    let zoned = jiff::Zoned::new(instant, TimeZone::system());
    format!(
        "@{:02}:{:02}:{:02}",
        zoned.hour(),
        zoned.minute(),
        zoned.second()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_show_no_temporal_segment_when_the_session_is_in_the_present() {
        assert_eq!(
            temporal_segments(),
            None,
            "§4.1: a session starts in the present, and the present has no marker"
        );
    }
}
