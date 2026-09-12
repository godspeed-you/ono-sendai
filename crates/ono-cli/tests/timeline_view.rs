//! The full-screen timeline, driven at a real terminal (spec v0.5 §19).
//!
//! Narrative: `docs/specs/ono_sendai_shell_spec_v0.5_temporal_causal_systems_interface.md` —
//! §19.1 (`timeline --view`, or `T` from a temporal-capable map, opens it), §19.2 (the normative
//! information architecture: the place and the window across the top, the rows with a cursor, the
//! evidence and coverage behind them, the keys under them), §19.3 (the canonical keys), §19.4
//! (event density), §19.5 (raw event expansion); plus §11.8 (a window centred on the coordinate
//! rather than ending at now), §18.3 (`T` opens the timeline *at the cursor*), §47.5 (PTY tests)
//! and v0.4 §49.8, §52.2, §43.4 (a full-screen view is entered deliberately, left cleanly on
//! every exit path including Ctrl-C, and hands the terminal back cooked).
//!
//! What is *not* proved here, and where it is proved instead:
//!
//! - §19.4's grouping and §19.5's expansion of a grouped row are decided by
//!   `ono-temporal-query`'s planner and drawn by `ono-temporal-render`, and
//!   `crates/ono-temporal-render/tests/grouping.rs` holds them over a record that has groups.
//!   Nothing in the shell writes an `object.observed` or `object.changed` event yet, so no
//!   session this suite can start produces a groupable run; what the terminal can prove is that
//!   the view asks for grouping and answers honestly when a row stands for one event (ADR-0781).
//! - The layout itself — the header, the rows, the evidence line, the legend — is a pure
//!   function of the record and is proved by `crates/ono-temporal-render/tests/timeline.rs`.
//!   Here it is proved *reachable*: that a person at a terminal gets it.
//!
//! Every test drives the real `ono` binary through a pseudo-terminal with `NO_COLOR=1`, a scratch
//! `HOME` and every XDG root inside it, so the store the recorder opens is the test's own and the
//! developer's history is neither read nor written. `ONO_TEMPORAL_RECORDING_ENABLED` is the
//! environment spelling of §10.2's opt-in.
//!
//! The assertions are about what the screen shows, what the prompt says afterwards, what an
//! external program inherits and what the exit status is — never about how the view is wired
//! (AGENTS.md §11). Waits are bounded, so a screen change that never arrives fails the run
//! instead of hanging it.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::{Duration, Instant};

use ono_process::{Command, Executor, PtySession, Signal, WindowSize};
use ono_testkit::Scratch;

mod support;

/// How long any one screen change may take before the test calls it missing.
///
/// A liveness bound and not a performance assertion: §32's budgets are measured by `xtask perf`.
/// Opening a view on a machine running a dozen other process-spawning suites costs a whole query,
/// so the bound is generous on purpose — what it buys is a failure that names the screen it was
/// looking at instead of a run that hangs.
const BUDGET: Duration = Duration::from_secs(45);

/// How long the very first prompt may take, while the process is still starting.
const STARTUP: Duration = Duration::from_secs(60);

/// The alternate screen buffer: entering it is how a full-screen view borrows the terminal and
/// leaving it is how the shell's own screen comes back (v0.4 §23.3, §49.8, §52.2).
const ALTERNATE_SCREEN_ON: &str = "\u{1b}[?1049h";
const ALTERNATE_SCREEN_OFF: &str = "\u{1b}[?1049l";

/// The cursor-position report a prompt asks for after a command (issue #132, ADR-0855).
const CURSOR_QUESTION: &str = "\u{1b}[6n";

/// §19.3's canonical keys, in the spelling a terminal delivers.
const UP: &[u8] = b"\x1b[A";
const ENTER: &[u8] = b"\r";
const ESCAPE: &[u8] = b"\x1b";
const WHY: &[u8] = b"W";
const AT: &[u8] = b"A";
const HELP: &[u8] = b"?";
const EXPAND: &[u8] = b"X";
/// §18.3's `T`, which opens the timeline from the map.
const OPEN_TIMELINE: &[u8] = b"T";
/// §18.3's `Space`, which freezes the map's temporal cursor. In the timeline it is bound to
/// nothing, which makes it the key that dismisses an overlay without also asking for anything —
/// and two `Esc` bytes in a row are one Alt-Esc to a terminal, never two presses.
const PAUSE: &[u8] = b" ";
const ANY_KEY: &[u8] = b" ";
/// Ctrl-C, which v0.4 §43.4 requires to leave a full-screen view without ending the shell.
const INTERRUPT: &[u8] = &[0x03];

/// A line the legend of §19.3 always carries, whatever the terminal's width.
const LEGEND: &str = "Enter inspect";

/// An interactive `ono` on a pseudo-terminal, and everything it has painted so far.
struct Terminal {
    pty: PtySession,
    seen: String,
}

impl Terminal {
    /// A recording `ono` at a prompt, with every root inside `home`.
    ///
    /// Recording is on because every window this suite reads is a window over a ledger, and a
    /// session without one can only answer §2.17's "nothing was watching". The roots are
    /// redirected so the store this session opens is this test's and nobody else's (§30.2).
    fn at_a_prompt(home: &Scratch) -> Self {
        let work = home.path().join("work");
        std::fs::create_dir_all(&work).expect("a working directory inside the scratch home");
        let root = home.path().display().to_string();
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
            .run_pty(&command, WindowSize::new(30, 100))
            .expect("a pseudo-terminal must be allocatable");
        let mut session = Self {
            pty,
            seen: String::new(),
        };
        assert!(
            session.wait_for("> ", STARTUP),
            "the shell must reach a prompt; saw:\n{}",
            shown(session.seen())
        );
        session
    }

    /// Everything the terminal has painted, escape sequences included.
    fn seen(&self) -> &str {
        &self.seen
    }

    /// Reads until `ready` accepts the transcript, or the budget runs out.
    ///
    /// The read is inside the loop rather than in a helper of its own: a full-screen view repaints
    /// a whole frame at a time, so what a bounded read produces is only meaningful against the
    /// predicate that is waiting for it.
    fn wait_until(&mut self, budget: Duration, ready: impl Fn(&str) -> bool) -> bool {
        let deadline = Instant::now() + budget;
        let mut buffer = [0u8; 16384];
        loop {
            if ready(&self.seen) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            if let Ok(Some(count)) = self
                .pty
                .read_timeout(&mut buffer, Duration::from_millis(120))
            {
                // Since issue #132 the prompt after a command asks the terminal where its cursor
                // is (`ESC [ 6 n`; ADR-0855, ADR-0861), and every terminal emulator answers. This
                // bare pseudo-terminal answers as one would — the first column, where the views
                // this suite closes leave the cursor — so no prompt waits out the shell's
                // two-second bound. Counting over the whole transcript answers a question split
                // across two reads as well.
                let asked = self.seen.matches(CURSOR_QUESTION).count();
                self.seen
                    .push_str(&String::from_utf8_lossy(&buffer[..count]));
                for _ in asked..self.seen.matches(CURSOR_QUESTION).count() {
                    self.keys(b"\x1b[1;1R");
                }
            }
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
    fn wait_since(&mut self, mark: usize, needle: &str, budget: Duration) -> bool {
        let needle = needle.to_owned();
        self.wait_until(budget, |seen| {
            seen[mark.min(seen.len())..].contains(&needle)
        })
    }

    /// Keeps reading for `patience`, so everything a command painted has arrived.
    fn settle(&mut self, patience: Duration) {
        self.wait_until(patience, |_| false);
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

    /// Opens the full-screen timeline and waits for §19.2's legend to prove it is up.
    fn open_the_timeline(&mut self) {
        let opened = self.mark();
        self.line("timeline --view");
        assert!(
            self.wait_since(opened, ALTERNATE_SCREEN_ON, BUDGET),
            "v0.5 §19.1: `timeline --view` opens the full-screen timeline, which borrows the \
             terminal's alternate screen; saw:\n{}",
            shown(self.since(opened))
        );
        assert!(
            self.wait_since(opened, LEGEND, BUDGET),
            "v0.5 §19.2, §19.3: the timeline draws the keys it answers under the rows; saw:\n{}",
            shown(self.since(opened))
        );
        self.settle(Duration::from_millis(400));
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
    text.rsplit_once(mark)
        .map_or_else(String::new, |(_, after)| after.to_owned())
}

/// The rows of the frame most recently painted.
///
/// A full-screen frame is written as `ESC [ <row> ; 1 H` followed by the row's text, so a row
/// begins wherever the cursor was addressed. Splitting there is the only way to read a frame as
/// lines: there are no newlines in one.
fn frame_rows(text: &str) -> Vec<String> {
    let frame = match text.rfind("\u{1b}[1;1H") {
        Some(at) => &text[at..],
        None => text,
    };
    let mut rows: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut rest = frame;
    while let Some(at) = rest.find('\u{1b}') {
        current.push_str(&rest[..at]);
        let body = &rest[at + 1..];
        let Some((index, final_byte)) = body
            .char_indices()
            .find(|(index, character)| *index > 0 && ('\u{40}'..='\u{7e}').contains(character))
        else {
            rest = "";
            break;
        };
        if final_byte == 'H' {
            rows.push(std::mem::take(&mut current));
        }
        rest = &body[index + final_byte.len_utf8()..];
    }
    current.push_str(rest);
    rows.push(current);
    rows
}

/// What a reader of the screen would see: the rows of the last frame, one per line.
///
/// Only for failure messages. A frame separates its rows by addressing the cursor rather than by
/// a newline, so a message that merely stripped the escapes would run thirty rows together into
/// one — which is the difference between a failure somebody can read and one they cannot.
fn shown(text: &str) -> String {
    frame_rows(text).join("\n")
}

/// The row §19.2's cursor is on, as the frame most recently painted drew it.
///
/// The cursor is a `>` in the first column followed by the row's wall clock, which is what tells
/// it apart from the shell's own `> ` prompt anywhere else in the transcript (§45.2: the
/// selection is legible without colour).
fn selected_row(text: &str) -> Option<String> {
    frame_rows(text).into_iter().rev().find(|row| {
        row.strip_prefix('>')
            .is_some_and(|rest| rest.starts_with(|character: char| character.is_ascii_digit()))
    })
}

/// The window §19.2's header states, as the two wall clocks it draws either end of the rule with.
fn header_window(text: &str) -> Option<(String, String)> {
    let header = frame_rows(text)
        .into_iter()
        .find(|row| row.contains("---") && clocks(row).len() == 2)?;
    let found = clocks(&header);
    Some((found[0].clone(), found[1].clone()))
}

/// Every `HH:MM` in a line, in the order it was written.
fn clocks(line: &str) -> Vec<String> {
    let characters: Vec<char> = line.chars().collect();
    let mut found = Vec::new();
    for start in 0..characters.len().saturating_sub(4) {
        let window: String = characters[start..start + 5].iter().collect();
        let digits: Vec<char> = window.chars().collect();
        if digits[0].is_ascii_digit()
            && digits[1].is_ascii_digit()
            && digits[2] == ':'
            && digits[3].is_ascii_digit()
            && digits[4].is_ascii_digit()
            && !characters
                .get(start + 5)
                .is_some_and(|next| *next == ':' || next.is_ascii_digit())
            && start
                .checked_sub(1)
                .is_none_or(|before| !characters[before].is_ascii_digit())
        {
            found.push(window);
        }
    }
    found
}

/// The historical marker the *prompt* carries — `[PAST]` where coverage is complete and `[PAST?]`
/// where it is partial (§4.6).
fn historical_prompt(text: &str) -> Option<&'static str> {
    ["[PAST?]", "[PAST]"]
        .into_iter()
        .find(|marker| text.contains(marker))
}

#[test]
fn should_show_the_window_the_events_and_the_keys_when_the_full_screen_timeline_opens() {
    // §19.1: "`timeline --view` […] opens the full-screen timeline." §19.2 fixes what is in it:
    // the place and the window across the top, the event rows with a selection, the evidence and
    // the coverage behind them, and the keys underneath. "The exact border style may vary. The
    // information architecture is normative" — so the assertions are on the components, not on
    // the box drawing.
    let home = support::home_with_a_recorded_action();
    let mut session = Terminal::at_a_prompt(&home);

    let opened = session.mark();
    session.open_the_timeline();
    let painted = session.since(opened).to_owned();

    assert!(
        header_window(&painted).is_some(),
        "v0.5 §19.2: the header states the window the timeline is a statement about, as a wall \
         clock either end of the rule; saw:\n{}",
        shown(&painted)
    );
    assert!(
        selected_row(&painted).is_some(),
        "v0.5 §19.2: the rows carry a cursor, and this ledger holds the four lifecycle events of \
         one Ono mutation to put one on; saw:\n{}",
        shown(&painted)
    );
    let frame = frame_rows(&painted).join("\n");
    assert!(
        frame.contains("evidence:") && frame.contains("coverage:"),
        "v0.5 §19.2: the evidence line names the sources the window rests on and the coverage \
         over it; saw:\n{}",
        shown(&painted)
    );
    for key in ["Enter inspect", "W why", "A at", "Esc exit"] {
        assert!(
            frame.contains(key),
            "v0.5 §19.3: `{key}` is one of the canonical keys, and a key the legend stops naming \
             is a key the reader stops knowing about; saw:\n{}",
            shown(&painted)
        );
    }

    let left = session.mark();
    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "v0.5 §19.3 with v0.4 §49.8: `Esc` exits and the shell's own screen comes back; saw:\n{}",
        shown(session.since(left))
    );
    assert!(session.alive(), "leaving the view does not end the session");
    session.line("exit");
}

#[test]
fn should_move_the_selection_between_events_when_the_arrow_keys_are_pressed() {
    // §19.3: "Up/Down or j/k    select event". The view opens on the most recent row, so `Up` is
    // the key that has somewhere to go; the observation is the cursor column of §19.2 moving to a
    // different row, which is the only thing a selection *is* on a screen.
    let home = support::home_with_a_recorded_action();
    let mut session = Terminal::at_a_prompt(&home);
    session.open_the_timeline();

    let opened = selected_row(session.seen()).unwrap_or_else(|| {
        panic!(
            "v0.5 §19.2: the timeline opens with a row selected; saw:\n{}",
            shown(session.seen())
        )
    });
    session.keys(UP);
    assert!(
        session.wait_until(BUDGET, |seen| selected_row(seen)
            .is_some_and(|row| row != opened)),
        "v0.5 §19.3: `Up` selects the event above, so the row the cursor is on changes. It \
         opened on {opened:?}; saw:\n{}",
        shown(session.seen())
    );

    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "v0.4 §49.8: the view still closes cleanly after navigating; saw:\n{}",
        shown(session.seen())
    );
    session.line("exit");
}

#[test]
fn should_inspect_the_selected_event_when_enter_is_pressed() {
    // §19.3: "Enter    inspect event". The event a row stands for is an `ono.temporal-event/1`,
    // and what inspecting it shows is the identity the next command accepts — §11.6's reference —
    // together with the canonical kind of §6.1.
    let home = support::home_with_a_recorded_action();
    let mut session = Terminal::at_a_prompt(&home);
    session.open_the_timeline();

    let inspected = session.mark();
    session.keys(ENTER);
    assert!(
        session.wait_until(BUDGET, |seen| {
            let frame = frame_rows(&seen[inspected.min(seen.len())..]).join("\n");
            frame.contains("@e") && frame.contains("kind")
        }),
        "v0.5 §19.3 with §11.6: `Enter` inspects the selected event, and what it shows carries \
         the event's own reference and its canonical kind; saw:\n{}",
        shown(session.since(inspected))
    );

    session.keys(ANY_KEY);
    session.settle(Duration::from_millis(400));
    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "v0.4 §49.8: the view closes cleanly after an inspection; saw:\n{}",
        shown(session.seen())
    );
    session.line("exit");
}

#[test]
fn should_explain_the_selected_event_when_why_is_asked_for() {
    // §19.3: "W    why selected event". §16.1 makes `why` need no model, and §15.7 makes an
    // unknown cause a complete answer rather than a failure — so what the screen must show is a
    // causal explanation with its coverage, whichever of the two it is.
    let home = support::home_with_a_recorded_action();
    let mut session = Terminal::at_a_prompt(&home);
    session.open_the_timeline();

    let asked = session.mark();
    session.keys(WHY);
    assert!(
        session.wait_until(BUDGET, |seen| {
            let frame = frame_rows(&seen[asked.min(seen.len())..]).join("\n");
            frame.contains("cause") && frame.contains("coverage")
        }),
        "v0.5 §19.3, §16.5, §16.6: `W` explains the selected event, and the explanation states \
         its cause — known or unknown — and the coverage it rests on; saw:\n{}",
        shown(session.since(asked))
    );

    session.keys(ANY_KEY);
    session.settle(Duration::from_millis(400));
    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "v0.4 §49.8: the view closes cleanly after an explanation; saw:\n{}",
        shown(session.seen())
    );
    session.line("exit");
}

#[test]
fn should_stand_the_session_at_the_selected_event_when_at_is_pressed() {
    // §19.3: "A    set session temporal context to event". §4.2 says what that means and §4.6
    // says how it is visible: the prompt carries `[PAST]` — or `[PAST?]` where the coverage is
    // partial — for as long as the session is standing in the past. So the observation is the
    // prompt *after* the view has closed: the coordinate the view moved outlives it, which is
    // what makes it the session's context rather than the view's own cursor.
    let home = support::home_with_a_recorded_action();
    let mut session = Terminal::at_a_prompt(&home);
    session.open_the_timeline();

    let moved = session.mark();
    session.keys(AT);
    session.settle(Duration::from_secs(2));
    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "v0.4 §49.8: the view hands the screen back; saw:\n{}",
        shown(session.since(moved))
    );
    session.settle(Duration::from_millis(800));
    let prompt = tail(session.seen(), ALTERNATE_SCREEN_OFF);
    assert!(
        historical_prompt(&prompt).is_some(),
        "v0.5 §19.3 with §4.2, §4.6: `A` sets the *session's* temporal context to the selected \
         event, so the prompt says the session is standing in the past; saw:\n{prompt}"
    );

    session.line("now");
    session.settle(Duration::from_millis(600));
    session.line("exit");
}

#[test]
fn should_name_every_key_it_answers_when_help_is_asked_for() {
    // §19.3: "?    help". The legend has room for the ten canonical keys and nothing else, and
    // §19.5 requires a grouped row to be expandable without fixing a key for it — so the help is
    // where a key the legend has no room for has to be discoverable, or it is a key nobody knows.
    let home = support::home_with_a_recorded_action();
    let mut session = Terminal::at_a_prompt(&home);
    session.open_the_timeline();

    let asked = session.mark();
    session.keys(HELP);
    assert!(
        session.wait_until(BUDGET, |seen| {
            let frame = frame_rows(&seen[asked.min(seen.len())..]).join("\n");
            frame.contains('X') && frame.contains("expand")
        }),
        "v0.5 §19.3, §19.5: `?` names the keys the view answers, including the one that expands \
         a grouped row into the events it stands for; saw:\n{}",
        shown(session.since(asked))
    );

    session.keys(ANY_KEY);
    session.settle(Duration::from_millis(400));
    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "v0.4 §49.8: the view closes cleanly after the help; saw:\n{}",
        shown(session.seen())
    );
    session.line("exit");
}

#[test]
fn should_say_a_row_stands_for_one_event_when_expansion_is_asked_for_on_an_ungrouped_row() {
    // §19.5: "A grouped event MUST be expandable to individual retained events where they
    // exist." An action lifecycle event is never grouped — §19.4's dimensions are repeated
    // samples and churn, and folding an operator's action into a count would hide the one row a
    // reader is looking for — so expanding this row must say so rather than pretending to open
    // something (§2.17: the shell says what it does not know instead of implying it knows).
    let home = support::home_with_a_recorded_action();
    let mut session = Terminal::at_a_prompt(&home);
    session.open_the_timeline();

    let expanded = session.mark();
    session.keys(EXPAND);
    assert!(
        session.wait_until(BUDGET, |seen| {
            frame_rows(&seen[expanded.min(seen.len())..])
                .join("\n")
                .contains("stands for one event")
        }),
        "v0.5 §19.5: a row that stands for one event has nothing to expand, and the view says so; \
         saw:\n{}",
        shown(session.since(expanded))
    );

    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "v0.4 §49.8: the view closes cleanly; saw:\n{}",
        shown(session.seen())
    );
    session.line("exit");
}

#[test]
fn should_open_the_timeline_at_the_cursor_when_t_is_pressed_in_the_map() {
    // §18.3: "T    open timeline at cursor", and §19.1 makes that one of the two ways the
    // full-screen timeline is opened. Two things have to be true and both are visible from a
    // terminal.
    //
    // The first is that it opens *at the cursor* rather than at now. §11.8: a timeline with an
    // active coordinate centres its window on that coordinate instead of ending it at the
    // present, so a timeline opened from a frozen map cursor states a different window from one
    // opened in the present. Comparing the two headers in one session is what makes that an
    // observation rather than an arithmetic claim about the wall clock.
    //
    // The second is that the map is still there afterwards: `T` opens a view, it does not throw
    // the map away, and the terminal is handed back through both of them.
    let home = support::home_with_a_recorded_action();
    let mut session = Terminal::at_a_prompt(&home);

    session.open_the_timeline();
    let present = header_window(session.seen()).unwrap_or_else(|| {
        panic!(
            "v0.5 §19.2: the timeline states its window; saw:\n{}",
            shown(session.seen())
        )
    });
    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "v0.4 §49.8: the first view closes before the map is opened; saw:\n{}",
        shown(session.seen())
    );

    let opened = session.mark();
    session.line("map");
    assert!(
        session.wait_since(opened, ALTERNATE_SCREEN_ON, BUDGET),
        "v0.4 §23.3: `map` opens the full-screen map; saw:\n{}",
        shown(session.since(opened))
    );
    session.settle(Duration::from_secs(2));
    // §18.2: `Space` freezes the view's temporal cursor, which is what gives `T` an instant of
    // its own to open at rather than the session's.
    session.keys(PAUSE);
    session.settle(Duration::from_secs(1));

    let asked = session.mark();
    session.keys(OPEN_TIMELINE);
    assert!(
        session.wait_since(asked, LEGEND, BUDGET),
        "v0.5 §18.3, §19.1: `T` opens the full-screen timeline from the map; saw:\n{}",
        shown(session.since(asked))
    );
    session.settle(Duration::from_millis(600));
    let at_the_cursor = header_window(session.seen()).unwrap_or_else(|| {
        panic!(
            "v0.5 §19.2: the timeline `T` opened states its window too; saw:\n{}",
            shown(session.since(asked))
        )
    });
    assert_ne!(
        at_the_cursor, present,
        "v0.5 §18.3 with §11.8: `T` opens the timeline at the map's cursor, so its window is \
         centred on that instant rather than ending at now — the window a `timeline --view` in \
         the present states"
    );

    let back = session.mark();
    session.keys(ESCAPE);
    assert!(
        session.wait_until(BUDGET, |seen| {
            frame_rows(&seen[back.min(seen.len())..])
                .join("\n")
                .contains(" close  ")
        }),
        "v0.5 §19.3: `Esc` leaves the timeline, and the map it was opened from is still there; \
         saw:\n{}",
        shown(session.since(back))
    );

    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "v0.4 §49.8: the map closes cleanly after the timeline it opened; saw:\n{}",
        shown(session.seen())
    );
    assert!(session.alive(), "neither view ended the session");
    session.line("exit");
}

#[test]
fn should_restore_the_terminal_on_every_exit_path_when_the_timeline_view_is_left() {
    // v0.4 §49.8 and §52.2 require every full-screen view to be exited cleanly, and v0.4 §43.4
    // names Ctrl-C explicitly: it ends the view and not the shell. A view that restored the
    // terminal only on the quiet path would leave a raw terminal behind for whatever the user
    // runs next, so both exits are driven and the proof is an ordinary external program
    // reporting a cooked line discipline afterwards (v0.4 §44.10).
    let home = support::home_with_a_recorded_action();
    let mut session = Terminal::at_a_prompt(&home);

    for (key, path) in [(ESCAPE, "Esc"), (INTERRUPT, "Ctrl-C")] {
        session.open_the_timeline();
        let left = session.mark();
        session.keys(key);
        assert!(
            session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
            "v0.4 §43.4, §49.8: {path} leaves the timeline and restores the shell's own screen; \
             saw:\n{}",
            shown(session.since(left))
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
            shown(session.since(after))
        );
    }

    let checked = session.mark();
    session.line("stty -a");
    assert!(
        session.wait_since(checked, "icanon", BUDGET),
        "v0.4 §44.10: an external program inherits the terminal the view borrowed; saw:\n{}",
        shown(session.since(checked))
    );
    let reported = shown(session.since(checked));
    assert!(
        !reported.contains("-icanon"),
        "v0.4 §44.10, §49.8: the line discipline is cooked again after every exit path — a raw \
         terminal left behind is what makes the next program unusable; saw:\n{reported}"
    );

    session.line("exit");
}

#[test]
fn should_answer_with_the_text_timeline_when_no_terminal_can_be_taken() {
    // v0.2 §50: behaviour is deterministic when output is redirected, and v0.4 §29.1 forbids a
    // hidden TUI dependency — a script never gets a screen. `ono -c` is not an interactive
    // session, so `--view` has no terminal to draw into and the honest answer is the window
    // itself: the ordinary text timeline, over exactly the same values, with no escape sequence
    // anywhere in it.
    let home = support::home_with_a_recorded_action();
    let run = support::recording_shell(&home, "timeline --view");

    assert!(
        run.status().is_success(),
        "v0.5 §19.1 with v0.2 §50: `timeline --view` without a terminal degrades to the text \
         timeline rather than failing; got {:?}",
        run.output()
    );
    assert!(
        !run.stdout().contains('\u{1b}'),
        "v0.2 §50: nothing writes escape sequences into a pipe; got {:?}",
        run.stdout()
    );
    assert!(
        run.stdout().contains("coverage:"),
        "v0.5 §11.5, §8.5: the text timeline says what window it is a statement about and what \
         the coverage over it was; got {:?}",
        run.output()
    );
}
