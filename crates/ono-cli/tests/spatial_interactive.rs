//! Outcome tests for the part of the v0.4 Spatial Systems Interface that only exists at a real
//! terminal: the startup horizon, `look`'s rendering, the prompt as a place indicator, the
//! ambiguity picker, the full-screen map, and the promise that the shell survives all of it.
//!
//! Narrative: `docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md` — §5 (entry
//! experience and the spatial horizon), §8.3 (expansion is a view action, `enter` is
//! navigation), §9.4 (completion as spatial discovery), §21 (prompt and HUD semantics), §23
//! (map rendering, full-screen half), §24 (`look` rendering rules), §27 (spatial resolution and
//! the interactive ambiguity picker), §29.1 (no hidden TUI dependency — the same session
//! without a terminal), §39 (accessibility and terminal capability), §43.4 (the normative list
//! of PTY interaction tests this file implements), §43.5 (renderer widths), §44.10 (raw shell
//! continuity), §49.8 (full-screen views are entered deliberately and exited cleanly), §52.1
//! ("full-screen map works on supported interactive terminals"), §53 (resolved decisions:
//! focus never moves the shell; the startup horizon is on by default interactively).
//!
//! Every test drives the real `ono` binary through a pseudo-terminal, with `NO_COLOR=1` and a
//! scratch `HOME`, and asserts on what a user would see on that screen — never on how the
//! renderer is wired (AGENTS.md §11). Waits are bounded: an assertion that reports "never
//! appeared" is the failure, not a hang.
//!
//! Key bindings: §23.3 fixes the *semantic* actions and says single keys MAY be remapped. These
//! tests use the defaults that section itself lists — Enter enters the focused node, Backspace
//! goes back, Esc closes the map view preserving the current place — plus Ctrl-C to leave a live
//! map, which §43.4 requires to leave the shell alive. An implementation that remaps them must
//! remap these tests with the same commit.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use ono_process::{Command, Executor, PtySession, Signal, WindowSize};
use ono_testkit::{Scratch, scratch};

/// How long any single screen change may take before the test calls it missing.
///
/// This is a **liveness bound, not a performance assertion**: it exists so that a screen change
/// that never comes fails the run instead of hanging it. Nothing here measures how fast the
/// shell is — v0.4 §34's budgets are asserted by
/// `should_repaint_a_focus_move_far_inside_the_frame_budget_when_the_map_is_open`, which times
/// one repaint against 16 ms, and by `docker/acceptance/cases/100-spatial-performance-budgets.case`,
/// which measures every §34 figure in the container.
///
/// It was eight seconds, and eight seconds is a race the machine wins: `cargo test --workspace`
/// runs a dozen process-spawning suites beside this file, and opening a full-screen map of
/// COMPUTE on a 500-process host costs one whole projection. Two tests here — the picker and the
/// resize — then failed roughly one full gate run in two while passing on their own, which makes
/// the referee unusable and every claim it green-lights worth less (AGENTS.md §14). The
/// assertions are untouched; only the premise that the machine answers within a fixed wall clock
/// is (S11c).
const BUDGET: Duration = Duration::from_secs(45);

/// How long the very first prompt may take (the process still has to start).
///
/// A liveness bound for the same reason and in the same sense as [`BUDGET`].
const STARTUP: Duration = Duration::from_secs(60);

/// The six canonical domains of the root place (spec §7.1, §53 "Root geography").
const DOMAINS: [&str; 6] = [
    "compute",
    "network",
    "storage",
    "containers",
    "identity",
    "devices",
];

/// The alternate screen buffer: entering it is how a full-screen view borrows the terminal, and
/// leaving it is how the shell screen comes back (spec §23.3, §49.8, §52.2).
const ALTERNATE_SCREEN_ON: &str = "\u{1b}[?1049h";
const ALTERNATE_SCREEN_OFF: &str = "\u{1b}[?1049l";

/// The keys §23.3 names for the semantic actions this file exercises.
const DOWN: &[u8] = b"\x1b[B";
const UP: &[u8] = b"\x1b[A";
const TAB: &[u8] = b"\t";
const ENTER: &[u8] = b"\r";
const BACKSPACE: &[u8] = b"\x7f";
const ESCAPE: &[u8] = b"\x1b";
/// Ctrl-C, which §43.4 requires to leave a live map without ending the session.
const INTERRUPT: &[u8] = &[0x03];

/// An interactive `ono` on a pseudo-terminal, plus everything it has painted so far.
struct Session {
    pty: PtySession,
    seen: String,
    buffer: [u8; 16384],
}

impl Session {
    /// Starts `ono` on a pty of `size`, with colour disabled and a scratch home.
    fn start(size: WindowSize, home: &Path, cwd: &Path) -> Self {
        let mut executor = Executor::detached();
        let command = Command::new(ono_testkit::ono_binary())
            .env("TERM", "xterm")
            .env("NO_COLOR", "1")
            .env("HOME", home.display().to_string())
            .current_dir(cwd);
        let pty = executor
            .run_pty(&command, size)
            .expect("a pseudo-terminal must be allocatable");
        Self {
            pty,
            seen: String::new(),
            buffer: [0u8; 16384],
        }
    }

    /// Everything the terminal has shown so far, escape sequences included.
    fn seen(&self) -> &str {
        &self.seen
    }

    /// Reads until `ready` accepts the transcript or the budget runs out.
    fn wait_until(&mut self, budget: Duration, ready: impl Fn(&str) -> bool) -> bool {
        let deadline = Instant::now() + budget;
        loop {
            if ready(&self.seen) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            if let Ok(Some(count)) = self
                .pty
                .read_timeout(&mut self.buffer, Duration::from_millis(120))
            {
                let chunk = String::from_utf8_lossy(&self.buffer[..count]).into_owned();
                self.seen.push_str(&chunk);
            }
        }
    }

    /// Reads until `needle` is on screen.
    fn wait_for(&mut self, needle: &str, budget: Duration) -> bool {
        let needle = needle.to_owned();
        self.wait_until(budget, |seen| seen.contains(&needle))
    }

    /// Reads until `needle` appears in what was painted after `mark`.
    fn wait_for_after(&mut self, mark: &str, needle: &str, budget: Duration) -> bool {
        let mark = mark.to_owned();
        let needle = needle.to_owned();
        self.wait_until(budget, |seen| after(seen, &mark).contains(&needle))
    }

    /// Sends raw key bytes.
    fn keys(&mut self, bytes: &[u8]) {
        self.pty
            .write_all(bytes)
            .expect("the terminal must accept input");
    }

    /// Sends `bytes` and returns how long the terminal took to paint anything in answer.
    ///
    /// `None` means nothing was painted inside `patience` — a key the view had no answer for,
    /// which is not a frame and is not counted.
    fn repaint_after(&mut self, bytes: &[u8], patience: Duration) -> Option<Duration> {
        self.keys(bytes);
        let started = Instant::now();
        let deadline = started + patience;
        while Instant::now() < deadline {
            if let Ok(Some(count)) = self
                .pty
                .read_timeout(&mut self.buffer, Duration::from_millis(1))
                && count > 0
            {
                let elapsed = started.elapsed();
                let chunk = String::from_utf8_lossy(&self.buffer[..count]).into_owned();
                self.seen.push_str(&chunk);
                return Some(elapsed);
            }
        }
        None
    }

    /// Types a line and presses Return.
    fn line(&mut self, text: &str) {
        self.keys(text.as_bytes());
        self.keys(b"\n");
    }

    /// Runs `script` between two `echo` markers and returns exactly what it printed.
    ///
    /// The markers are matched surrounded by line breaks, which the echo of the typed line never
    /// produces, so the returned text is the command's own output and not the line the user
    /// typed.
    fn output_of(&mut self, mark: &str, script: &str) -> String {
        self.line(&format!("echo {mark}BEGIN; {script}; echo {mark}END"));
        let end = format!("\r\n{mark}END\r\n");
        assert!(
            self.wait_for(&end, BUDGET),
            "`{script}` must finish and print again at the prompt; saw:\n{}",
            plain(&self.seen)
        );
        let begin = format!("\r\n{mark}BEGIN\r\n");
        let tail = after(&self.seen, &begin);
        tail.split(&end).next().unwrap_or_default().to_owned()
    }

    /// Keeps reading for `patience`, so that everything a command painted has arrived.
    fn settle(&mut self, patience: Duration) {
        let deadline = Instant::now() + patience;
        while Instant::now() < deadline {
            if let Ok(Some(count)) = self
                .pty
                .read_timeout(&mut self.buffer, Duration::from_millis(100))
            {
                let chunk = String::from_utf8_lossy(&self.buffer[..count]).into_owned();
                self.seen.push_str(&chunk);
            }
        }
    }

    /// Draws a fresh prompt and returns the text of it, so a test can read the place it names.
    ///
    /// The marker is echoed first; everything the shell paints afterwards is the prompt, and the
    /// screen is read without escape sequences because it is what the prompt *says* that matters.
    fn prompt(&mut self, mark: &str) -> String {
        self.line(&format!("echo {mark}"));
        let marker = format!("\r\n{mark}\r\n");
        assert!(
            self.wait_for(&marker, BUDGET),
            "the shell must return to its prompt; saw:\n{}",
            plain(&self.seen)
        );
        self.settle(Duration::from_millis(400));
        let drawn = plain(&after(&self.seen, &marker)).trim().to_owned();
        assert!(
            !drawn.is_empty(),
            "§21: the shell paints a prompt after every command; saw:\n{}",
            plain(&self.seen)
        );
        drawn
    }

    /// Resizes the window the shell believes it is drawing into.
    fn resize(&mut self, size: WindowSize) {
        self.pty
            .resize(size)
            .expect("the pseudo-terminal must be resizable");
    }

    /// Is the shell still running?
    fn alive(&mut self) -> bool {
        matches!(self.pty.try_wait(), Ok(None))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.pty.signal(Signal::KILL);
        let _ = self.pty.wait();
    }
}

/// The transcript after the last occurrence of `mark`, or everything when it never appeared.
fn after(text: &str, mark: &str) -> String {
    text.rsplit_once(mark)
        .map_or_else(|| text.to_owned(), |(_, tail)| tail.to_owned())
}

/// The text without terminal escape sequences, as a reader of the screen would see it.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '\u{1b}' {
            out.push(character);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                for next in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                for next in chars.by_ref() {
                    if next == '\u{7}' {
                        break;
                    }
                }
            }
            _ => {
                chars.next();
            }
        }
    }
    out
}

/// The names of the canonical domains missing from `screen`, lower-cased comparison.
fn domains_missing(screen: &str) -> Vec<&'static str> {
    let lower = screen.to_lowercase();
    DOMAINS
        .into_iter()
        .filter(|domain| !lower.contains(domain))
        .collect()
}

/// Where a whole number occurs in `text`, ignoring occurrences inside a longer number.
fn number_at(text: &str, number: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    text.match_indices(number)
        .find(|(index, _)| {
            let before = index
                .checked_sub(1)
                .is_none_or(|previous| !bytes[previous].is_ascii_digit());
            let after = bytes
                .get(index + number.len())
                .is_none_or(|byte| !byte.is_ascii_digit());
            before && after
        })
        .map(|(index, _)| index)
}

/// The pid of the place `look --json` describes, as the `PlaceView` of §6.1 writes it.
///
/// The document arrives with the terminal's carriage returns in it, which no JSON parser has to
/// tolerate, so they are dropped before it is read.
fn place_pid(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let document = text.get(start..=end)?.replace('\r', "");
    let view: serde_yaml_ng::Value = serde_yaml_ng::from_str(&document).ok()?;
    let pid = view.get("place")?.get("pid")?;
    pid.as_u64().map(|pid| pid.to_string())
}

/// A scratch home plus an empty working directory, so nothing on this machine leaks in.
fn workspace() -> (Scratch, PathBuf) {
    let home = scratch();
    let work = home.path().join("work");
    std::fs::create_dir_all(&work).expect("a working directory");
    (home, work)
}

/// The path of `name` on `PATH`, for building a fixture out of a real program.
fn program_path(name: &str) -> PathBuf {
    let path = std::env::var_os("PATH").expect("PATH must be set");
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| panic!("the fixture needs `{name}` on PATH"))
}

/// Children the test started, killed when it ends however it ends.
struct Children(Vec<Child>);

impl Drop for Children {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Starts two processes that share one unmistakable name, so a selector for that name matches
/// two different places and nothing else on the machine (spec §27.2).
fn twins(scratch: &Scratch, name: &str) -> (Children, String, String) {
    let binary = scratch.path().join("bin").join(name);
    std::fs::create_dir_all(binary.parent().expect("a parent")).expect("a bin directory");
    std::fs::copy(program_path("sleep"), &binary).expect("a renamed copy of `sleep`");
    // `ETXTBSY` is a race between the copy above and any *other* test thread that forks in the
    // window before the copy's write descriptor is closed: the forked child inherits it, and the
    // kernel refuses to execute a file some process holds open for writing. It is a property of
    // running a dozen process-spawning tests in one binary, not of the shell, so it is waited
    // out rather than asserted on.
    let spawn = || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match std::process::Command::new(&binary)
                .arg("300")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(child) => return child,
                Err(error)
                    if error.kind() == std::io::ErrorKind::ExecutableFileBusy
                        && Instant::now() < deadline => {}
                Err(error) => panic!("the fixture process must start: {error:?}"),
            }
        }
    };
    let children = Children(vec![spawn(), spawn()]);
    let first = children.0[0].id().to_string();
    let second = children.0[1].id().to_string();
    (children, first, second)
}

#[test]
fn should_show_the_spatial_horizon_when_the_session_starts_at_a_terminal_and_never_in_a_pipe() {
    // §5: an interactive start MUST establish place and nearby possibilities without an explicit
    // discovery command — host identity, the canonical domains, compact counts, landmarks, and a
    // prompt naming the spatial scope. §53 makes it the default interactively. §29.1 is the other
    // half of the same rule: nothing spatial may be printed where there is no terminal, so a
    // script's stdout stays exactly what the script asked for.
    let (home, work) = workspace();
    let mut session = Session::start(WindowSize::new(30, 100), home.path(), &work);
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        plain(session.seen())
    );
    let screen = plain(session.seen());
    assert!(
        domains_missing(&screen).is_empty(),
        "§5: the startup horizon names the canonical domains as destinations, missing {:?}; saw:\n{screen}",
        domains_missing(&screen)
    );
    assert!(
        screen.to_lowercase().contains("local"),
        "§5: the horizon names the current host/context identity; saw:\n{screen}"
    );

    let script = ono_testkit::Shell::new()
        .args(["-c", "echo hi"])
        .env("NO_COLOR", "1")
        .env("HOME", home.path().display().to_string())
        .run();
    script.assert_success();
    assert_eq!(
        script.stdout().trim(),
        "hi",
        "§29.1: without a terminal the horizon is not printed at all, got {:?}",
        script.stdout()
    );
    assert!(
        domains_missing(script.stderr()).len() == DOMAINS.len(),
        "§29.1: a script's stderr carries no horizon either, got {:?}",
        script.stderr()
    );

    session.line("exit");
}

#[test]
fn should_describe_the_current_place_when_look_runs_at_eighty_columns() {
    // §24.1: `look` prioritises identity and state, direct exits, landmarks, changes and summary
    // counts — and §24.2 makes the displayed groups real navigation targets, which is why the
    // root's exits are the six canonical domains of §7.1. §39.1: with colour disabled the same
    // distinctions must still be readable, so the body carries no escape sequences at all.
    let (home, work) = workspace();
    let mut session = Session::start(WindowSize::new(24, 80), home.path(), &work);
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        plain(session.seen())
    );

    let body = session.output_of("LK80", "look");
    let screen = plain(&body);
    assert!(
        domains_missing(&screen).is_empty(),
        "§24.2: `look` at the root shows the canonical domains as exits, missing {:?}; saw:\n{screen}",
        domains_missing(&screen)
    );
    assert!(
        screen.to_lowercase().contains("local"),
        "§6.1/§24.1: `look` states the identity of the current place; saw:\n{screen}"
    );
    assert!(
        !body.contains('\u{1b}'),
        "§39.1: with NO_COLOR the rendering carries no escape sequences; saw:\n{body:?}"
    );

    session.line("exit");
}

#[test]
fn should_keep_the_same_spatial_semantics_when_look_runs_at_forty_columns() {
    // §39.3: at narrow widths the projection may collapse, but "spatial semantics remain
    // identical" — the place and its exits are still there. §43.5 lists 40 columns as a
    // representative width and forbids turning layout into a semantic contract, so the only
    // layout claim here is the one legibility needs: nothing is drawn past the right edge.
    let (home, work) = workspace();
    let mut session = Session::start(WindowSize::new(24, 40), home.path(), &work);
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        plain(session.seen())
    );

    let body = session.output_of("LK40", "look");
    let screen = plain(&body);
    assert!(
        domains_missing(&screen).is_empty(),
        "§39.3: the same exits are reachable at 40 columns, missing {:?}; saw:\n{screen}",
        domains_missing(&screen)
    );
    let overflowing: Vec<&str> = screen
        .lines()
        .map(str::trim_end)
        .filter(|line| line.chars().count() > 40)
        .collect();
    assert!(
        overflowing.is_empty(),
        "§39.3/§24.1: the rendering fits the terminal it was asked for, overflowing lines {overflowing:?}"
    );

    session.line("exit");
}

#[test]
fn should_name_the_current_place_in_the_prompt_and_follow_it_when_the_place_changes() {
    // §21.1: the prompt's semantic components are the link/host and the current place, and §21.2
    // has it show `<host>/<current-place-kind>/<display-name>` rather than the whole trail —
    // `trail` keeps the history. §53: entering a non-directory object does not touch cwd, so the
    // only thing that may move here is the spatial place.
    let (home, work) = workspace();
    let mut session = Session::start(WindowSize::new(24, 100), home.path(), &work);
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        plain(session.seen())
    );

    let root = session.prompt("PR0");
    assert!(
        root.to_lowercase().contains("local"),
        "§21.1: the prompt names the host/link, got {root:?}"
    );
    assert!(
        !root.to_lowercase().contains("compute"),
        "the session starts at the root place, got {root:?}"
    );

    session.line("enter compute");
    let inside = session.prompt("PR1");
    assert!(
        inside.to_lowercase().contains("compute"),
        "§21.1/§21.2: the prompt follows the current place, got {inside:?} after `enter compute`"
    );
    assert!(
        inside.to_lowercase().contains("local"),
        "§21.1: the host segment survives the move, got {inside:?}"
    );

    let trail = session.output_of("TR1", "trail");
    assert!(
        plain(&trail).to_lowercase().contains("compute"),
        "§21.2: the full movement history lives in `trail`, not in the prompt; saw:\n{trail}"
    );

    session.line("home");
    let back_home = session.prompt("PR2");
    assert!(
        !back_home.to_lowercase().contains("compute"),
        "§6.6: `home` returns to the root place, got {back_home:?}"
    );

    session.line("exit");
}

#[test]
fn should_open_a_picker_and_make_the_choice_current_when_a_selector_is_ambiguous() {
    // §27.2: interactive ambiguity opens a picker, and the picker MUST show disambiguating
    // context (the example rows carry the kind and the place path). A script never sees this:
    // §29.3 turns the same ambiguity into a structured `spatial.ambiguous_selector` error, which
    // another suite asserts without a terminal.
    //
    // The fixture is two processes sharing a name that exists nowhere else on the machine, so the
    // ambiguity is real, local and unprivileged. The reading taken here: the picker starts on its
    // first row, one Down moves to the second, and Enter adopts it — §23.3's key table is the
    // nearest normative binding and it fixes Enter as "enter focused node".
    let (home, work) = workspace();
    let name = format!("onoamb{}", std::process::id());
    let (_children, first, second) = twins(&home, &name);

    let mut session = Session::start(WindowSize::new(30, 100), home.path(), &work);
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        plain(session.seen())
    );
    let root = session.prompt("AM0");

    session.line(&format!("enter {name}"));
    assert!(
        session.wait_until(BUDGET, |seen| {
            let tail = plain(&after(seen, "\r\nAM0\r\n"));
            number_at(&tail, &first).is_some() && number_at(&tail, &second).is_some()
        }),
        "§27.2: an ambiguous selector opens a picker listing every candidate ({first}, {second}); saw:\n{}",
        plain(&after(session.seen(), "\r\nAM0\r\n"))
    );

    let listed = plain(&after(session.seen(), "\r\nAM0\r\n"));
    assert!(
        listed.to_lowercase().contains("process"),
        "§27.2: each row carries its disambiguating context — kind and place; saw:\n{listed}"
    );
    let chosen = if number_at(&listed, &second) > number_at(&listed, &first) {
        second.clone()
    } else {
        first.clone()
    };

    session.keys(DOWN);
    session.keys(ENTER);
    let picked = session.prompt("AM1");
    assert_ne!(
        picked, root,
        "§27.2: the picked candidate becomes the current place, prompt unchanged at {root:?}"
    );

    // The two candidates share their name, their kind and their place path, so the *rendered*
    // view names neither of them uniquely — it only ever showed the pid where a permission error
    // happened to quote a `/proc/<pid>` path, which is a property of the host the test ran on and
    // not of the behaviour it states (AGENTS.md section 11). §6.1's `look --json` carries the
    // identity the provider filed the place under, and §29.4 makes `pid` a field of the place
    // itself, so the answer to "which of the two am I standing in" is read where it is written.
    let body = plain(&session.output_of("AM2", "look --json"));
    assert_eq!(
        place_pid(&body).as_deref(),
        Some(chosen.as_str()),
        "§27.2: the second listed candidate ({chosen}) is the place that was entered; saw:\n{body}"
    );

    session.line("exit");
}

#[test]
fn should_restore_the_shell_screen_when_the_full_screen_map_closes() {
    // §52.1 requires the full-screen map on supported interactive terminals, §23.3 gives Esc as
    // "close map view, preserving current place", and §49.8 insists the shell is not a dashboard:
    // the view is entered deliberately and left cleanly, with the shell's own screen back and the
    // next command running normally.
    let (home, work) = workspace();
    let mut session = Session::start(WindowSize::new(30, 100), home.path(), &work);
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        plain(session.seen())
    );
    let before = session.prompt("MP0");

    session.line("map");
    assert!(
        session.wait_for_after("\r\nMP0\r\n", ALTERNATE_SCREEN_ON, BUDGET),
        "§23.3/§52.1: `map` opens a full-screen view on an interactive terminal; saw:\n{}",
        plain(&after(session.seen(), "\r\nMP0\r\n"))
    );
    // The screen is taken before the projection exists (v0.4.1 §33.1, issue #20), so the frame
    // with the exits in it is the second thing painted rather than the first.
    assert!(
        session.wait_until(BUDGET, |seen| {
            domains_missing(&plain(&after(seen, ALTERNATE_SCREEN_ON))).len() < DOMAINS.len()
        }),
        "§23.1: the map draws the current place and its canonical exits; saw:\n{}",
        plain(&after(session.seen(), ALTERNATE_SCREEN_ON))
    );

    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "§23.3/§49.8: Esc closes the view and gives the shell screen back; saw:\n{}",
        plain(&after(session.seen(), ALTERNATE_SCREEN_ON))
    );

    let after_map = session.output_of("MP1", "echo still-here");
    assert!(
        after_map.contains("still-here"),
        "§49.8: the shell keeps working after the view closes; saw:\n{after_map}"
    );
    assert_eq!(
        session.prompt("MP2"),
        before,
        "§23.3: closing the map preserves the current place"
    );

    session.line("exit");
}

#[test]
fn should_change_the_place_only_on_enter_when_focus_moves_inside_the_map() {
    // §23.4 and §53 ("Does focus move the shell? No"): moving focus is a view action, exactly as
    // expanding a cluster is in §8.3; only Enter or an explicit navigation action moves the
    // shell. So: browse with the arrow keys, close, and the place is untouched; then focus a node
    // and press Enter, and the place follows.
    let (home, work) = workspace();
    let mut session = Session::start(WindowSize::new(30, 100), home.path(), &work);
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        plain(session.seen())
    );
    let root = session.prompt("FC0");

    session.line("map");
    assert!(
        session.wait_for_after("\r\nFC0\r\n", ALTERNATE_SCREEN_ON, BUDGET),
        "§52.1: `map` opens a full-screen view; saw:\n{}",
        plain(&after(session.seen(), "\r\nFC0\r\n"))
    );
    session.keys(DOWN);
    session.keys(DOWN);
    session.keys(TAB);
    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "the view closes again; saw:\n{}",
        plain(&after(session.seen(), ALTERNATE_SCREEN_ON))
    );
    assert_eq!(
        session.prompt("FC1"),
        root,
        "§23.4/§53: moving focus inside the map never moves the shell's current place"
    );

    session.line("map");
    assert!(
        session.wait_for_after("\r\nFC1\r\n", ALTERNATE_SCREEN_ON, BUDGET),
        "the view opens a second time; saw:\n{}",
        plain(&after(session.seen(), "\r\nFC1\r\n"))
    );
    session.keys(DOWN);
    session.keys(ENTER);
    session.keys(ESCAPE);
    assert!(
        session.wait_for_after("\r\nFC1\r\n", ALTERNATE_SCREEN_OFF, BUDGET),
        "the view closes after Enter; saw:\n{}",
        plain(&after(session.seen(), "\r\nFC1\r\n"))
    );
    let entered = session.prompt("FC2");
    assert_ne!(
        entered, root,
        "§23.4: Enter on the focused node is navigation and does change the current place"
    );
    assert!(
        DOMAINS
            .into_iter()
            .any(|domain| entered.to_lowercase().contains(domain)),
        "§23.3: Enter entered one of the nodes the root map draws, got {entered:?}"
    );

    session.line("exit");
}

#[test]
fn should_return_to_the_previous_place_when_back_is_used_at_the_prompt_and_in_the_map() {
    // §6.6: `back` follows navigation history. §43.4 requires the same semantic action inside the
    // full-screen view, and §23.3 binds it to `b`/Backspace there; Backspace (0x7f) is the one
    // used here because it is the binding a user reaches for without reading the key table.
    let (home, work) = workspace();
    let mut session = Session::start(WindowSize::new(30, 100), home.path(), &work);
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        plain(session.seen())
    );
    let root = session.prompt("BK0");

    session.line("enter compute");
    let inside = session.prompt("BK1");
    assert_ne!(inside, root, "§6.3: `enter compute` moves the place");
    session.line("back");
    assert_eq!(
        session.prompt("BK2"),
        root,
        "§6.6: `back` follows navigation history to the place before the move"
    );

    session.line("map");
    assert!(
        session.wait_for_after("\r\nBK2\r\n", ALTERNATE_SCREEN_ON, BUDGET),
        "§52.1: `map` opens a full-screen view; saw:\n{}",
        plain(&after(session.seen(), "\r\nBK2\r\n"))
    );
    session.keys(DOWN);
    session.keys(ENTER);
    session.keys(BACKSPACE);
    session.keys(ESCAPE);
    assert!(
        session.wait_for_after("\r\nBK2\r\n", ALTERNATE_SCREEN_OFF, BUDGET),
        "the view closes again; saw:\n{}",
        plain(&after(session.seen(), "\r\nBK2\r\n"))
    );
    assert_eq!(
        session.prompt("BK3"),
        root,
        "§43.4: Backspace inside the map returns to the place Enter came from"
    );

    session.line("exit");
}

#[test]
fn should_keep_the_shell_alive_when_ctrl_c_ends_the_live_map() {
    // §43.4: "Ctrl-C exits live map without killing the shell". §25.1 makes `map --live` a
    // subscription that updates topology in place; ending it must leave the session, its place
    // and its prompt intact. The exit status of an abandoned view is not fixed by the spec, so
    // this asserts what the spec does fix: the shell survives and the next command runs.
    let (home, work) = workspace();
    let mut session = Session::start(WindowSize::new(30, 100), home.path(), &work);
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        plain(session.seen())
    );
    let before = session.prompt("LV0");

    session.line("map --live");
    assert!(
        session.wait_for_after("\r\nLV0\r\n", ALTERNATE_SCREEN_ON, BUDGET),
        "§25.1: `map --live` opens the live view; saw:\n{}",
        plain(&after(session.seen(), "\r\nLV0\r\n"))
    );

    session.keys(INTERRUPT);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "§43.4: Ctrl-C leaves the live map and restores the shell screen; saw:\n{}",
        plain(&after(session.seen(), ALTERNATE_SCREEN_ON))
    );
    assert!(
        session.alive(),
        "§43.4: Ctrl-C ends the view, not the shell"
    );
    let body = session.output_of("LV1", "echo survived");
    assert!(
        body.contains("survived"),
        "§43.4: the prompt comes back and the next command runs; saw:\n{body}"
    );
    assert_eq!(
        session.prompt("LV2"),
        before,
        "§23.3: leaving the view preserves the current place"
    );

    session.line("exit");
}

#[test]
fn should_preserve_the_current_place_when_the_terminal_is_resized_with_a_place_open() {
    // §43.4: "terminal resize preserves current place and focus where possible". §39.3 allows the
    // projection to collapse at the new width; what may not change is where the user is. Focus
    // preservation is asserted as far as a transcript can see it: the node the map was drawing
    // before the resize is still drawn after it.
    let (home, work) = workspace();
    let mut session = Session::start(WindowSize::new(30, 100), home.path(), &work);
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        plain(session.seen())
    );

    session.line("enter compute");
    let inside = session.prompt("RS0");
    assert!(
        inside.to_lowercase().contains("compute"),
        "§21.1: the place is open before the resize, got {inside:?}"
    );

    session.line("map");
    assert!(
        session.wait_for_after("\r\nRS0\r\n", ALTERNATE_SCREEN_ON, BUDGET),
        "§52.1: `map` opens a full-screen view; saw:\n{}",
        plain(&after(session.seen(), "\r\nRS0\r\n"))
    );
    session.keys(DOWN);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, "compute", BUDGET),
        "§23.1: the map names the place it is drawing; saw:\n{}",
        plain(&after(session.seen(), ALTERNATE_SCREEN_ON))
    );

    // The frame before the resize is a frame at thirty rows, which is what makes the assertion
    // after it mean something.
    let painted = after(session.seen(), ALTERNATE_SCREEN_ON);
    let before = rows_addressed(&painted);
    assert!(
        before.contains(&30),
        "the map fills the terminal it was given, so the frame before the resize reaches row 30; \
         it addressed {before:?}"
    );

    let mark = session.seen().len();
    session.resize(WindowSize::new(20, 60));
    // §43.4 asks that a resize preserve the place, and the only way to see that a resize happened
    // at all is the geometry of the frame it caused. Waiting for "new output naming the place"
    // was satisfied by the repaint the earlier `Down` was still producing at thirty rows, so this
    // test passed on runs whose whole key history was `Down`, `Esc`, with no resize in it (#6).
    // A frame that addresses row 20 and no row above it is a frame at the new row count, and
    // nothing an earlier repaint produced can be one.
    assert!(
        session.wait_until(BUDGET, |seen| {
            if seen.len() <= mark {
                return false;
            }
            frames(&seen[mark..]).into_iter().any(|frame| {
                let rows = rows_addressed(frame);
                rows.contains(&20)
                    && rows.iter().all(|row| *row <= 20)
                    && plain(frame).to_lowercase().contains("compute")
            })
        }),
        "§43.4/§39.3: the map redraws at the new twenty-row size and still shows the open place. \
         The frames it painted addressed rows {:?}; saw:\n{}",
        frames(&session.seen()[mark.min(session.seen().len())..])
            .into_iter()
            .map(rows_addressed)
            .collect::<Vec<_>>(),
        plain(&session.seen()[mark.min(session.seen().len())..])
    );

    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "the view closes; saw:\n{}",
        plain(&after(session.seen(), ALTERNATE_SCREEN_ON))
    );
    assert_eq!(
        session.prompt("RS1"),
        inside,
        "§43.4: a resize preserves the current place"
    );

    session.line("exit");
}

#[test]
fn should_leave_the_terminal_in_order_for_an_external_program_after_the_map_closes() {
    // §44.10: after extensive navigation and full-screen map use, `vim`, `less`, `ssh` and
    // `cargo test` must still work — interactive process control, terminal state and cwd remain
    // correct. §52.2 says the same as a release criterion. `stty -a` is the observable: it is an
    // ordinary external program, it runs on the terminal it inherited, and it reports the line
    // discipline and the window size the shell handed back.
    let (home, work) = workspace();
    std::fs::create_dir_all(work.join("deeper")).expect("a directory to be in");
    let mut session = Session::start(WindowSize::new(24, 80), home.path(), &work);
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        plain(session.seen())
    );

    session.line("cd deeper");
    session.line("enter compute");
    let mark = session.prompt("RW0");
    assert!(
        mark.to_lowercase().contains("compute"),
        "§21.1: the session navigated before the map, got {mark:?}"
    );

    session.line("map");
    assert!(
        session.wait_for_after("\r\nRW0\r\n", ALTERNATE_SCREEN_ON, BUDGET),
        "§52.1: `map` opens a full-screen view; saw:\n{}",
        plain(&after(session.seen(), "\r\nRW0\r\n"))
    );
    session.keys(DOWN);
    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "the view closes; saw:\n{}",
        plain(&after(session.seen(), ALTERNATE_SCREEN_ON))
    );

    let state = plain(&session.output_of("RW1", "stty -a"));
    assert!(
        state.contains("icanon") && !state.contains("-icanon"),
        "§44.10: an external program inherits a canonical-mode terminal after the view closed; saw:\n{state}"
    );
    assert!(
        state.contains("rows 24") && state.contains("columns 80"),
        "§44.10: the window size the shell was given is the one it hands on; saw:\n{state}"
    );

    let where_we_are = plain(&session.output_of("RW2", "pwd"));
    assert!(
        where_we_are.trim().ends_with("/deeper"),
        "§44.10/§53: cwd survives spatial navigation and the full-screen view; saw:\n{where_we_are}"
    );

    session.line("exit");
}

#[test]
fn should_offer_the_neighbouring_places_when_completion_runs_at_the_prompt() {
    // §9.4: completion is "a lightweight local map" — at `enter <TAB>` it MUST prioritise what is
    // visible in the current neighbourhood, which at the root is the canonical domains of §7.1.
    // The working directory is an empty scratch tree, so nothing that appears can have come from
    // filename completion.
    let (home, work) = workspace();
    let mut session = Session::start(WindowSize::new(30, 100), home.path(), &work);
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        plain(session.seen())
    );
    let mark = session.prompt("CP0");
    assert!(
        !mark.to_lowercase().contains("compute"),
        "the prompt itself must not be what the assertion below finds, got {mark:?}"
    );

    session.keys(b"enter \t");
    assert!(
        session.wait_until(BUDGET, |seen| {
            let offered = plain(&after(seen, "\r\nCP0\r\n")).to_lowercase();
            offered.contains("compute") && offered.contains("network")
        }),
        "§9.4: completing `enter` offers the places of the current neighbourhood; saw:\n{}",
        plain(&after(session.seen(), "\r\nCP0\r\n"))
    );

    session.keys(INTERRUPT);
    session.line("exit");
}

#[test]
fn should_repaint_a_focus_move_far_inside_the_frame_budget_when_the_map_is_open() {
    // §34: "focus/navigation inside rendered map < 16 ms frame target". `docs/ACCEPTANCE.md`
    // §4.7.5 asks for that budget at a real pseudo-terminal, in the shape of
    // `crates/ono-editor/tests/latency.rs` (§4.3): the whole keystroke-to-paint path is timed,
    // not the renderer alone, and the figure asserted is the spec's own — the measurement sits
    // two orders below it, so machine noise cannot decide the release while a quadratic redraw
    // still fails the gate.
    //
    // The median is taken, because one scheduling hiccup on a shared machine is not a frame
    // cost. A key the view had nothing to answer is not counted as a frame at all.
    let (home, work) = workspace();
    let mut session = Session::start(WindowSize::new(30, 100), home.path(), &work);
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        plain(session.seen())
    );
    session.prompt("FB0");
    session.line("map");
    assert!(
        session.wait_for_after("\r\nFB0\r\n", ALTERNATE_SCREEN_ON, BUDGET),
        "§52.1: `map` opens a full-screen view; saw:\n{}",
        plain(&after(session.seen(), "\r\nFB0\r\n"))
    );
    // Everything the opening paint produced has to be off the wire before a frame can be timed.
    session.settle(Duration::from_millis(600));

    let mut frames: Vec<Duration> = Vec::new();
    for step in 0..40 {
        let key = if step % 2 == 0 { DOWN } else { UP };
        if let Some(elapsed) = session.repaint_after(key, Duration::from_secs(2)) {
            frames.push(elapsed);
        }
    }
    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "the view closes again; saw:\n{}",
        plain(&after(session.seen(), ALTERNATE_SCREEN_ON))
    );
    session.line("exit");

    assert!(
        frames.len() >= 30,
        "§23.4: every focus move repaints the view; only {} of 40 did",
        frames.len()
    );
    frames.sort_unstable();
    let median = frames[frames.len() / 2];
    assert!(
        median <= Duration::from_millis(16),
        "§34: a focus move inside the rendered map is budgeted at 16 ms per frame; the median of \
         {} frames was {median:?} (slowest {:?})",
        frames.len(),
        frames.last().copied().unwrap_or_default()
    );
}

// --- a projection in flight does not take the terminal with it (v0.4.1 §33.1, §35.2; issue #20)

/// How long the terminal may stay blank after `map` before the view has failed to open.
///
/// This one *is* a discriminator rather than a liveness bound, and the margin is measured:
/// projecting COMPUTE on a Profile M host costs 1.84 s in a debug build, so a shell that opened
/// the screen only after the projection could not reach this in any run. It is deliberately far
/// below that figure and far above the few milliseconds the paint itself costs.
const FIRST_FRAME: Duration = Duration::from_millis(700);

#[test]
fn should_answer_focus_movement_while_a_projection_is_still_running() {
    // Issue #20: "a full-screen map of COMPUTE is unresponsive while one projection is in
    // flight". v0.4 §34 requires the shell to "remain interactive and progressively update rather
    // than block unnecessarily", and v0.4.1 §33.1 makes time to first useful result a first-class
    // target — so the screen has to exist before the picture does, and it has to be truthful
    // about the picture not being there yet (§35.2, §2.17).
    //
    // The host is held at Profile M for the whole test, so the projection is the expensive thing
    // the issue is about rather than whatever the machine happened to be running (§32.1).
    let _population = ono_testkit::ProcessPopulation::of(ono_testkit::PROFILE_M);
    let (home, work) = workspace();
    let mut session = Session::start(WindowSize::new(30, 100), home.path(), &work);
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        plain(session.seen())
    );
    session.line("enter compute");
    session.prompt("IF0");
    session.line("map");

    assert!(
        session.wait_for_after("\r\nIF0\r\n", ALTERNATE_SCREEN_ON, FIRST_FRAME),
        "§33.1: the full-screen view takes the terminal before it has a projection to draw, so \
         the user is never looking at a blank screen while COMPUTE is being observed; saw:\n{}",
        plain(&after(session.seen(), "\r\nIF0\r\n"))
    );
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, "projecting", FIRST_FRAME),
        "§35.2: a first frame that does not hold the detail must say so rather than look like an \
         empty place; saw:\n{}",
        plain(&after(session.seen(), ALTERNATE_SCREEN_ON))
    );

    // A key pressed while the projection is still running is answered rather than swallowed
    // (ADR-0424), so the view acts on it once the picture lands.
    session.keys(DOWN);
    session.keys(DOWN);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, "COMPUTE", BUDGET),
        "the projection lands and the view draws the place; saw:\n{}",
        plain(&after(session.seen(), ALTERNATE_SCREEN_ON))
    );
    assert!(
        session.repaint_after(UP, Duration::from_secs(5)).is_some(),
        "§23.4: focus movement repaints the view once the projection is in; saw:\n{}",
        plain(&after(session.seen(), ALTERNATE_SCREEN_ON))
    );

    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "the view closes again; saw:\n{}",
        plain(&after(session.seen(), ALTERNATE_SCREEN_ON))
    );
    session.line("exit");
}

#[test]
fn should_close_the_full_screen_map_promptly_while_a_projection_is_in_flight() {
    // §35.5's rule for the live map, and ADR-0424's for the view: the one key whose whole purpose
    // is to get out must work while the view is busy. Before the screen was taken ahead of the
    // projection there was nothing to press it into — the terminal was still cooked and no key
    // was read at all until COMPUTE had been observed.
    //
    // The whole of `map` to the screen coming back is bounded below the 1.84 s the projection
    // itself costs at Profile M in a debug build, so a close that waited for the projection
    // cannot pass.
    let _population = ono_testkit::ProcessPopulation::of(ono_testkit::PROFILE_M);
    let (home, work) = workspace();
    let mut session = Session::start(WindowSize::new(30, 100), home.path(), &work);
    assert!(
        session.wait_for("> ", STARTUP),
        "the shell must reach a prompt; saw:\n{}",
        plain(session.seen())
    );
    session.line("enter compute");
    session.prompt("CL0");

    let opened = Instant::now();
    session.line("map");
    assert!(
        session.wait_for_after("\r\nCL0\r\n", ALTERNATE_SCREEN_ON, FIRST_FRAME),
        "the view takes the terminal before the projection is done; saw:\n{}",
        plain(&after(session.seen(), "\r\nCL0\r\n"))
    );
    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, FIRST_FRAME),
        "§34, ADR-0424: leaving must not wait for the projection to finish; saw:\n{}",
        plain(&after(session.seen(), ALTERNATE_SCREEN_ON))
    );
    let left = opened.elapsed();
    assert!(
        left < Duration::from_millis(1_500),
        "opening and leaving the view took {left:?}, which is the cost of the projection it was \
         supposed not to wait for"
    );

    // And the shell is still there, in the place it was in.
    let answered = session.prompt("CL1");
    assert!(
        session.wait_for("CL1", BUDGET),
        "the shell survives the view it just left; saw:\n{answered}\n{}",
        plain(session.seen())
    );
    session.line("exit");
}

#[test]
fn should_paint_no_frame_at_a_new_row_count_when_the_terminal_is_not_resized() {
    // The other half of #6, and the reason the test above is worth its complication: without the
    // resize, the same key sequence paints frames at the size the terminal has had all along.
    // This is the run whose whole key history is `Down`, `Esc` — the one that satisfied the old
    // assertion — and here it is red by construction, so a resize that stops arriving turns the
    // test above red instead of leaving it silently green (§65.10).
    let (home, work) = workspace();
    let mut session = Session::start(WindowSize::new(30, 100), home.path(), &work);
    assert!(session.wait_for("> ", STARTUP));

    session.line("enter compute");
    let _ = session.prompt("NR0");
    session.line("map");
    assert!(
        session.wait_for_after("\r\nNR0\r\n", ALTERNATE_SCREEN_ON, BUDGET),
        "§52.1: `map` opens a full-screen view; saw:\n{}",
        plain(&after(session.seen(), "\r\nNR0\r\n"))
    );

    let mark = session.seen().len();
    session.keys(DOWN);
    // Waited for the way the resized half waits: for a frame with the geometry the assertion is
    // about, and not for the first sign that a frame started. A repaint reaches the terminal a
    // row at a time, and the label naming the place is written before the rows beneath it — so
    // reading the buffer as soon as "compute" appears reads a frame that is still being written,
    // and on a machine slow enough to split the write it had reached row 18 and no further.
    // BUDGET is the watchdog on a repaint that never comes (ADR-0517); nothing here waits on a
    // duration it asserts.
    assert!(
        session.wait_until(BUDGET, |seen| {
            if seen.len() <= mark {
                return false;
            }
            frames(&seen[mark..]).into_iter().any(|frame| {
                rows_addressed(frame).iter().any(|row| *row > 20)
                    && plain(frame).to_lowercase().contains("compute")
            })
        }),
        "§43.4: a repaint with no resize behind it is a frame at the terminal's own thirty rows, \
         and would be indistinguishable from a resized one if the assertion only asked for new \
         output naming the place. The frames it painted addressed rows {:?}; saw:\n{}",
        frames(&session.seen()[mark.min(session.seen().len())..])
            .into_iter()
            .map(rows_addressed)
            .collect::<Vec<_>>(),
        plain(&session.seen()[mark.min(session.seen().len())..])
    );

    session.keys(ESCAPE);
    assert!(
        session.wait_for_after(ALTERNATE_SCREEN_ON, ALTERNATE_SCREEN_OFF, BUDGET),
        "the view closes; saw:\n{}",
        plain(&after(session.seen(), ALTERNATE_SCREEN_ON))
    );
    session.line("exit");
}

/// The frames a passage of output holds, split where each one starts.
///
/// The map repaints by homing the cursor and addressing every row in turn, so `ESC [ 1 ; 1 H`
/// is where one frame ends and the next begins. Reading a window of output as one frame would
/// merge the repaint that was still in flight with the one the resize caused, which is exactly
/// the confusion #6 is about.
fn frames(text: &str) -> Vec<&str> {
    const HOME: &str = "\u{1b}[1;1H";
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(HOME) {
        let body = &rest[at + HOME.len()..];
        let end = body.find(HOME).unwrap_or(body.len());
        found.push(&body[..end]);
        rest = &body[end..];
    }
    found
}

/// The terminal rows a passage of output positioned the cursor to.
///
/// The map paints a frame by addressing each row in turn — `ESC [ <row> ; 1 H` — so the set of
/// rows a frame touched is its height, read off the wire. That is the one observation a resize
/// produces and nothing else can: a repaint at the old size addresses the old rows.
fn rows_addressed(text: &str) -> BTreeSet<usize> {
    let mut found = BTreeSet::new();
    let mut rest = text;
    while let Some(at) = rest.find('\u{1b}') {
        rest = &rest[at + 1..];
        let Some(body) = rest.strip_prefix('[') else {
            continue;
        };
        let Some(end) = body.find('H') else {
            continue;
        };
        if let Some((row, _)) = body[..end].split_once(';')
            && let Ok(row) = row.parse::<usize>()
        {
            found.insert(row);
        }
    }
    found
}
