//! The text being edited, the cursor within it, and the ring of killed text.
//!
//! The cursor is a byte offset that is always on a character boundary. Every operation is
//! defined in whole characters, so a multi-byte or a wide character can never be split — the
//! shell must be usable for a person whose filenames are not ASCII.
//!
//! Two notions of "word" coexist here, exactly as they do in every shell a user has used before:
//!
//! - a **word** is a run of alphanumeric characters and `_`, which is what the word movements
//!   and the case operations act on, so `a-b` is two words;
//! - a **big word** is a run of non-whitespace, which is what [`LineBuffer::kill_word_backward`]
//!   acts on, so `/usr/bin` is deleted in one stroke.

use std::ops::Range;

/// How many kills the ring remembers before the oldest is forgotten.
const KILL_RING_CAPACITY: usize = 64;

/// Which side of the cursor a kill took its text from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KillDirection {
    /// The text came from before the cursor.
    Backward,
    /// The text came from after the cursor.
    Forward,
}

/// The ring of text removed by the kill operations.
///
/// Consecutive kills join into one entry, in the order the text stood on the line, so
/// `Ctrl-W Ctrl-W Ctrl-Y` restores both words the way they were written.
#[derive(Debug, Clone, Default)]
pub struct KillRing {
    entries: Vec<String>,
    cursor: usize,
}

impl KillRing {
    /// An empty ring.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many kills the ring holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there is nothing to yank.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The text a yank would insert.
    #[must_use]
    pub fn current(&self) -> Option<&str> {
        self.entries.get(self.cursor).map(String::as_str)
    }

    fn add(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        self.entries.insert(0, text);
        self.entries.truncate(KILL_RING_CAPACITY);
        self.cursor = 0;
    }

    fn merge(&mut self, text: &str, direction: KillDirection) {
        if text.is_empty() {
            return;
        }
        match self.entries.first_mut() {
            Some(entry) => match direction {
                KillDirection::Backward => entry.insert_str(0, text),
                KillDirection::Forward => entry.push_str(text),
            },
            None => self.add(text.to_owned()),
        }
        self.cursor = 0;
    }

    fn rotate(&mut self) {
        if !self.entries.is_empty() {
            self.cursor = (self.cursor + 1) % self.entries.len();
        }
    }
}

/// The line being edited.
///
/// ```
/// use ono_editor::LineBuffer;
/// let mut buffer = LineBuffer::from_text("get process");
/// buffer.kill_word_backward();
/// assert_eq!(buffer.text(), "get ");
/// buffer.yank();
/// assert_eq!(buffer.text(), "get process");
/// ```
#[derive(Debug, Clone, Default)]
pub struct LineBuffer {
    text: String,
    cursor: usize,
    kill_ring: KillRing,
    last_kill: Option<KillDirection>,
    last_yank: Option<Range<usize>>,
}

impl LineBuffer {
    /// An empty buffer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A buffer holding `text`, with the cursor at its end.
    #[must_use]
    pub fn from_text(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.len();
        Self {
            text,
            cursor,
            ..Self::default()
        }
    }

    /// The text of the whole buffer, newlines and all.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The cursor, as a byte offset on a character boundary.
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Whether the buffer holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The ring of killed text.
    #[must_use]
    pub fn kill_ring(&self) -> &KillRing {
        &self.kill_ring
    }

    /// Moves the cursor to `offset`, snapped down to the nearest character boundary.
    ///
    /// An offset past the end lands at the end. Snapping rather than rejecting keeps a caller
    /// that computed an offset from a span from having to know how wide the characters are.
    pub fn set_cursor(&mut self, offset: usize) {
        self.forget_transient();
        self.cursor = floor_boundary(&self.text, offset);
    }

    /// Replaces the whole buffer and puts the cursor at the end.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.forget_transient();
        self.text = text.into();
        self.cursor = self.text.len();
    }

    /// Empties the buffer. The kill ring survives, as it does in every editor.
    pub fn clear(&mut self) {
        self.forget_transient();
        self.text.clear();
        self.cursor = 0;
    }

    /// Inserts one character at the cursor.
    pub fn insert_char(&mut self, character: char) {
        self.forget_transient();
        self.text.insert(self.cursor, character);
        self.cursor += character.len_utf8();
    }

    /// Inserts `text` at the cursor.
    pub fn insert_str(&mut self, text: &str) {
        self.forget_transient();
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    /// Replaces the bytes in `range` with `text`, carrying the cursor with the change.
    ///
    /// The range's ends are snapped to character boundaries, so a span computed by a completer
    /// can never split a character.
    pub fn replace_range(&mut self, range: Range<usize>, text: &str) {
        self.forget_transient();
        let start = floor_boundary(&self.text, range.start);
        let end = floor_boundary(&self.text, range.end).max(start);
        self.text.replace_range(start..end, text);
        self.cursor = if self.cursor <= start {
            self.cursor
        } else if self.cursor >= end {
            self.cursor - (end - start) + text.len()
        } else {
            start + text.len()
        };
    }

    /// Deletes the character before the cursor. Reports whether there was one.
    pub fn delete_backward_char(&mut self) -> bool {
        self.forget_transient();
        match previous_char(&self.text, self.cursor) {
            Some((start, _)) => {
                self.text.replace_range(start..self.cursor, "");
                self.cursor = start;
                true
            }
            None => false,
        }
    }

    /// Deletes the character under the cursor. Reports whether there was one.
    pub fn delete_forward_char(&mut self) -> bool {
        self.forget_transient();
        match next_char(&self.text, self.cursor) {
            Some((end, _)) => {
                self.text.replace_range(self.cursor..end, "");
                true
            }
            None => false,
        }
    }

    /// Moves one character left. Reports whether it could.
    pub fn move_left(&mut self) -> bool {
        self.forget_transient();
        match previous_char(&self.text, self.cursor) {
            Some((start, _)) => {
                self.cursor = start;
                true
            }
            None => false,
        }
    }

    /// Moves one character right. Reports whether it could.
    pub fn move_right(&mut self) -> bool {
        self.forget_transient();
        match next_char(&self.text, self.cursor) {
            Some((end, _)) => {
                self.cursor = end;
                true
            }
            None => false,
        }
    }

    /// Moves to the start of the word before the cursor.
    pub fn move_word_left(&mut self) {
        self.forget_transient();
        self.cursor = word_start_before(&self.text, self.cursor);
    }

    /// Moves to the end of the word after the cursor.
    pub fn move_word_right(&mut self) {
        self.forget_transient();
        self.cursor = word_end_after(&self.text, self.cursor);
    }

    /// Moves to the start of the line the cursor is on.
    pub fn move_line_start(&mut self) {
        self.forget_transient();
        self.cursor = line_start(&self.text, self.cursor);
    }

    /// Moves to the end of the line the cursor is on.
    pub fn move_line_end(&mut self) {
        self.forget_transient();
        self.cursor = line_end(&self.text, self.cursor);
    }

    /// Kills from the cursor to the end of the line, or the line break itself when the cursor
    /// already sits at the end of a line.
    pub fn kill_line_forward(&mut self) {
        let end = line_end(&self.text, self.cursor);
        let end = if end == self.cursor {
            next_char(&self.text, self.cursor).map_or(end, |(after, _)| after)
        } else {
            end
        };
        self.kill(self.cursor..end, KillDirection::Forward);
    }

    /// Kills from the start of the line to the cursor.
    pub fn kill_line_backward(&mut self) {
        let start = line_start(&self.text, self.cursor);
        self.kill(start..self.cursor, KillDirection::Backward);
    }

    /// Kills the whitespace-delimited word before the cursor.
    pub fn kill_word_backward(&mut self) {
        let start = big_word_start_before(&self.text, self.cursor);
        self.kill(start..self.cursor, KillDirection::Backward);
    }

    /// Kills the word after the cursor.
    pub fn kill_word_forward(&mut self) {
        let end = word_end_after(&self.text, self.cursor);
        self.kill(self.cursor..end, KillDirection::Forward);
    }

    /// Inserts the most recent kill at the cursor. Reports whether there was one.
    pub fn yank(&mut self) -> bool {
        let Some(text) = self.kill_ring.current().map(str::to_owned) else {
            return false;
        };
        let start = self.cursor;
        self.text.insert_str(start, &text);
        self.cursor = start + text.len();
        self.last_kill = None;
        self.last_yank = Some(start..self.cursor);
        true
    }

    /// Replaces the text just yanked with the kill before it.
    ///
    /// Reports whether it could: popping is only meaningful directly after a yank, because it
    /// replaces text the yank put there.
    pub fn yank_pop(&mut self) -> bool {
        let Some(range) = self.last_yank.clone() else {
            return false;
        };
        self.kill_ring.rotate();
        let Some(text) = self.kill_ring.current().map(str::to_owned) else {
            return false;
        };
        self.text.replace_range(range.clone(), &text);
        self.cursor = range.start + text.len();
        self.last_yank = Some(range.start..self.cursor);
        true
    }

    /// Swaps the two characters around the cursor.
    ///
    /// At the end of a line there is nothing to the right to swap with, so the two characters
    /// before the cursor trade places instead — which is what makes the binding useful for
    /// repairing a typo the moment it is noticed.
    pub fn transpose_chars(&mut self) {
        self.forget_transient();
        let at_line_end =
            self.cursor == self.text.len() || self.text[self.cursor..].starts_with('\n');
        if at_line_end {
            let Some((second_start, second)) = previous_char(&self.text, self.cursor) else {
                return;
            };
            let Some((first_start, first)) = previous_char(&self.text, second_start) else {
                return;
            };
            let swapped = format!("{second}{first}");
            self.text.replace_range(first_start..self.cursor, &swapped);
            self.cursor = first_start + swapped.len();
        } else {
            let Some((left_start, left)) = previous_char(&self.text, self.cursor) else {
                return;
            };
            let Some((right_end, right)) = next_char(&self.text, self.cursor) else {
                return;
            };
            let swapped = format!("{right}{left}");
            self.text.replace_range(left_start..right_end, &swapped);
            self.cursor = left_start + swapped.len();
        }
    }

    /// Upper-cases the word after the cursor and moves to its end.
    pub fn uppercase_word(&mut self) {
        self.map_word(|word| word.to_uppercase());
    }

    /// Lower-cases the word after the cursor and moves to its end.
    pub fn lowercase_word(&mut self) {
        self.map_word(|word| word.to_lowercase());
    }

    /// Capitalises the word after the cursor and moves to its end.
    pub fn capitalise_word(&mut self) {
        self.map_word(|word| {
            let mut result = String::with_capacity(word.len());
            let mut first = true;
            for character in word.chars() {
                if is_word_char(character) && first {
                    first = false;
                    result.extend(character.to_uppercase());
                } else {
                    result.extend(character.to_lowercase());
                }
            }
            result
        });
    }

    fn map_word(&mut self, transform: impl Fn(&str) -> String) {
        self.forget_transient();
        let end = word_end_after(&self.text, self.cursor);
        if end <= self.cursor {
            return;
        }
        let replacement = transform(&self.text[self.cursor..end]);
        self.text.replace_range(self.cursor..end, &replacement);
        self.cursor += replacement.len();
    }

    fn kill(&mut self, range: Range<usize>, direction: KillDirection) {
        self.last_yank = None;
        if range.start >= range.end {
            // A kill that removed nothing must not claim the previous entry, or the next kill
            // would merge into text it has nothing to do with.
            return;
        }
        let removed = self.text[range.clone()].to_owned();
        self.text.replace_range(range.clone(), "");
        if self.cursor > range.start {
            self.cursor = range.start;
        }
        if self.last_kill.is_some() {
            self.kill_ring.merge(&removed, direction);
        } else {
            self.kill_ring.add(removed);
        }
        self.last_kill = Some(direction);
    }

    /// Forgets the state that only survives until the next unrelated operation.
    fn forget_transient(&mut self) {
        self.last_kill = None;
        self.last_yank = None;
    }
}

/// Whether `character` counts as part of a word for movement and case operations.
fn is_word_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// The largest character boundary at or below `offset`.
fn floor_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

/// The character before `offset`, with the offset it starts at.
fn previous_char(text: &str, offset: usize) -> Option<(usize, char)> {
    let offset = floor_boundary(text, offset);
    let character = text[..offset].chars().next_back()?;
    Some((offset - character.len_utf8(), character))
}

/// The character at `offset`, with the offset just past it.
fn next_char(text: &str, offset: usize) -> Option<(usize, char)> {
    let offset = floor_boundary(text, offset);
    let character = text[offset..].chars().next()?;
    Some((offset + character.len_utf8(), character))
}

fn line_start(text: &str, offset: usize) -> usize {
    let offset = floor_boundary(text, offset);
    text[..offset].rfind('\n').map_or(0, |index| index + 1)
}

fn line_end(text: &str, offset: usize) -> usize {
    let offset = floor_boundary(text, offset);
    text[offset..]
        .find('\n')
        .map_or(text.len(), |index| offset + index)
}

fn word_start_before(text: &str, offset: usize) -> usize {
    let mut offset = floor_boundary(text, offset);
    while let Some((start, character)) = previous_char(text, offset) {
        if is_word_char(character) {
            break;
        }
        offset = start;
    }
    while let Some((start, character)) = previous_char(text, offset) {
        if !is_word_char(character) {
            break;
        }
        offset = start;
    }
    offset
}

fn word_end_after(text: &str, offset: usize) -> usize {
    let mut offset = floor_boundary(text, offset);
    while let Some((end, character)) = next_char(text, offset) {
        if is_word_char(character) {
            break;
        }
        offset = end;
    }
    while let Some((end, character)) = next_char(text, offset) {
        if !is_word_char(character) {
            break;
        }
        offset = end;
    }
    offset
}

fn big_word_start_before(text: &str, offset: usize) -> usize {
    let mut offset = floor_boundary(text, offset);
    while let Some((start, character)) = previous_char(text, offset) {
        if !character.is_whitespace() {
            break;
        }
        offset = start;
    }
    while let Some((start, character)) = previous_char(text, offset) {
        if character.is_whitespace() {
            break;
        }
        offset = start;
    }
    offset
}
