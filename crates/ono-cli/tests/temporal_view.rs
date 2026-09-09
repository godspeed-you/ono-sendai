//! The temporal half of the full-screen map, driven at a real terminal (spec v0.5 §18, §19).
//!
//! Narrative: `docs/specs/ono_sendai_shell_spec_v0.5_temporal_causal_systems_interface.md` —
//! §18.1 (one live/historical model), §18.2 (`Space` freezes the view and nothing else), §18.3
//! (the normative rewind bindings), §18.4 (`[` and `]` step significant events rather than
//! provider samples), §18.5 (no fake frames), §18.6 (a cursor beyond coverage shows the gap
//! rather than the last state), §18.7 (`N` returns to now and summarises through the canonical
//! `changes` engine), §47.5 (PTY tests), §48.6 (the rewind acceptance scenarios); plus v0.4
//! §43.4 (resize preserves place and focus; Ctrl-C leaves a live map without ending the shell)
//! and v0.4 §49.8 (a full-screen view is exited cleanly).
//!
//! What is driven here is the temporal cursor *of the map*. §19's full-screen timeline is a
//! second view with its own keys, and it has its own suite: `crates/ono-cli/tests/timeline_view.rs`
//! drives `timeline --view` and the `T` that opens it from a map (ADR-0781). What this file holds
//! of §19 is the terminal contract every full-screen view shares — entered deliberately, left
//! cleanly, on the quiet path and on Ctrl-C alike. The timeline's own rendering is proved by
//! `crates/ono-temporal-render/tests/hud.rs` and `…/grouping.rs`.
//!
//! §18.6 is held here at the coordinate a terminal can move: `at` refuses an instant nothing
//! observed and names it, so the session never stands somewhere it has no evidence for. The gap
//! *frame* — the interval, the reason and the last supported instant, drawn instead of the map —
//! is proved over a ledger whose coverage has a hole by
//! `crates/ono-cli/tests/spatial_temporal_view.rs` and
//! `crates/ono-temporal-render/tests/gap_frame.rs`, because a session's own recorder produces no
//! such hole to step into.
//!
//! Every test here drives the real `ono` binary through a pseudo-terminal with `NO_COLOR=1`, a
//! scratch `HOME` and every XDG root inside it, so the store the recorder opens is the test's own
//! and the developer's history is neither read nor written. `ONO_TEMPORAL_RECORDING_ENABLED` is
//! the environment spelling of §10.2's opt-in, which is how these sessions get a ledger without
//! writing a configuration file.
//!
//! The assertions are about what the screen shows, what the prompt says afterwards and whether
//! the shell is still there — never about how the view is wired (AGENTS.md §11). Waits are
//! bounded, so a screen change that never arrives fails the run instead of hanging it.
//!
//! The same behaviours are proved against the container by `docker/acceptance/cases/`
//! `253-rewind-pause`, `254-rewind-stepping`, `255-rewind-gap` and `256-return-to-now`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::collections::BTreeSet;
use std::path::Path;
use std::time::{Duration, Instant};

use ono_process::{Command, Executor, PtySession, Signal, WindowSize};
use ono_testkit::scratch;

mod support;

/// How long any one screen change may take before the test calls it missing.
///
/// A liveness bound and not a performance assertion: §32's budgets are measured by `xtask perf`
/// and by the container's own budget cases. Opening a full-screen map of a place on a machine
/// running a dozen other process-spawning suites costs a whole projection, so the bound is
/// generous on purpose — what it buys is a failure that names the screen it was looking at
/// instead of a run that hangs.
const BUDGET: Duration = Duration::from_secs(45);

/// How long the very first prompt may take, while the process is still starting.
const STARTUP: Duration = Duration::from_secs(60);

/// The alternate screen buffer: entering it is how a full-screen view borrows the terminal and
/// leaving it is how the shell's own screen comes back (v0.4 §23.3, §49.8, §52.2).
const ALTERNATE_SCREEN_ON: &str = "\u{1b}[?1049h";
const ALTERNATE_SCREEN_OFF: &str = "\u{1b}[?1049l";

/// §18.3's normative default bindings, in the spelling a terminal actually delivers.
const PAUSE: &[u8] = b" ";
const STEP_PREVIOUS: &[u8] = b"[";
const NUDGE_BACK: &[u8] = b"{";
const RETURN_TO_NOW: &[u8] = b"N";
/// v0.4 §23.3's own keys, which the temporal view keeps.
const DOWN: &[u8] = b"\x1b[B";
const ENTER: &[u8] = b"\r";
const ESCAPE: &[u8] = b"\x1b";
/// Ctrl-C, which v0.4 §43.4 requires to leave a live map without ending the session.
const INTERRUPT: &[u8] = &[0x03];

/// How far `{` moves the cursor, in seconds (§18.3's `Shift-[`).
const NUDGE_SECONDS: i64 = 30;

/// An interactive `ono` on a pseudo-terminal, and everything it has painted so far.
struct Terminal {
    pty: PtySession,
    seen: String,
}

impl Terminal {
    /// Starts a recording `ono` on a pty of `size`, with every root inside `home`.
    ///
    /// Recording is on because §18.4's stepping and §18.7's summary read a ledger, and a session
    /// without one can only answer §2.17's "nothing was watching". The roots are redirected so
    /// the ledger this session opens is this test's and nobody else's (§30.2).
    fn start(home: &Path, size: WindowSize) -> Self {
        let work = home.join("work");
        std::fs::create_dir_all(&work).expect("a working directory inside the scratch home");
        let root = home.display().to_string();
        let command = Command::new(ono_testkit::ono_binary())
            .env("TERM", "xterm")
            .env("NO_COLOR", "1")
            .env("HOME", root.clone())
            .env("XDG_CONFIG_HOME", format!("{root}/config"))
            .env("XDG_DATA_HOME", format!("{root}/data"))
            .env("XDG_STATE_HOME", format!("{root}/state"))
            .env("ONO_TEMPORAL_RECORDING_ENABLED", "true")
            .current_dir(&work);
        let pty = Executor::detached()
            .run_pty(&command, size)
            .expect("a pseudo-terminal must be allocatable");
        Self {
            pty,
            seen: String::new(),
        }
    }

    /// Everything the terminal has painted, escape sequences included.
    fn seen(&self) -> &str {
        &self.seen
    }

    /// Takes one bounded read from the terminal into the transcript.
    fn poll(&mut self) {
        let mut buffer = [0u8; 16384];
        if let Ok(Some(count)) = self
            .pty
            .read_timeout(&mut buffer, Duration::from_millis(120))
        {
            self.seen
                .push_str(&String::from_utf8_lossy(&buffer[..count]));
        }
    }

    /// Reads until `ready` accepts the transcript, or the budget runs out.
    fn wait_until(&mut self, budget: Duration, ready: impl Fn(&str) -> bool) -> bool {
        let deadline = Instant::now() + budget;
        loop {
            if ready(&self.seen) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            self.poll();
        }
    }

    /// Reads until `needle` is anywhere on screen.
    fn wait_for(&mut self, needle: &str, budget: Duration) -> bool {
        let needle = needle.to_owned();
        self.wait_until(budget, |seen| seen.contains(&needle))
    }

    /// Reads until `needle` appears in what was painted after the last `mark`.
    fn wait_for_after(&mut self, mark: &str, needle: &str, budget: Duration) -> bool {
        let mark = mark.to_owned();
        let needle = needle.to_owned();
        self.wait_until(budget, |seen| tail(seen, &mark).contains(&needle))
    }

    /// Reads until `needle` appears in what was painted since `mark`.
    ///
    /// The index is the anchor to prefer whenever the text a test typed can be repeated by the
    /// answer: `map` is echoed as a command and then written again into the frame's own header,
    /// so an anchor on the *word* moves forward under the assertion and the assertion never
    /// finishes.
    fn wait_since(&mut self, mark: usize, needle: &str, budget: Duration) -> bool {
        let needle = needle.to_owned();
        self.wait_until(budget, |seen| {
            seen[mark.min(seen.len())..].contains(&needle)
        })
    }

    /// Keeps reading for `patience`, so everything a command painted has arrived.
    fn settle(&mut self, patience: Duration) {
        let deadline = Instant::now() + patience;
        while Instant::now() < deadline {
            self.poll();
        }
    }

    /// Sends raw key bytes.
    fn keys(&mut self, bytes: &[u8]) {
        self.pty
            .write_all(bytes)
            .expect("the pseudo-terminal must accept a key press");
    }

    /// Types a line and presses Return.
    fn line(&mut self, text: &str) {
        self.keys(text.as_bytes());
        self.keys(b"\n");
    }

    /// Resizes the window the shell believes it is drawing into.
    fn resize(&mut self, size: WindowSize) {
        self.pty
            .resize(size)
            .expect("the pseudo-terminal must be resizable");
    }

    /// How much has been painted so far, as a place to start reading from.
    fn mark(&self) -> usize {
        self.seen.len()
    }

    /// Everything painted since `mark`.
    fn since(&self, mark: usize) -> &str {
        &self.seen[mark.min(self.seen.len())..]
    }

    /// Is the shell still running?
    fn alive(&mut self) -> bool {
        matches!(self.pty.try_wait(), Ok(None))
    }

    /// Puts the session into historical context at `selector`, and answers with the marker the
    /// prompt then carries.
    ///
    /// §4.6 makes historical context visually obvious, so the prompt marker is how a test sees
    /// that `at` took effect; a session whose ledger cannot reach that far is refused by name
    /// (§12.3), and the assertion says so rather than failing later inside the view.
    fn enter_the_past(&mut self, selector: &str) -> &'static str {
        let mark = self.mark();
        self.line(&format!("at {selector}"));
        assert!(
            self.wait_until(BUDGET, |seen| historical_prompt(&seen[mark..]).is_some()),
            "v0.5 §4.2, §4.6: `at {selector}` must put the session into historical context and \
             the prompt must say so with `[PAST]` or `[PAST?]`. A refusal here is §12.3's \
             `temporal.not_recorded`, which means this session's ledger does not reach that far \
             back; saw:\n{}",
            screen(self.since(mark))
        );
        self.settle(Duration::from_millis(400));
        historical_prompt(self.since(mark)).expect("the prompt just carried a historical marker")
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.pty.signal(Signal::KILL);
        let _ = self.pty.wait();
    }
}

/// Everything painted after the last occurrence of `mark`, or nothing when it never appeared.
fn tail(text: &str, mark: &str) -> String {
    match text.rfind(mark) {
        Some(at) => text[at + mark.len()..].to_owned(),
        None => String::new(),
    }
}

/// The text with its escape sequences removed, as a reader of the screen would see it.
///
/// Only for failure messages: an assertion that stripped the escapes would lose the geometry the
/// resize tests are about, so the assertions read the raw transcript and the messages read this.
fn screen(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('\u{1b}') {
        out.push_str(&rest[..at]);
        let body = &rest[at + 1..];
        let skip = body
            .char_indices()
            .find(|(index, character)| *index > 0 && ('\u{40}'..='\u{7e}').contains(character))
            .map_or(body.len(), |(index, character)| {
                index + character.len_utf8()
            });
        rest = &body[skip..];
    }
    out.push_str(rest);
    out
}

/// The repaints a passage holds, split where each frame homes the cursor.
fn repaints(text: &str) -> Vec<&str> {
    text.split("\u{1b}[1;1H").skip(1).collect()
}

/// The terminal rows a passage positioned the cursor to, read off `ESC [ <row> ; <column> H`.
///
/// The map paints a frame by addressing each row in turn, so the set of rows a frame touched is
/// the height it was drawn at. That is the one observation a resize produces and an earlier
/// repaint cannot.
fn rows_painted(text: &str) -> BTreeSet<usize> {
    let mut rows = BTreeSet::new();
    for piece in text.split('\u{1b}') {
        let Some(body) = piece.strip_prefix('[') else {
            continue;
        };
        let Some(end) = body.find('H') else {
            continue;
        };
        let Some((row, _)) = body[..end].split_once(';') else {
            continue;
        };
        if let Ok(row) = row.parse::<usize>() {
            rows.insert(row);
        }
    }
    rows
}

/// The instant `PAUSED @…` names, as the HUD wrote it (§18.2).
fn frozen_instant(text: &str) -> Option<String> {
    let after = text.rsplit_once("PAUSED @")?.1;
    let instant: String = after
        .chars()
        .take_while(|character| {
            character.is_ascii_digit() || *character == ':' || *character == '.'
        })
        .collect();
    (instant.len() >= "00:00:00".len()).then_some(instant)
}

/// The instant the map's HUD carries beside `[PAST]`, as §4.6 spells the historical coordinate.
fn cursor_instant(text: &str) -> Option<String> {
    let head = text.rsplit_once(" [PAST]")?.0;
    let at = head.rfind('@')?;
    let instant = head[at + 1..].trim().to_owned();
    (instant.len() == "00:00:00".len()).then_some(instant)
}

/// The historical marker the *prompt* carries — `[PAST]` where coverage is complete and `[PAST?]`
/// where it is partial (§4.6).
fn historical_prompt(text: &str) -> Option<&'static str> {
    ["[PAST?]", "[PAST]"]
        .into_iter()
        .find(|marker| text.contains(marker))
}

/// A wall-clock `HH:MM:SS` as seconds into the day, so two HUD instants can be subtracted.
fn day_seconds(clock: &str) -> Option<i64> {
    let mut parts = clock.split(':');
    let hours: i64 = parts.next()?.parse().ok()?;
    let minutes: i64 = parts.next()?.parse().ok()?;
    let seconds: i64 = parts.next()?.split('.').next()?.parse().ok()?;
    Some(hours * 3600 + minutes * 60 + seconds)
}

/// Whether a passage names a canonical event of the kinds §18.4 ranks as significant.
///
/// `provider.event` is deliberately absent: §18.4's whole point is that a step lands on a change
/// to the world and not on the next raw sample a provider produced.
fn names_a_significant_event(text: &str) -> bool {
    ["object.", "relation.", "action.", "landmark."]
        .into_iter()
        .any(|kind| text.contains(kind))
}

/// Whether a passage carries either of the two honest answers to a step that found nothing
/// (§18.5, §2.17).
fn says_there_was_nothing_to_step_to(text: &str) -> bool {
    text.contains("no significant event that way")
        || text.contains("this session has recorded no events")
}

#[test]
fn should_freeze_the_view_and_leave_the_providers_running_when_space_pauses_a_live_map() {
    // §18.2: `Space` "pauses temporal advancement of the displayed view", and "pausing does NOT
    // stop providers, the recorder or the real system. It freezes only the view's temporal
    // cursor." The HUD "MUST show `PAUSED @14:03:12.410`".
    //
    // So the test freezes a live map and then asks the three things the sentence names whether
    // they stopped: a process this test started is still running, the recorder still reports
    // itself running, and the shell still answers. Releasing the pause says in words that the
    // view is following the present again, which is the other half of §18.2.
    let mut witness = support::fixture_process();
    let home = scratch();
    let mut session = Terminal::start(home.path(), WindowSize::new(30, 100));
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        screen(session.seen())
    );

    let opened = session.mark();
    session.line("map --live");
    assert!(
        session.wait_since(opened, ALTERNATE_SCREEN_ON, BUDGET),
        "v0.4 §25.1: `map --live` opens a full-screen live view; saw:\n{}",
        screen(session.since(opened))
    );

    let paused = session.mark();
    session.keys(PAUSE);
    assert!(
        session.wait_until(BUDGET, |seen| frozen_instant(&seen[paused..]).is_some()),
        "v0.5 §18.2: the HUD must show `PAUSED @<instant>` while the view's cursor is frozen; \
         saw:\n{}",
        screen(session.since(paused))
    );
    let frozen = frozen_instant(session.since(paused)).expect("the HUD just named the instant");
    assert!(
        session.wait_for_after(
            ALTERNATE_SCREEN_ON,
            "the providers, the recorder and the mac",
            BUDGET
        ),
        "v0.5 §18.2: pausing freezes the view and nothing else, and the view says so rather than \
         leaving the user to guess whether the machine stopped too; saw:\n{}",
        screen(session.since(paused))
    );

    let released = session.mark();
    session.keys(PAUSE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, "following the present again", BUDGET),
        "v0.5 §18.2: `Space` is pause *and* resume, and resuming returns the view to the present \
         it never stopped observing; saw:\n{}",
        screen(session.since(released))
    );

    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "v0.4 §49.8: the view closes and the shell's own screen comes back; saw:\n{}",
        screen(session.since(released))
    );

    // The three things §18.2 promises were never stopped, asked one at a time.
    assert!(
        matches!(witness.try_wait(), Ok(None)),
        "v0.5 §18.2: freezing the view must not stop the real system — the process this test \
         started before the pause is gone, so something stopped the machine and not just the view"
    );
    let after = session.mark();
    session.line("get recorder");
    assert!(
        session.wait_since(after, "recorder running", BUDGET),
        "v0.5 §18.2, §10.3: the recorder kept running while the view's cursor was frozen — \
         pausing a view is not a way of stopping ingestion; saw:\n{}",
        screen(session.since(after))
    );
    assert!(
        session.alive(),
        "v0.5 §18.2 with v0.4 §49.8: pausing and leaving the view ends the view, not the shell. \
         The view was frozen at {frozen}"
    );

    session.line("exit");
    let _ = witness.kill();
    let _ = witness.wait();
}

#[test]
fn should_move_the_cursor_only_to_a_significant_event_when_the_step_keys_walk_the_horizon() {
    // §18.4: "`[` and `]` step through events relevant to the visible map horizon, not every raw
    // provider event", and §18.5 forbids inventing a state between supported positions. The
    // observable difference between the two is where the cursor lands: a step either names a
    // canonical event of §6.1 and moves onto its instant, or reports that there is none that way
    // and moves nothing at all. A view that slid to the next provider sample would have moved
    // without naming anything, and that is what this test rules out.
    //
    // `{` is the control: §18.3's thirty-second nudge *is* a move by time, so the same view
    // answers the two keys differently — which is what makes "the step keys walk events" a claim
    // about events rather than about the clock.
    let home = scratch();
    let mut session = Terminal::start(home.path(), WindowSize::new(30, 100));
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        screen(session.seen())
    );
    session.line("enter compute");
    session.settle(Duration::from_secs(3));
    session.enter_the_past("-3s");

    let opened = session.mark();
    session.line("map");
    assert!(
        session.wait_until(BUDGET, |seen| cursor_instant(&seen[opened..]).is_some()),
        "v0.5 §4.6 with v0.4 §23.3: a map opened in historical context carries the coordinate it \
         is drawing at in its HUD; saw:\n{}",
        screen(session.since(opened))
    );
    let before = cursor_instant(session.since(opened)).expect("the HUD just named the coordinate");

    let stepped = session.mark();
    session.keys(STEP_PREVIOUS);
    assert!(
        session.wait_until(BUDGET, |seen| {
            let painted = &seen[stepped..];
            cursor_instant(painted).is_some()
                && (says_there_was_nothing_to_step_to(painted)
                    || names_a_significant_event(painted))
        }),
        "v0.5 §18.4 with §2.17: `[` must be answered — either with the significant event it \
         landed on, or with the fact that this view's window holds none. An unanswered step is \
         neither; saw:\n{}",
        screen(session.since(stepped))
    );
    let painted = screen(session.since(stepped));
    let landed = cursor_instant(session.since(stepped)).expect("the HUD redrew its coordinate");
    if says_there_was_nothing_to_step_to(&painted) {
        assert_eq!(
            landed, before,
            "v0.5 §18.4, §18.5: a step that found no significant event must move nothing. This \
             view slid from {before} to {landed} without naming an event, which is a step through \
             provider samples rather than through change; saw:\n{painted}"
        );
    } else {
        assert_ne!(
            landed, before,
            "v0.5 §18.4: a step that named an event must stand at that event's instant; saw:\n\
             {painted}"
        );
    }
    assert!(
        !painted.contains("provider.event"),
        "v0.5 §18.4: the stepper walks the events relevant to the visible horizon — object and \
         relation lifecycles, service state, landmarks and operator actions — and never the raw \
         provider samples underneath them; saw:\n{painted}"
    );

    let nudged = session.mark();
    session.keys(NUDGE_BACK);
    assert!(
        session.wait_until(BUDGET, |seen| {
            cursor_instant(&seen[nudged..]).is_some_and(|now| now != landed)
        }),
        "v0.5 §18.3: `{{` moves the cursor thirty seconds back, which is a move by time and not \
         by event; saw:\n{}",
        screen(session.since(nudged))
    );
    let moved = cursor_instant(session.since(nudged)).expect("the HUD redrew after the nudge");
    let (from, to) = (
        day_seconds(&landed).expect("the HUD writes a wall clock"),
        day_seconds(&moved).expect("the HUD writes a wall clock"),
    );
    assert_eq!(
        (from - to).rem_euclid(24 * 3600),
        NUDGE_SECONDS,
        "v0.5 §18.3: the nudge is thirty seconds, so the cursor moved from {landed} to {moved}. \
         The step keys and the nudge keys are different operations on the same cursor, and that \
         is what §18.4 means by stepping events rather than time"
    );

    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "v0.4 §49.8: the view closes cleanly after the cursor has been moved; saw:\n{}",
        screen(session.since(nudged))
    );
    session.line("exit");
}

#[test]
fn should_enter_the_historical_place_and_keep_the_coordinate_when_enter_is_pressed_on_a_past_node()
{
    // §18.3: "`Enter` — enter focused object at cursor time". §14.1 makes the spatial commands
    // honour the temporal context, so entering a node while the cursor is in the past must move
    // the session to that place *at that instant*: the place follows the cursor and the
    // coordinate does not silently snap back to now.
    //
    // What the terminal can see is exactly that pair. Inside the view, the frame after `Enter`
    // names the place that was entered and still carries the historical marker; after the view
    // closes, the prompt names the same place and the same marker, so the session that comes
    // back is the one the cursor left behind.
    let home = scratch();
    let mut session = Terminal::start(home.path(), WindowSize::new(30, 100));
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        screen(session.seen())
    );
    session.line("enter compute");
    session.settle(Duration::from_secs(3));
    let marker = session.enter_the_past("-3s");

    let opened = session.mark();
    session.line("map");
    assert!(
        session.wait_until(BUDGET, |seen| cursor_instant(&seen[opened..]).is_some()),
        "v0.5 §4.6: the map drawn in historical context says which instant it is drawing; saw:\n{}",
        screen(session.since(opened))
    );
    let coordinate = cursor_instant(session.since(opened)).expect("the HUD named the coordinate");

    let entered = session.mark();
    session.keys(DOWN);
    session.keys(ENTER);
    assert!(
        session.wait_until(BUDGET, |seen| {
            tail(&seen[entered..], "\u{1b}[1;1H").contains("local/compute/")
        }),
        "v0.5 §18.3 with v0.4 §23.3: `Enter` on the focused node enters it, so the frame after \
         the key names a place inside `local/compute` rather than `local/compute` itself; saw:\n{}",
        screen(session.since(entered))
    );
    assert!(
        cursor_instant(session.since(entered)).is_some_and(|at| at == coordinate),
        "v0.5 §18.3, §14.1: the object was entered *at cursor time* — the place moved and the \
         instant did not. The view opened at {coordinate}; saw:\n{}",
        screen(session.since(entered))
    );

    let closed = session.mark();
    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "v0.4 §49.8: the view closes and hands the screen back; saw:\n{}",
        screen(session.since(closed))
    );
    session.settle(Duration::from_millis(600));
    // Read from the moment the shell's own screen came back: the map's own header names the place
    // too, and an assertion that could be satisfied by the last frame would prove nothing about
    // where the *session* is standing.
    let prompt = screen(&tail(session.seen(), ALTERNATE_SCREEN_OFF));
    assert!(
        prompt.contains("local/compute/"),
        "v0.4 §21.1 with v0.5 §18.3: the session is standing where the cursor entered, so the \
         prompt names a place inside `local/compute`; saw:\n{prompt}"
    );
    assert!(
        prompt.contains(marker),
        "v0.5 §4.6, §18.3: the session's coordinate followed the cursor into the past place — \
         the prompt still carries `{marker}` rather than having quietly returned to now; \
         saw:\n{prompt}"
    );

    session.line("now");
    session.settle(Duration::from_millis(600));
}

#[test]
fn should_return_the_view_to_the_present_and_summarise_what_changed_when_n_is_pressed() {
    // §18.7: "Pressing `N` […] returns the temporal cursor to present state", and "the view
    // SHOULD briefly summarize accumulated changes […] This summary uses the canonical `changes`
    // engine." The summary the renderer writes opens with `returned to now` and then reads the
    // `ono.temporal-change/1` answer — the same answer the `changes` command renders — so what
    // the terminal can check is that the view says it returned, that the paused marker is gone
    // from the frames after it, and that the shell's own `changes` command answers over the same
    // window without a second historical code path (§4.5, §13.1).
    let home = scratch();
    let mut session = Terminal::start(home.path(), WindowSize::new(30, 100));
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        screen(session.seen())
    );

    let opened = session.mark();
    session.line("map --live");
    assert!(
        session.wait_since(opened, ALTERNATE_SCREEN_ON, BUDGET),
        "v0.4 §25.1: `map --live` opens the live view the cursor is frozen in; saw:\n{}",
        screen(session.since(opened))
    );

    let paused = session.mark();
    session.keys(PAUSE);
    assert!(
        session.wait_until(BUDGET, |seen| frozen_instant(&seen[paused..]).is_some()),
        "v0.5 §18.2: the cursor is frozen before it is returned, and the HUD names the instant; \
         saw:\n{}",
        screen(session.since(paused))
    );
    let frozen = frozen_instant(session.since(paused)).expect("the HUD named the frozen instant");

    let returned = session.mark();
    session.keys(RETURN_TO_NOW);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, "returned to now", BUDGET),
        "v0.5 §18.7: `N` returns the cursor to the present and summarises what accumulated while \
         it was away. The view was frozen at {frozen}; saw:\n{}",
        screen(session.since(returned))
    );
    session.settle(Duration::from_secs(2));
    let painted = repaints(session.since(returned));
    assert!(
        !painted.is_empty(),
        "v0.5 §18.7: returning the cursor repaints the view at the present it returned to; \
         saw:\n{}",
        screen(session.since(returned))
    );
    let last = painted.last().copied().unwrap_or_default();
    assert!(
        frozen_instant(last).is_none(),
        "v0.5 §18.7 with §18.2: after the return the cursor follows the present, so the frame the \
         view is left showing carries no `PAUSED @`. It was frozen at {frozen}; saw:\n{}",
        screen(last)
    );

    let closed = session.mark();
    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "v0.4 §49.8: the view closes after the return; saw:\n{}",
        screen(session.since(closed))
    );

    // §13.1 and §18.7: one change engine, reachable as a command. A summary produced by a second
    // comparison would leave this command answering about a window the view could not read.
    let asked = session.mark();
    session.line("changes --since 1m");
    assert!(
        session.wait_since(asked, "> ", BUDGET),
        "v0.5 §13.1: the canonical `changes` command answers over the window the return-to-now \
         summary reads, in the same session and from the same ledger; saw:\n{}",
        screen(session.since(asked))
    );
    let answered = screen(session.since(asked));
    assert!(
        !answered.contains("unknown_command") && !answered.contains("has no option"),
        "v0.5 §13.1, §18.7: the summary is a reading of the canonical `changes` engine, so that \
         engine is the shell's own command and takes `--since`; saw:\n{answered}"
    );
    assert!(
        session.alive(),
        "v0.4 §49.8: returning to now and leaving the view ends the view, not the shell"
    );

    session.line("exit");
}

#[test]
fn should_keep_the_frozen_instant_and_the_place_when_the_terminal_is_resized_during_the_view() {
    // v0.4 §43.4: "terminal resize preserves current place and focus where possible", and §39.3
    // lets a smaller frame drop detail. v0.5 adds the part that matters here: a resize is a
    // change of geometry and must therefore change no *semantic* time and no place. The map is
    // rewound into the past before the window changes, so both are in play at once — the
    // coordinate the cursor is standing at, and the place the session is standing in.
    //
    // The observation a resize produces and an earlier repaint cannot is the geometry of the
    // frame it caused: a frame that addresses row 20 and no row above it is a frame at the new
    // twenty-row size, and this one still carries the same coordinate.
    let home = scratch();
    let mut session = Terminal::start(home.path(), WindowSize::new(30, 100));
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        screen(session.seen())
    );
    session.line("enter compute");
    session.settle(Duration::from_secs(3));
    let marker = session.enter_the_past("-3s");

    let opened = session.mark();
    session.line("map");
    assert!(
        session.wait_until(BUDGET, |seen| cursor_instant(&seen[opened..]).is_some()),
        "v0.5 §4.6: the historical map names the instant it is drawing; saw:\n{}",
        screen(session.since(opened))
    );
    let coordinate = cursor_instant(session.since(opened)).expect("the HUD named the coordinate");
    let before = rows_painted(session.since(opened));
    assert!(
        before.contains(&30),
        "the map fills the terminal it was given, so the frame before the resize reaches row 30; \
         it addressed {before:?}"
    );

    let resized = session.mark();
    // The rows change and the columns do not: a narrower frame is allowed to drop detail, and
    // the detail this test is about is the coordinate in the HUD.
    session.resize(WindowSize::new(20, 100));
    assert!(
        session.wait_until(BUDGET, |seen| {
            repaints(&seen[resized..]).into_iter().any(|frame| {
                let rows = rows_painted(frame);
                rows.contains(&20)
                    && rows.iter().all(|row| *row <= 20)
                    && cursor_instant(frame).is_some_and(|at| at == coordinate)
            })
        }),
        "v0.4 §43.4 with v0.5 §18: the map redraws at the new twenty-row size and still stands at \
         {coordinate}. A resize is geometry; it moves no semantic time. The frames it painted \
         addressed {:?}; saw:\n{}",
        repaints(session.since(resized))
            .into_iter()
            .map(rows_painted)
            .collect::<Vec<_>>(),
        screen(session.since(resized))
    );

    let closed = session.mark();
    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "v0.4 §49.8: the resized view still closes cleanly; saw:\n{}",
        screen(session.since(closed))
    );
    session.settle(Duration::from_millis(600));
    // Read from the moment the shell's own screen came back, so what is asserted on is the
    // prompt and never the last frame the view painted before it left.
    let prompt = screen(&tail(session.seen(), ALTERNATE_SCREEN_OFF));
    assert!(
        prompt.contains("local/compute"),
        "v0.4 §43.4: a resize preserves the current place; saw:\n{prompt}"
    );
    assert!(
        prompt.contains(marker),
        "v0.5 §4.6 with v0.4 §43.4: a resize preserves the session's coordinate too — the prompt \
         still carries `{marker}`; saw:\n{prompt}"
    );

    session.line("now");
    session.settle(Duration::from_millis(600));
}

#[test]
fn should_restore_the_terminal_on_every_exit_path_when_the_temporal_view_is_left() {
    // v0.4 §49.8 and §52.2 require every full-screen view to be exited cleanly, and v0.4 §43.4
    // names Ctrl-C explicitly: it ends the view and not the shell. A temporal view has more exit
    // paths than a plain one, because the cursor may be frozen or rewound when the user leaves,
    // and a view that restored the terminal only on the quiet path would leave a raw terminal
    // behind for whatever the user runs next.
    //
    // So both exits are driven with the cursor frozen, and the proof that the terminal came back
    // is an ordinary external program reporting a cooked line discipline afterwards (v0.4
    // §44.10).
    let home = scratch();
    let mut session = Terminal::start(home.path(), WindowSize::new(30, 100));
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        screen(session.seen())
    );

    for (key, path) in [(ESCAPE, "Esc"), (INTERRUPT, "Ctrl-C")] {
        let opened = session.mark();
        session.line("map --live");
        assert!(
            session.wait_since(opened, ALTERNATE_SCREEN_ON, BUDGET),
            "v0.4 §25.1: the live view opens before the {path} path is taken; saw:\n{}",
            screen(session.since(opened))
        );
        let paused = session.mark();
        session.keys(PAUSE);
        assert!(
            session.wait_until(BUDGET, |seen| frozen_instant(&seen[paused..]).is_some()),
            "v0.5 §18.2: the cursor is frozen when the {path} path is taken; saw:\n{}",
            screen(session.since(paused))
        );
        let left = session.mark();
        session.keys(key);
        assert!(
            session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
            "v0.4 §43.4, §49.8: {path} leaves the temporal view and restores the shell's own \
             screen; saw:\n{}",
            screen(session.since(left))
        );
        assert!(
            session.alive(),
            "v0.4 §43.4: {path} ends the view, not the session"
        );
        let after = session.mark();
        session.line("echo survived");
        assert!(
            session.wait_since(after, "\r\nsurvived", BUDGET),
            "v0.4 §43.4: the prompt comes back after {path} and the next command runs; saw:\n{}",
            screen(session.since(after))
        );
    }

    // v0.4 §44.10: an external program must inherit a usable terminal after all of that.
    let checked = session.mark();
    session.line("stty -a");
    assert!(
        session.wait_since(checked, "icanon", BUDGET),
        "v0.4 §44.10: an external program inherits the terminal the view borrowed; saw:\n{}",
        screen(session.since(checked))
    );
    let reported = screen(session.since(checked));
    assert!(
        !reported.contains("-icanon"),
        "v0.4 §44.10, §49.8: the line discipline is cooked again after every exit path — a raw \
         terminal left behind is what makes the next program unusable; saw:\n{reported}"
    );

    session.line("exit");
}

#[test]
fn should_answer_the_view_and_the_textual_query_from_one_coordinate_when_the_session_is_in_the_past()
 {
    // §18.1: "v0.4 `map --live` and v0.5 historical playback MUST converge on one event/state
    // model", with no "live diff model A / historical event model B". §2's invariant states it
    // as a property a user can see: pausing a live map uses the same canonical event and state
    // model as a textual temporal query.
    //
    // At a terminal that is one coordinate seen twice. The textual `at` moves the session into
    // the past and the prompt says so; the map opened afterwards draws at that same coordinate
    // and marks itself historical without being told again; `look` — a textual query — answers
    // at the same place the view drew; and after `now`, a freshly opened map carries no
    // historical marker at all. A second model would have to be told separately, and would not
    // follow the textual command that never mentioned it.
    let home = scratch();
    let mut session = Terminal::start(home.path(), WindowSize::new(30, 100));
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        screen(session.seen())
    );
    session.line("enter compute");
    session.settle(Duration::from_secs(3));
    let marker = session.enter_the_past("-3s");

    // The place path is read out of the answer's own document rather than off the screen: the
    // prompt names the place too, and an assertion the prompt could satisfy would say nothing
    // about what the query answered.
    let asked = session.mark();
    session.line("look --json");
    assert!(
        session.wait_since(asked, "\"place_path\":\"local/compute\"", BUDGET),
        "v0.5 §14.1: a textual query in historical context answers about the place the session \
         is standing in, and says which place that was; saw:\n{}",
        screen(session.since(asked))
    );

    let opened = session.mark();
    session.line("map");
    assert!(
        session.wait_until(BUDGET, |seen| cursor_instant(&seen[opened..]).is_some()),
        "v0.5 §18.1, §4.6: the view opened in a historical session is historical without being \
         told — it reads the one coordinate the textual `at` set, which the prompt reports as \
         `{marker}`; saw:\n{}",
        screen(session.since(opened))
    );
    assert!(
        screen(session.since(opened)).contains("compute"),
        "v0.5 §18.1: the view and the textual query are drawing the same place at the same \
         coordinate; saw:\n{}",
        screen(session.since(opened))
    );

    let closed = session.mark();
    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "v0.4 §49.8: the view closes; saw:\n{}",
        screen(session.since(closed))
    );

    let returned = session.mark();
    session.line("now");
    session.settle(Duration::from_secs(1));
    let present = session.mark();
    session.line("map");
    assert!(
        session.wait_since(present, ALTERNATE_SCREEN_ON, BUDGET),
        "v0.4 §23.3: a map opens again in the present; saw:\n{}",
        screen(session.since(present))
    );
    session.settle(Duration::from_secs(2));
    assert!(
        cursor_instant(session.since(present)).is_none(),
        "v0.5 §18.1, §4.6: `now` returned the session to the present, and the view follows it — \
         a present view carries no historical marker, because there is one temporal model and \
         not one per surface. The session had been at {marker} since {returned}; saw:\n{}",
        screen(session.since(present))
    );

    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "v0.4 §49.8: the second view closes too; saw:\n{}",
        screen(session.since(present))
    );
    session.line("exit");
}

#[test]
fn should_name_the_uncovered_interval_rather_than_the_last_state_when_the_coordinate_leaves_coverage()
 {
    // §18.6: a cursor that steps beyond coverage must show the gap, and "the map MUST NOT
    // continue showing the last state with a silently advancing timestamp". §12.3 is the same
    // rule for the session's own coordinate: moving to an instant nothing observed is refused by
    // name, and §2.17 forbids reading "nothing recorded" as "nothing happened".
    //
    // This session's ledger begins when the session does, so ten minutes ago is outside every
    // coverage it has and three seconds ago is inside. Both are asked, in that order, so the
    // refusal is known to be about coverage rather than about `at` never working: the uncovered
    // instant is named and the session stays where it was, and the covered one moves it.
    let home = scratch();
    let mut session = Terminal::start(home.path(), WindowSize::new(30, 100));
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        screen(session.seen())
    );
    session.settle(Duration::from_secs(3));

    let refused = session.mark();
    session.line("at -10m");
    assert!(
        session.wait_since(refused, "temporal.not_recorded", BUDGET),
        "v0.5 §12.3, §18.6, §55.5: an instant no evidence covers is named as uncovered. \
         Answering it from the last state with an older timestamp is the fake rewind §18.6 \
         forbids; saw:\n{}",
        screen(session.since(refused))
    );
    session.settle(Duration::from_millis(600));
    let answer = screen(session.since(refused));
    assert!(
        answer.contains("nothing recorded"),
        "v0.5 §2.17: the refusal says what is missing — nothing was recorded there — rather than \
         presenting an empty or stale world as history; saw:\n{answer}"
    );
    assert!(
        historical_prompt(&answer).is_none(),
        "v0.5 §12.1, §12.3: `at` resolves before it changes context, so a coordinate it refused \
         leaves the session in the present. A prompt carrying a historical marker here would \
         mean the shell moved to an instant it has no evidence for; saw:\n{answer}"
    );

    // The other side of the boundary, so the refusal above is known to be about coverage.
    let marker = session.enter_the_past("-3s");
    assert!(
        ["[PAST]", "[PAST?]"].contains(&marker),
        "v0.5 §4.6: a covered coordinate is entered and marked, and its marker states the \
         coverage: `[PAST]` complete, `[PAST?]` partial. Got {marker:?}"
    );

    session.line("now");
    session.settle(Duration::from_millis(600));
}
