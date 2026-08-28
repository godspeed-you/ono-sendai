//! The editor itself: a state machine fed with key presses.
//!
//! Nothing here touches a terminal. Every behaviour — editing, history, completion, the frame
//! that would be drawn — is reachable by feeding [`KeyPress`] values and reading the result,
//! which is what makes the editor's contract testable and what keeps spec §34's latency budget
//! measurable without a PTY.

use ono_core::Span;
use ono_render::{Presentation, Theme, Token};

use crate::buffer::LineBuffer;
use crate::complete::{Completer, Completion, NoCompleter};
use crate::frame::{Frame, RowDraft, candidate_lines, display_char};
use crate::highlight::{Highlighter, PlainHighlighter};
use crate::key::KeyPress;
use crate::keymap::{EditAction, Keymap};
use crate::prompt::Prompt;

/// What the caller must do after a key press.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Keep editing. Redraw the frame where it stands.
    Continue,
    /// The user submitted this line. It is no longer in the buffer.
    Submit(String),
    /// The user abandoned an empty line. Start a fresh prompt; the shell keeps running.
    Cancelled,
    /// End of input on an empty line: the session is over.
    EndOfInput,
    /// Clear the screen, then draw the frame at the top of it.
    Redraw,
}

/// Where a history walk started and how far it has gone.
struct HistoryNav {
    anchor: String,
    saved: String,
    index: Option<usize>,
}

/// The state of an incremental search backwards through the history.
struct SearchState {
    query: String,
    index: Option<usize>,
    origin: String,
    origin_cursor: usize,
    found: bool,
}

impl SearchState {
    fn prompt_text(&self) -> String {
        if self.found {
            format!("(reverse-i-search)`{}': ", self.query)
        } else {
            format!("(failed reverse-i-search)`{}': ", self.query)
        }
    }
}

/// The interactive line editor.
///
/// ```
/// use ono_editor::{Editor, KeyCode, KeyPress, Outcome};
/// let mut editor = Editor::new();
/// for character in "get process".chars() {
///     editor.feed(KeyPress::char(character));
/// }
/// assert_eq!(
///     editor.feed(KeyPress::key(KeyCode::Enter)),
///     Outcome::Submit("get process".to_owned())
/// );
/// ```
pub struct Editor {
    buffer: LineBuffer,
    keymap: Keymap,
    highlighter: Box<dyn Highlighter>,
    completer: Box<dyn Completer>,
    history: Vec<String>,
    prompt: Prompt,
    continuation: Prompt,
    highlight: Vec<(Span, Token)>,
    navigation: Option<HistoryNav>,
    search: Option<SearchState>,
    listing: Vec<String>,
    completion_offered: bool,
}

impl Editor {
    /// An editor with the default keymap, no highlighting and no completion.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buffer: LineBuffer::new(),
            keymap: Keymap::emacs(),
            highlighter: Box::new(PlainHighlighter),
            completer: Box::new(NoCompleter),
            history: Vec::new(),
            prompt: Prompt::default(),
            continuation: Prompt::plain(".. "),
            highlight: Vec::new(),
            navigation: None,
            search: None,
            listing: Vec::new(),
            completion_offered: false,
        }
    }

    /// Uses `highlighter` for the colours of the line being typed.
    #[must_use]
    pub fn with_highlighter(mut self, highlighter: impl Highlighter + 'static) -> Self {
        self.highlighter = Box::new(highlighter);
        self.refresh_highlight();
        self
    }

    /// Uses `completer` to answer Tab.
    #[must_use]
    pub fn with_completer(mut self, completer: impl Completer + 'static) -> Self {
        self.completer = Box::new(completer);
        self
    }

    /// Uses `keymap` instead of the default bindings.
    #[must_use]
    pub fn with_keymap(mut self, keymap: Keymap) -> Self {
        self.keymap = keymap;
        self
    }

    /// Shows `prompt` before the first line.
    #[must_use]
    pub fn with_prompt(mut self, prompt: impl Into<Prompt>) -> Self {
        self.prompt = prompt.into();
        self
    }

    /// The bindings in force.
    #[must_use]
    pub fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// The bindings in force, for a configuration to change.
    pub fn keymap_mut(&mut self) -> &mut Keymap {
        &mut self.keymap
    }

    /// Sets the prompt shown before the first line.
    pub fn set_prompt(&mut self, prompt: impl Into<Prompt>) {
        self.prompt = prompt.into();
    }

    /// Sets the prompt shown before every line after the first.
    pub fn set_continuation_prompt(&mut self, prompt: impl Into<Prompt>) {
        self.continuation = prompt.into();
    }

    /// Replaces the history the editor recalls, oldest entry first.
    ///
    /// The editor holds the entries and nothing else: keeping, trimming and persisting them
    /// belongs to `ono-history` (spec §20).
    pub fn set_history(&mut self, entries: Vec<String>) {
        self.history = entries;
        self.navigation = None;
        self.search = None;
    }

    /// Appends one entry to the history the editor recalls.
    pub fn push_history(&mut self, entry: impl Into<String>) {
        self.history.push(entry.into());
        self.navigation = None;
    }

    /// The text being edited, newlines and all.
    #[must_use]
    pub fn line(&self) -> &str {
        self.buffer.text()
    }

    /// The cursor, as a byte offset into [`Editor::line`].
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.buffer.cursor()
    }

    /// Replaces the line being edited, putting the cursor at its end.
    pub fn set_line(&mut self, text: impl Into<String>) {
        self.buffer.set_text(text);
        self.navigation = None;
        self.search = None;
        self.listing.clear();
        self.completion_offered = false;
        self.refresh_highlight();
    }

    /// Starts a fresh, empty line.
    pub fn reset(&mut self) {
        self.set_line(String::new());
    }

    /// Feeds one key press to the editor and reports what the caller must do.
    pub fn feed(&mut self, key: KeyPress) -> Outcome {
        if self.search.is_some() {
            return self.feed_search(key);
        }
        let action = self
            .keymap
            .lookup(key)
            .or_else(|| key.insertable().map(EditAction::Insert));
        match action {
            Some(action) => self.apply(action),
            None => Outcome::Continue,
        }
    }

    /// Renders the editor's state for a terminal `width` display cells across.
    ///
    /// The lines are soft-wrapped at `width` and the cursor is reported in display cells, so a
    /// caller places the real cursor without knowing anything about the characters before it.
    /// A destination that takes no colour receives a frame with no escape sequence at all,
    /// which is the non-TTY determinism spec §4.6 requires.
    #[must_use]
    pub fn frame(&self, width: usize, presentation: Presentation, theme: &Theme) -> Frame {
        let width = width.max(1);
        let search_prompt = self
            .search
            .as_ref()
            .map(|search| Prompt::plain(search.prompt_text()));
        let prompt = search_prompt.as_ref().unwrap_or(&self.prompt);
        let prompt_rendered = prompt.render(theme, presentation);
        let prompt_width = prompt.width();
        let continuation_rendered = self.continuation.render(theme, presentation);
        let continuation_width = self.continuation.width();

        let cursor = self.buffer.cursor();
        let mut rows = vec![RowDraft::new(prompt_rendered)];
        let mut column = prompt_width;
        let mut cursor_row = 0;
        let mut cursor_column = prompt_width;
        let mut cursor_placed = false;
        let mut span_index = 0;
        let mut scratch = String::new();

        for (offset, character) in self.buffer.text().char_indices() {
            while span_index < self.highlight.len()
                && (self.highlight[span_index].0.end() as usize) <= offset
            {
                span_index += 1;
            }
            let token = self
                .highlight
                .get(span_index)
                .filter(|(span, _)| (span.start() as usize) <= offset)
                .map_or(Token::Foreground, |(_, token)| *token);

            if character == '\n' {
                if offset == cursor {
                    cursor_row = rows.len() - 1;
                    cursor_column = column;
                    cursor_placed = true;
                }
                rows.push(RowDraft::new(continuation_rendered.clone()));
                column = continuation_width;
                continue;
            }

            let (display, cells) = display_char(character, &mut scratch);
            if column + cells > width && column > 0 {
                rows.push(RowDraft::new(String::new()));
                column = 0;
            }
            if offset == cursor {
                cursor_row = rows.len() - 1;
                cursor_column = column;
                cursor_placed = true;
            }
            if let Some(row) = rows.last_mut() {
                row.push(display, token);
            }
            column += cells;
        }
        if !cursor_placed {
            cursor_row = rows.len() - 1;
            cursor_column = column;
        }

        let mut lines: Vec<String> = rows
            .into_iter()
            .map(|row| row.finish(theme, presentation))
            .collect();
        for line in candidate_lines(&self.listing, width) {
            lines.push(theme.paint(&line, Token::Foreground, presentation));
        }
        Frame {
            lines,
            cursor_row,
            cursor_column,
        }
    }

    fn apply(&mut self, action: EditAction) -> Outcome {
        if action != EditAction::Complete {
            self.listing.clear();
            self.completion_offered = false;
        }
        if action.changes_text()
            && !matches!(
                action,
                EditAction::HistoryPrevious | EditAction::HistoryNext
            )
        {
            self.navigation = None;
        }

        let outcome = match action {
            EditAction::Insert(character) => {
                self.buffer.insert_char(character);
                Outcome::Continue
            }
            EditAction::MoveCharLeft => {
                self.buffer.move_left();
                Outcome::Continue
            }
            EditAction::MoveCharRight => {
                self.buffer.move_right();
                Outcome::Continue
            }
            EditAction::MoveWordLeft => {
                self.buffer.move_word_left();
                Outcome::Continue
            }
            EditAction::MoveWordRight => {
                self.buffer.move_word_right();
                Outcome::Continue
            }
            EditAction::MoveLineStart => {
                self.buffer.move_line_start();
                Outcome::Continue
            }
            EditAction::MoveLineEnd => {
                self.buffer.move_line_end();
                Outcome::Continue
            }
            EditAction::DeleteCharBackward => {
                self.buffer.delete_backward_char();
                Outcome::Continue
            }
            EditAction::DeleteCharForward => {
                self.buffer.delete_forward_char();
                Outcome::Continue
            }
            EditAction::DeleteCharForwardOrEndOfInput => {
                if self.buffer.is_empty() {
                    return Outcome::EndOfInput;
                }
                self.buffer.delete_forward_char();
                Outcome::Continue
            }
            EditAction::KillLineForward => {
                self.buffer.kill_line_forward();
                Outcome::Continue
            }
            EditAction::KillLineBackward => {
                self.buffer.kill_line_backward();
                Outcome::Continue
            }
            EditAction::KillWordBackward => {
                self.buffer.kill_word_backward();
                Outcome::Continue
            }
            EditAction::KillWordForward => {
                self.buffer.kill_word_forward();
                Outcome::Continue
            }
            EditAction::Yank => {
                self.buffer.yank();
                Outcome::Continue
            }
            EditAction::YankPop => {
                self.buffer.yank_pop();
                Outcome::Continue
            }
            EditAction::TransposeChars => {
                self.buffer.transpose_chars();
                Outcome::Continue
            }
            EditAction::UppercaseWord => {
                self.buffer.uppercase_word();
                Outcome::Continue
            }
            EditAction::LowercaseWord => {
                self.buffer.lowercase_word();
                Outcome::Continue
            }
            EditAction::CapitaliseWord => {
                self.buffer.capitalise_word();
                Outcome::Continue
            }
            EditAction::ClearScreen => Outcome::Redraw,
            EditAction::CancelLine => {
                if self.buffer.is_empty() {
                    return Outcome::Cancelled;
                }
                self.buffer.clear();
                Outcome::Continue
            }
            EditAction::Accept => {
                if self.highlighter.is_complete(self.buffer.text()) {
                    let submitted = self.buffer.text().to_owned();
                    self.buffer.clear();
                    Outcome::Submit(submitted)
                } else {
                    self.buffer.insert_char('\n');
                    Outcome::Continue
                }
            }
            EditAction::InsertNewline => {
                self.buffer.insert_char('\n');
                Outcome::Continue
            }
            EditAction::HistoryPrevious => {
                self.history_previous();
                Outcome::Continue
            }
            EditAction::HistoryNext => {
                self.history_next();
                Outcome::Continue
            }
            EditAction::ReverseSearch => {
                self.navigation = None;
                self.start_search();
                Outcome::Continue
            }
            EditAction::Complete => self.complete(),
        };

        if action.changes_text() {
            self.refresh_highlight();
        }
        outcome
    }

    fn complete(&mut self) -> Outcome {
        let completion: Completion = self
            .completer
            .complete(self.buffer.text(), self.buffer.cursor());
        if completion.is_empty() {
            self.listing.clear();
            self.completion_offered = false;
            return Outcome::Continue;
        }

        let start = completion.span.start() as usize;
        let end = (completion.span.end() as usize).max(start);

        if let [only] = completion.candidates.as_slice() {
            let candidate = only.clone();
            self.buffer.replace_range(start..end, &candidate);
            self.listing.clear();
            self.completion_offered = false;
            return Outcome::Continue;
        }

        // A discovery listing is shown as soon as it is offered: it is what the user asked for,
        // not a hint that more typing would resolve. The word is still extended as far as the
        // candidates agree, so Tab does both jobs at once.
        if !completion.listing.is_empty() {
            let prefix = completion.common_prefix().to_owned();
            if prefix.len() > end - start {
                self.buffer.replace_range(start..end, &prefix);
            }
            self.listing = completion.listing;
            self.completion_offered = true;
            return Outcome::Continue;
        }

        if self.completion_offered {
            self.listing = completion.candidates;
            return Outcome::Continue;
        }

        let prefix = completion.common_prefix().to_owned();
        if prefix.len() > end - start {
            self.buffer.replace_range(start..end, &prefix);
        }
        self.listing.clear();
        self.completion_offered = true;
        Outcome::Continue
    }

    fn history_previous(&mut self) {
        let (anchor, saved, index) = match self.navigation.take() {
            Some(navigation) => (navigation.anchor, navigation.saved, navigation.index),
            None => {
                let text = self.buffer.text();
                let anchor = text.get(..self.buffer.cursor()).unwrap_or("").to_owned();
                (anchor, text.to_owned(), None)
            }
        };
        let end = index.unwrap_or(self.history.len());
        let found = self
            .history
            .get(..end)
            .unwrap_or(&[])
            .iter()
            .rposition(|entry| entry.starts_with(&anchor));
        let index = match found {
            Some(position) => {
                if let Some(entry) = self.history.get(position) {
                    let entry = entry.clone();
                    self.buffer.set_text(entry);
                }
                Some(position)
            }
            None => index,
        };
        self.navigation = Some(HistoryNav {
            anchor,
            saved,
            index,
        });
    }

    fn history_next(&mut self) {
        let Some(navigation) = self.navigation.take() else {
            return;
        };
        let HistoryNav {
            anchor,
            saved,
            index,
        } = navigation;
        let Some(current) = index else {
            self.navigation = Some(HistoryNav {
                anchor,
                saved,
                index,
            });
            return;
        };
        let found = self
            .history
            .iter()
            .enumerate()
            .skip(current + 1)
            .find(|(_, entry)| entry.starts_with(&anchor))
            .map(|(position, _)| position);
        match found {
            Some(position) => {
                if let Some(entry) = self.history.get(position) {
                    let entry = entry.clone();
                    self.buffer.set_text(entry);
                }
                self.navigation = Some(HistoryNav {
                    anchor,
                    saved,
                    index: Some(position),
                });
            }
            None => {
                self.buffer.set_text(saved);
                self.navigation = None;
            }
        }
    }

    fn start_search(&mut self) {
        match self.search.as_ref().map(|search| search.index) {
            Some(index) => {
                let end = index.unwrap_or(self.history.len());
                self.search_within(end);
            }
            None => {
                self.search = Some(SearchState {
                    query: String::new(),
                    index: None,
                    origin: self.buffer.text().to_owned(),
                    origin_cursor: self.buffer.cursor(),
                    found: true,
                });
            }
        }
    }

    fn feed_search(&mut self, key: KeyPress) -> Outcome {
        match self.keymap.lookup(key) {
            Some(EditAction::ReverseSearch) => {
                self.start_search();
                Outcome::Continue
            }
            Some(EditAction::CancelLine) => {
                self.abandon_search();
                Outcome::Continue
            }
            Some(EditAction::DeleteCharBackward) => {
                self.search_backspace();
                Outcome::Continue
            }
            Some(action) => {
                self.search = None;
                self.apply(action)
            }
            None => match key.insertable() {
                Some(character) => {
                    self.search_push(character);
                    Outcome::Continue
                }
                None => {
                    self.search = None;
                    Outcome::Continue
                }
            },
        }
    }

    fn search_push(&mut self, character: char) {
        let end = match self.search.as_mut() {
            Some(search) => {
                search.query.push(character);
                search.index.map_or(self.history.len(), |index| index + 1)
            }
            None => return,
        };
        self.search_within(end);
    }

    fn search_backspace(&mut self) {
        let empty = match self.search.as_mut() {
            Some(search) => {
                search.query.pop();
                search.query.is_empty()
            }
            None => return,
        };
        if empty {
            if let Some(search) = self.search.as_mut() {
                search.index = None;
                search.found = true;
                let origin = search.origin.clone();
                let origin_cursor = search.origin_cursor;
                self.buffer.set_text(origin);
                self.buffer.set_cursor(origin_cursor);
                self.refresh_highlight();
            }
            return;
        }
        self.search_within(self.history.len());
    }

    /// Searches `history[..end]` for the newest entry containing the query.
    fn search_within(&mut self, end: usize) {
        let Some(query) = self.search.as_ref().map(|search| search.query.clone()) else {
            return;
        };
        if query.is_empty() {
            return;
        }
        let found = self
            .history
            .get(..end.min(self.history.len()))
            .unwrap_or(&[])
            .iter()
            .rposition(|entry| entry.contains(&query));
        match found {
            Some(position) => {
                let entry = self.history.get(position).cloned().unwrap_or_default();
                let at = entry.find(&query).unwrap_or(0);
                self.buffer.set_text(entry);
                self.buffer.set_cursor(at);
                self.refresh_highlight();
                if let Some(search) = self.search.as_mut() {
                    search.index = Some(position);
                    search.found = true;
                }
            }
            None => {
                if let Some(search) = self.search.as_mut() {
                    search.found = false;
                }
            }
        }
    }

    fn abandon_search(&mut self) {
        if let Some(search) = self.search.take() {
            self.buffer.set_text(search.origin);
            self.buffer.set_cursor(search.origin_cursor);
            self.refresh_highlight();
        }
    }

    fn refresh_highlight(&mut self) {
        self.highlight = self.highlighter.highlight(self.buffer.text());
        self.highlight
            .sort_by_key(|(span, _)| (span.start(), span.end()));
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}
