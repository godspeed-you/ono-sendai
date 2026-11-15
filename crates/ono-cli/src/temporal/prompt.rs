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
/// The coordinate is published under the lock by the transition that commits it, so it is read
/// whoever holds the state now: a holder that is still working has not moved the session, and
/// the prompt cannot claim the past falsely. It used to give up whenever the state was held,
/// which since ADR-0984 can happen behind every prompt, and then said the present while the
/// session stood in the past (issue #311, ADR-1032).
#[must_use]
pub fn marker_segments() -> Option<(String, String)> {
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
    #![allow(
        clippy::expect_used,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;
    use ono_temporal_core::{CoverageSummary, TemporalContext, TimeRange, TimeSelector};

    /// The tests of this binary that move or read the process's one temporal coordinate take
    /// turns at it.
    static TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn should_show_no_temporal_segment_when_the_session_is_in_the_present() {
        let _turn = TURN.lock().await;
        assert_eq!(
            temporal_segments(),
            None,
            "§4.1: a session starts in the present, and the present has no marker"
        );
    }

    #[tokio::test]
    async fn should_keep_the_past_marker_in_the_prompt_while_another_task_holds_the_temporal_state()
    {
        // v0.5 §4.6: historical context is "visually obvious", and the prompt is where it shows.
        // The coordinate is published under the lock by the transition that commits it, so a
        // prompt can read it whoever holds the state now — and since ADR-0984 something may: the
        // read behind the prompt reaches the `not_recorded` refusal, which consults the temporal
        // state on a worker thread. A prompt that dropped `[PAST?]` whenever the two met said the
        // session was in the present while it was not (issue #311).
        let _turn = TURN.lock().await;
        let now = jiff::Timestamp::now();
        let past = now - jiff::SignedDuration::from_secs(3);
        {
            let mut state = crate::temporal::session::temporal_session().await;
            state.commit(
                TemporalContext::Historical {
                    requested: TimeSelector::parse("-3s").expect("a relative selector"),
                    requested_text: std::sync::Arc::from("-3s"),
                    resolved_at: past,
                    coverage: CoverageSummary::compose(&[], TimeRange::between(past, now)),
                    anchor_event: None,
                },
                "-3s",
                now,
            );
        }

        let background = crate::temporal::session::temporal_session().await;
        let shown = marker_segments();
        drop(background);

        let mut state = crate::temporal::session::temporal_session().await;
        state.commit(TemporalContext::Present, "now", jiff::Timestamp::now());
        drop(state);

        assert_eq!(
            shown.map(|(_, marker)| marker).as_deref(),
            Some("[PAST?]"),
            "v0.5 §4.6: the prompt says the session is in the past whoever else is reading the \
             temporal state"
        );
    }
}
