//! Output that does not end in a newline, and the prompt drawn after it (issue #132; v0.6.1 §8,
//! §27).
//!
//! A frame of the line editor starts by clearing the row the cursor stands on, and a program that
//! printed `abc` without a newline leaves the cursor on the row that holds `abc`. What a person
//! sees is the terminal's screen, so these tests keep one: every byte the shell writes is replayed
//! through a minimal model of a VT100-style screen, and the assertions read its rows rather than
//! the byte stream.
//!
//! A pseudo-terminal has no terminal emulator behind it, so nothing answers the cursor-position
//! report (`ESC [ 6 n`) a shell may ask for. The model answers it, with the column the replay has
//! the cursor in at the point in the stream where the question was asked — which is what a real
//! terminal says. The answer is derived from the same bytes the assertions read rather than typed
//! into the test, so a shell that asked and then drew over the output anyway still fails, and so
//! does one that moved to a new line without being told it had to.

#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::time::{Duration, Instant};

use ono_process::{Command, Executor, PtySession, WindowSize};
use ono_testkit::{Scratch, scratch};

mod support;
use support::interactive_shell_in;

/// The rows the pseudo-terminal has, which is also the largest row a cursor report can name.
const ROWS: usize = 24;

/// A screen as far as these tests need one: rows of characters, a cursor, and the questions the
/// shell asked about where that cursor is.
///
/// It scrolls nowhere — rows only accumulate — and wraps nothing, which holds for these tests
/// because every line they draw is far narrower than the terminal.
#[derive(Default)]
struct Screen {
    rows: Vec<Vec<char>>,
    row: usize,
    column: usize,
    /// Where the cursor stood each time the shell asked for its position, in order.
    queries: Vec<(usize, usize)>,
}

impl Screen {
    /// The screen everything in `bytes` draws. An escape sequence cut off at the end waits for
    /// the next replay.
    fn replay(bytes: &[u8]) -> Self {
        let chars: Vec<char> = String::from_utf8_lossy(bytes).chars().collect();
        let mut screen = Self::default();
        let mut index = 0;
        while index < chars.len() {
            let character = chars[index];
            index += 1;
            match character {
                '\u{1b}' => match chars.get(index) {
                    Some('[') => {
                        let start = index + 1;
                        let Some(end) = (start..chars.len())
                            .find(|&at| ('\u{40}'..='\u{7e}').contains(&chars[at]))
                        else {
                            break;
                        };
                        let parameters: String = chars[start..end].iter().collect();
                        screen.control(&parameters, chars[end]);
                        index = end + 1;
                    }
                    // An operating-system command, such as a window title, runs to BEL or ST.
                    Some(']') => {
                        let Some(end) = (index..chars.len()).find(|&at| {
                            chars[at] == '\u{7}' || (chars[at] == '\\' && chars[at - 1] == '\u{1b}')
                        }) else {
                            break;
                        };
                        index = end + 1;
                    }
                    Some(_) => index += 1,
                    None => break,
                },
                '\r' => screen.column = 0,
                '\n' => screen.row += 1,
                '\u{8}' => screen.column = screen.column.saturating_sub(1),
                character if character.is_control() => {}
                character => screen.put(character),
            }
        }
        screen
    }

    /// One control sequence `ESC [ <parameters> <command>`.
    fn control(&mut self, parameters: &str, command: char) {
        let first = parameters
            .split(';')
            .next()
            .and_then(|number| number.parse::<usize>().ok());
        let count = first.filter(|number| *number > 0).unwrap_or(1);
        match command {
            'G' => self.column = count - 1,
            'A' => self.row = self.row.saturating_sub(count),
            'B' => self.row += count,
            'C' => self.column += count,
            'D' => self.column = self.column.saturating_sub(count),
            'E' => {
                self.row += count;
                self.column = 0;
            }
            'F' => {
                self.row = self.row.saturating_sub(count);
                self.column = 0;
            }
            'J' if matches!(first, Some(2 | 3)) => self.rows.clear(),
            'J' => {
                self.clear_to_end_of_row();
                self.rows.truncate(self.row + 1);
            }
            'K' if first == Some(2) => {
                if let Some(row) = self.rows.get_mut(self.row) {
                    row.clear();
                }
            }
            'K' => self.clear_to_end_of_row(),
            'n' if parameters == "6" => self.queries.push((self.row, self.column)),
            // Colours, cursor visibility and terminal modes draw nothing.
            _ => {}
        }
    }

    fn put(&mut self, character: char) {
        if self.rows.len() <= self.row {
            self.rows.resize(self.row + 1, Vec::new());
        }
        let row = &mut self.rows[self.row];
        if row.len() <= self.column {
            row.resize(self.column + 1, ' ');
        }
        row[self.column] = character;
        self.column += 1;
    }

    fn clear_to_end_of_row(&mut self) {
        if let Some(row) = self.rows.get_mut(self.row) {
            row.truncate(self.column);
        }
    }

    /// The rows as text, without trailing blanks and without the empty rows below the last one.
    fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .rows
            .iter()
            .map(|row| row.iter().collect::<String>().trim_end().to_owned())
            .collect();
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        lines
    }
}

/// Whether a row of the screen is a prompt, typed on or not: a local session's prompt names its
/// context first and ends in `>` (spec §4.2).
fn is_prompt(line: &str) -> bool {
    line.starts_with("local://") && (line.contains(" > ") || line.ends_with(" >"))
}

/// The screen from the first prompt on, each prompt written as `$` so the assertions read as a
/// session transcript rather than as this machine's paths.
fn session(screen: &Screen) -> Vec<String> {
    let lines = screen.lines();
    let first = lines
        .iter()
        .position(|line| is_prompt(line))
        .unwrap_or(lines.len());
    lines[first..]
        .iter()
        .map(|line| match line.split_once('>') {
            Some((_, typed)) if is_prompt(line) => format!("$ {}", typed.trim()).trim().to_owned(),
            _ => line.clone(),
        })
        .collect()
}

/// A shell on a pseudo-terminal, and everything it has written there.
struct Terminal {
    shell: PtySession,
    transcript: Vec<u8>,
    answered: usize,
    /// Whether the terminal answers a cursor-position report, as every terminal emulator does.
    answers: bool,
}

impl Terminal {
    fn new(shell: PtySession, answers: bool) -> Self {
        Self {
            shell,
            transcript: Vec::new(),
            answered: 0,
            answers,
        }
    }

    fn type_line(&mut self, line: &str) {
        self.type_keys(&format!("{line}\r"));
    }

    fn type_keys(&mut self, keys: &str) {
        self.shell.write_all(keys.as_bytes()).expect("input");
    }

    /// Reads until `done` holds for the screen, answering every cursor-position report on the
    /// way. The screen comes back either way, so a failed assertion shows what was on it.
    fn until(&mut self, done: impl Fn(&Screen) -> bool) -> Screen {
        let deadline = Instant::now() + ono_testkit::under_load(Duration::from_secs(20));
        let mut buffer = [0u8; 4096];
        loop {
            let screen = Screen::replay(&self.transcript);
            if self.answers {
                for &(row, column) in &screen.queries[self.answered..] {
                    // Rows are counted from the top of the visible screen; the model does not
                    // scroll, so the row is clamped to the last one. The column is exact.
                    let reply = format!("\u{1b}[{};{}R", row.min(ROWS - 1) + 1, column + 1);
                    self.shell.write_all(reply.as_bytes()).expect("answer");
                }
                self.answered = screen.queries.len();
            }
            if done(&screen) || Instant::now() >= deadline {
                return screen;
            }
            match self
                .shell
                .read_timeout(&mut buffer, Duration::from_millis(100))
            {
                Ok(Some(0)) | Err(_) => return screen,
                Ok(Some(count)) => self.transcript.extend_from_slice(&buffer[..count]),
                Ok(None) => {}
            }
        }
    }

    /// Reads until the screen shows `count` prompts, the last of them waiting for input.
    fn until_prompts(&mut self, count: usize) -> Screen {
        self.until(|screen| {
            let lines = screen.lines();
            lines.iter().filter(|line| is_prompt(line)).count() >= count
                && lines.last().is_some_and(|line| is_prompt(line))
        })
    }

    fn leave(mut self) {
        self.type_keys("\u{3}");
        self.type_line("exit");
        let _ = self.shell.wait();
    }
}

/// Starts `ono` interactively with `TERM` set to `term`.
fn shell_with_term(directory: &Scratch, term: &str) -> PtySession {
    let mut executor = Executor::detached();
    let command = Command::new(ono_testkit::ono_binary())
        .env("TERM", term)
        .env("NO_COLOR", "1")
        .env("HOME", directory.path().display().to_string())
        .current_dir(directory.path());
    executor
        .run_pty(&command, WindowSize::new(ROWS as u16, 100))
        .expect("a pseudo-terminal must be available")
}

#[test]
fn should_keep_output_without_a_trailing_newline_on_screen_above_the_next_prompt() {
    // Issue #132, v0.6.1 §8: `printf abc` — or `curl ifconfig.me` — printed its output and the
    // prompt erased it at once. Twice in a row, because the second prompt must not undo what the
    // first one got right.
    let directory = scratch();
    let mut terminal = Terminal::new(interactive_shell_in(&directory), true);
    terminal.until_prompts(1);

    terminal.type_line("printf abc");
    terminal.until_prompts(2);
    terminal.type_line("printf abc");
    let screen = terminal.until_prompts(3);

    let transcript = session(&screen);
    assert_eq!(
        transcript[transcript.len().saturating_sub(5)..],
        ["$ printf abc", "abc", "$ printf abc", "abc", "$"],
        "v0.6.1 §8: output without a trailing newline stays visible, and the prompt starts on the \
         line after it; the screen was:\n{}",
        screen.lines().join("\n")
    );
    terminal.leave();
}

#[test]
fn should_not_add_a_blank_line_after_output_that_ends_in_a_newline_or_after_no_output() {
    // v0.6.1 §8 and §27: the fix must not buy the case above with a blank line after every
    // command that did end its output properly — nor after a command that printed nothing.
    let directory = scratch();
    let mut terminal = Terminal::new(interactive_shell_in(&directory), true);
    terminal.until_prompts(1);

    terminal.type_line("echo abc");
    terminal.until_prompts(2);
    terminal.type_line("true");
    let screen = terminal.until_prompts(3);

    let transcript = session(&screen);
    assert_eq!(
        transcript[transcript.len().saturating_sub(4)..],
        ["$ echo abc", "abc", "$ true", "$"],
        "v0.6.1 §27: newline-terminated output and empty output gain no blank line before the \
         prompt; the screen was:\n{}",
        screen.lines().join("\n")
    );
    terminal.leave();
}

#[test]
fn should_keep_the_prompt_on_its_own_line_while_the_user_types_after_unterminated_output() {
    // Only the first frame of a prompt starts a fresh line. Every key redraws the prompt, and the
    // cursor is never in the first column while it does — a fresh line per frame would stack a
    // new copy of the prompt under the old one with every key typed.
    let directory = scratch();
    let mut terminal = Terminal::new(interactive_shell_in(&directory), true);
    terminal.until_prompts(1);

    terminal.type_line("printf abc");
    terminal.until_prompts(2);
    terminal.type_keys("ec");
    let screen = terminal.until(|screen| {
        screen
            .lines()
            .last()
            .is_some_and(|line| is_prompt(line) && line.ends_with("ec"))
    });

    let transcript = session(&screen);
    assert_eq!(
        transcript,
        ["$ printf abc", "abc", "$ ec"],
        "v0.6.1 §8: redrawing the prompt while typing keeps it on one line; the screen was:\n{}",
        screen.lines().join("\n")
    );
    terminal.leave();
}

#[test]
fn should_keep_prompting_when_the_terminal_never_reports_its_cursor() {
    // A terminal that does not answer the report costs a bounded wait and nothing else: the
    // prompt still comes, commands still run, and nothing is invented about where the cursor
    // was. This pseudo-terminal is such a terminal — nobody answers.
    let directory = scratch();
    let mut terminal = Terminal::new(interactive_shell_in(&directory), false);
    terminal.until_prompts(1);

    terminal.type_line("printf abc");
    terminal.until_prompts(2);
    terminal.type_line("echo still-here");
    let screen = terminal.until_prompts(3);

    let lines = screen.lines();
    assert!(
        lines.iter().any(|line| line == "still-here") && lines.last().is_some_and(|l| is_prompt(l)),
        "a terminal that never answers must not stop the shell from prompting and running; the \
         screen was:\n{}",
        lines.join("\n")
    );
    terminal.leave();
}

#[test]
fn should_not_ask_a_dumb_terminal_where_its_cursor_is() {
    // A dumb terminal answers no report; it prints the question. The shell does not ask it.
    let directory = scratch();
    let mut terminal = Terminal::new(shell_with_term(&directory, "dumb"), true);
    terminal.until_prompts(1);

    terminal.type_line("printf abc");
    let screen = terminal.until_prompts(2);

    assert!(
        screen.queries.is_empty(),
        "`TERM=dumb` cannot answer `ESC [ 6 n`, so the shell must not write it; the screen \
         was:\n{}",
        screen.lines().join("\n")
    );
    terminal.leave();
}
