//! The only part of the editor that knows a terminal exists.
//!
//! Everything above this module is decided by [`crate::Editor`]. This layer translates a
//! `crossterm` event into a [`KeyPress`], and paints a [`Frame`] where the previous one stood.
//! It holds no editing state and makes no editing decision, which is what keeps the editor
//! testable without a PTY.

use std::io::{self, Write};

use crossterm::cursor;
use crossterm::event::{Event, KeyEvent, KeyEventKind, KeyModifiers, read};
use crossterm::terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode, size};
use crossterm::{QueueableCommand, event};

use crate::frame::Frame;
use crate::key::{KeyCode, KeyPress, Modifiers};

/// Translates a terminal key event into a key press the editor understands.
///
/// Returns `None` for an event the editor has no use for — a key release, or a key the terminal
/// reports that has no editing meaning.
#[must_use]
pub fn key_press(key: KeyEvent) -> Option<KeyPress> {
    if matches!(key.kind, KeyEventKind::Release) {
        return None;
    }
    let code = match key.code {
        event::KeyCode::Char(character) => KeyCode::Char(character),
        event::KeyCode::Enter => KeyCode::Enter,
        event::KeyCode::Tab => KeyCode::Tab,
        event::KeyCode::BackTab => KeyCode::BackTab,
        event::KeyCode::Backspace => KeyCode::Backspace,
        event::KeyCode::Delete => KeyCode::Delete,
        event::KeyCode::Insert => KeyCode::Insert,
        event::KeyCode::Left => KeyCode::Left,
        event::KeyCode::Right => KeyCode::Right,
        event::KeyCode::Up => KeyCode::Up,
        event::KeyCode::Down => KeyCode::Down,
        event::KeyCode::Home => KeyCode::Home,
        event::KeyCode::End => KeyCode::End,
        event::KeyCode::PageUp => KeyCode::PageUp,
        event::KeyCode::PageDown => KeyCode::PageDown,
        event::KeyCode::Esc => KeyCode::Esc,
        _ => return None,
    };
    let mut modifiers = Modifiers::NONE;
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        modifiers = modifiers.with_ctrl();
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        modifiers = modifiers.with_alt();
    }
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        modifiers = modifiers.with_shift();
    }
    Some(KeyPress::new(code, modifiers))
}

/// Blocks until the terminal reports a key the editor can use.
///
/// # Errors
///
/// Fails when the terminal cannot be read.
pub fn read_key() -> io::Result<KeyPress> {
    loop {
        if let Event::Key(key) = read()?
            && let Some(press) = key_press(key)
        {
            return Ok(press);
        }
    }
}

/// The terminal's size in columns and rows.
///
/// # Errors
///
/// Fails when the size cannot be determined.
pub fn terminal_size() -> io::Result<(usize, usize)> {
    let (columns, rows) = size()?;
    Ok((columns as usize, rows as usize))
}

/// What the terminal reported: a key, or a new window size.
///
/// A line editor only ever needs the first; a full-screen view needs both, because §43.4 of the
/// v0.4 specification requires a resize to preserve the place and the focus, which is only
/// possible for a view that is told the size changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalEvent {
    /// A key the editor or a view can act on.
    Key(KeyPress),
    /// The terminal is now this many columns by this many rows.
    Resize(usize, usize),
}

/// Waits up to `patience` for the terminal to report something usable.
///
/// `Ok(None)` means the time ran out with nothing to report, which is how a live view gets to do
/// its own work between key presses without a thread of its own.
///
/// # Errors
///
/// Fails when the terminal cannot be read.
pub fn read_event_timeout(patience: std::time::Duration) -> io::Result<Option<TerminalEvent>> {
    let deadline = std::time::Instant::now() + patience;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if !event::poll(left)? {
            return Ok(None);
        }
        match read()? {
            Event::Key(key) => {
                if let Some(press) = key_press(key) {
                    return Ok(Some(TerminalEvent::Key(press)));
                }
            }
            Event::Resize(columns, rows) => {
                return Ok(Some(TerminalEvent::Resize(columns as usize, rows as usize)));
            }
            _ => {}
        }
        if std::time::Instant::now() >= deadline {
            return Ok(None);
        }
    }
}

/// The alternate screen buffer, left when this value is dropped.
///
/// A full-screen view borrows the terminal rather than scrolling the shell's own screen away:
/// entering the alternate buffer saves what was there, and leaving it puts it back, which is what
/// §49.8 and §52.2 of the v0.4 specification mean by a view that is entered deliberately and left
/// cleanly. The cursor is hidden while the view owns the screen and shown again afterwards.
#[derive(Debug)]
pub struct AlternateScreen {
    _private: (),
}

impl AlternateScreen {
    /// Enters the alternate screen buffer.
    ///
    /// The switch is *queued* rather than flushed, so the first [`paint`] after it leaves with
    /// it in one write. Nobody — a person or a test reading the terminal — ever sees an empty
    /// alternate screen between the two.
    ///
    /// # Errors
    ///
    /// Fails when the terminal cannot be written to.
    pub fn enter() -> io::Result<Self> {
        let mut out = io::stdout();
        out.queue(crossterm::terminal::EnterAlternateScreen)?;
        out.queue(cursor::Hide)?;
        Ok(Self { _private: () })
    }
}

impl Drop for AlternateScreen {
    fn drop(&mut self) {
        // Giving the screen back matters more than reporting that it could not be given back.
        let mut out = io::stdout();
        let _ = out.queue(cursor::Show);
        let _ = out.queue(crossterm::terminal::LeaveAlternateScreen);
        let _ = out.flush();
    }
}

/// Paints `lines` over the whole alternate screen, one per row.
///
/// # Errors
///
/// Fails when the terminal cannot be written to.
pub fn paint(lines: &[String]) -> io::Result<()> {
    let mut out = io::stdout();
    out.queue(cursor::MoveTo(0, 0))?;
    for (row, line) in lines.iter().enumerate() {
        out.queue(cursor::MoveTo(0, u16::try_from(row).unwrap_or(u16::MAX)))?;
        out.queue(Clear(ClearType::CurrentLine))?;
        out.write_all(line.as_bytes())?;
    }
    out.queue(Clear(ClearType::FromCursorDown))?;
    out.flush()
}

/// Raw mode, released when this value is dropped.
///
/// A line editor must see every key press itself, so the terminal's own line discipline is
/// switched off while the prompt is up — and switched back on however the shell leaves.
#[derive(Debug)]
pub struct RawMode {
    _private: (),
}

impl RawMode {
    /// Puts the terminal into raw mode.
    ///
    /// # Errors
    ///
    /// Fails when the terminal cannot be reconfigured.
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        Ok(Self { _private: () })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        // Leaving the terminal cooked matters more than reporting that it could not be done:
        // there is nowhere left to report it to.
        let _ = disable_raw_mode();
    }
}

/// Paints frames where the previous frame stood.
///
/// ```
/// use ono_editor::{Editor, Renderer};
/// use ono_render::{Presentation, Theme};
/// let editor = Editor::new().with_prompt("ono> ");
/// let frame = editor.frame(80, Presentation::Plain, &Theme::default());
/// let mut renderer = Renderer::new(Vec::new());
/// renderer.draw(&frame).expect("a vector never fails to accept bytes");
/// assert!(String::from_utf8_lossy(renderer.output()).contains("ono> "));
/// ```
#[derive(Debug)]
pub struct Renderer<W: Write> {
    out: W,
    rows_above_cursor: usize,
}

impl<W: Write> Renderer<W> {
    /// A renderer writing to `out`.
    pub const fn new(out: W) -> Self {
        Self {
            out,
            rows_above_cursor: 0,
        }
    }

    /// What has been written so far, for a caller that is inspecting rather than displaying.
    pub const fn output(&self) -> &W {
        &self.out
    }

    /// Draws `frame` over the frame drawn before it and leaves the terminal cursor where the
    /// frame says it belongs.
    ///
    /// # Errors
    ///
    /// Fails when the terminal cannot be written to.
    pub fn draw(&mut self, frame: &Frame) -> io::Result<()> {
        if self.rows_above_cursor > 0 {
            self.out.queue(cursor::MoveToPreviousLine(row_count(
                self.rows_above_cursor,
            )))?;
        }
        self.out.queue(cursor::MoveToColumn(0))?;
        self.out.queue(Clear(ClearType::FromCursorDown))?;
        for (index, line) in frame.lines.iter().enumerate() {
            if index > 0 {
                self.out.write_all(b"\r\n")?;
            }
            self.out.write_all(line.as_bytes())?;
        }
        let last = frame.lines.len().saturating_sub(1);
        let up = last.saturating_sub(frame.cursor_row);
        if up > 0 {
            self.out.queue(cursor::MoveToPreviousLine(row_count(up)))?;
        }
        self.out
            .queue(cursor::MoveToColumn(row_count(frame.cursor_column)))?;
        self.out.flush()?;
        self.rows_above_cursor = frame.cursor_row;
        Ok(())
    }

    /// Moves below the frame so the shell's own output starts on a fresh line.
    ///
    /// # Errors
    ///
    /// Fails when the terminal cannot be written to.
    pub fn finish(&mut self, frame: &Frame) -> io::Result<()> {
        let below = frame.lines.len().saturating_sub(frame.cursor_row + 1);
        for _ in 0..below {
            self.out.write_all(b"\r\n")?;
        }
        self.out.write_all(b"\r\n")?;
        self.out.flush()?;
        self.rows_above_cursor = 0;
        Ok(())
    }
}

/// Clamps a row or column count to what a terminal command can carry.
fn row_count(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}
