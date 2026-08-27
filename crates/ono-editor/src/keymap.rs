//! What a key does, and the table that decides it.
//!
//! Bindings are data — a list of `(key, action)` pairs — so a configuration file can replace or
//! extend them without the editor growing a second dispatch path. A user binding displaces the
//! default for the same key rather than sitting behind it.

use crate::key::{KeyCode, KeyPress, Modifiers};

/// One editing command.
///
/// Actions describe an intent ("kill the word before the cursor"), never a key. Two keys may
/// name the same action, and a configuration may point any key at any action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditAction {
    /// Insert a literal character at the cursor.
    Insert(char),
    /// One character left.
    MoveCharLeft,
    /// One character right.
    MoveCharRight,
    /// To the start of the word before the cursor.
    MoveWordLeft,
    /// To the end of the word after the cursor.
    MoveWordRight,
    /// To the start of the current line.
    MoveLineStart,
    /// To the end of the current line.
    MoveLineEnd,
    /// Delete the character before the cursor.
    DeleteCharBackward,
    /// Delete the character under the cursor.
    DeleteCharForward,
    /// Delete forward, or end the input when the line is empty.
    DeleteCharForwardOrEndOfInput,
    /// Kill from the cursor to the end of the line.
    KillLineForward,
    /// Kill from the start of the line to the cursor.
    KillLineBackward,
    /// Kill the whitespace-delimited word before the cursor.
    KillWordBackward,
    /// Kill the word after the cursor.
    KillWordForward,
    /// Insert the most recent kill.
    Yank,
    /// Replace the text just yanked with the kill before it.
    YankPop,
    /// Swap the two characters around the cursor.
    TransposeChars,
    /// Upper-case the word after the cursor.
    UppercaseWord,
    /// Lower-case the word after the cursor.
    LowercaseWord,
    /// Capitalise the word after the cursor.
    CapitaliseWord,
    /// Repaint the screen from scratch.
    ClearScreen,
    /// Abandon the line being typed, without ending the shell.
    CancelLine,
    /// Submit the line, or continue it when the statement is still open.
    Accept,
    /// Insert a line break whatever the statement's state.
    InsertNewline,
    /// The previous history entry, anchored on what has been typed.
    HistoryPrevious,
    /// The next history entry.
    HistoryNext,
    /// Start, or continue, an incremental search backwards through the history.
    ReverseSearch,
    /// Complete the word before the cursor.
    Complete,
}

impl EditAction {
    /// Whether performing this action can change the text of the line.
    ///
    /// The editor uses this to decide when the highlight is stale, so a movement key costs no
    /// parse at all (spec §34).
    pub(crate) const fn changes_text(self) -> bool {
        !matches!(
            self,
            EditAction::MoveCharLeft
                | EditAction::MoveCharRight
                | EditAction::MoveWordLeft
                | EditAction::MoveWordRight
                | EditAction::MoveLineStart
                | EditAction::MoveLineEnd
                | EditAction::ClearScreen
                | EditAction::ReverseSearch
        )
    }
}

/// The table mapping key presses to actions.
#[derive(Debug, Clone)]
pub struct Keymap {
    bindings: Vec<(KeyPress, EditAction)>,
}

impl Keymap {
    /// A keymap with no bindings at all, for a configuration that starts from nothing.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }

    /// The default Emacs-style keymap.
    ///
    /// The bindings are the ones a shell user's fingers already know. Where a terminal reports
    /// one key in two ways — `Ctrl-H` and Backspace, `Ctrl-I` and Tab, `Ctrl-M` and Return —
    /// both spellings are bound to the same action.
    #[must_use]
    pub fn emacs() -> Self {
        let mut keymap = Self::empty();
        let alt_enter = KeyPress::new(KeyCode::Enter, Modifiers::ALT);
        for (key, action) in [
            (KeyPress::ctrl('a'), EditAction::MoveLineStart),
            (KeyPress::key(KeyCode::Home), EditAction::MoveLineStart),
            (KeyPress::ctrl('e'), EditAction::MoveLineEnd),
            (KeyPress::key(KeyCode::End), EditAction::MoveLineEnd),
            (KeyPress::ctrl('b'), EditAction::MoveCharLeft),
            (KeyPress::key(KeyCode::Left), EditAction::MoveCharLeft),
            (KeyPress::ctrl('f'), EditAction::MoveCharRight),
            (KeyPress::key(KeyCode::Right), EditAction::MoveCharRight),
            (KeyPress::alt('b'), EditAction::MoveWordLeft),
            (KeyPress::alt('f'), EditAction::MoveWordRight),
            (KeyPress::ctrl('h'), EditAction::DeleteCharBackward),
            (
                KeyPress::key(KeyCode::Backspace),
                EditAction::DeleteCharBackward,
            ),
            (
                KeyPress::key(KeyCode::Delete),
                EditAction::DeleteCharForward,
            ),
            (
                KeyPress::ctrl('d'),
                EditAction::DeleteCharForwardOrEndOfInput,
            ),
            (KeyPress::ctrl('k'), EditAction::KillLineForward),
            (KeyPress::ctrl('u'), EditAction::KillLineBackward),
            (KeyPress::ctrl('w'), EditAction::KillWordBackward),
            (KeyPress::alt('d'), EditAction::KillWordForward),
            (KeyPress::ctrl('y'), EditAction::Yank),
            (KeyPress::alt('y'), EditAction::YankPop),
            (KeyPress::ctrl('t'), EditAction::TransposeChars),
            (KeyPress::alt('u'), EditAction::UppercaseWord),
            (KeyPress::alt('l'), EditAction::LowercaseWord),
            (KeyPress::alt('c'), EditAction::CapitaliseWord),
            (KeyPress::ctrl('l'), EditAction::ClearScreen),
            (KeyPress::ctrl('c'), EditAction::CancelLine),
            (KeyPress::ctrl('p'), EditAction::HistoryPrevious),
            (KeyPress::key(KeyCode::Up), EditAction::HistoryPrevious),
            (KeyPress::ctrl('n'), EditAction::HistoryNext),
            (KeyPress::key(KeyCode::Down), EditAction::HistoryNext),
            (KeyPress::ctrl('r'), EditAction::ReverseSearch),
            (KeyPress::key(KeyCode::Tab), EditAction::Complete),
            (KeyPress::ctrl('i'), EditAction::Complete),
            (KeyPress::key(KeyCode::Enter), EditAction::Accept),
            (KeyPress::ctrl('m'), EditAction::Accept),
            (KeyPress::ctrl('j'), EditAction::Accept),
            (alt_enter, EditAction::InsertNewline),
        ] {
            keymap.bind(key, action);
        }
        keymap
    }

    /// Points `key` at `action`, displacing whatever it was bound to before.
    pub fn bind(&mut self, key: KeyPress, action: EditAction) {
        match self
            .bindings
            .iter_mut()
            .find(|(candidate, _)| *candidate == key)
        {
            Some(binding) => binding.1 = action,
            None => self.bindings.push((key, action)),
        }
    }

    /// Removes any binding for `key`, so it falls back to self-insertion or to nothing.
    pub fn unbind(&mut self, key: KeyPress) {
        self.bindings.retain(|(candidate, _)| *candidate != key);
    }

    /// The action `key` is bound to, if any.
    #[must_use]
    pub fn lookup(&self, key: KeyPress) -> Option<EditAction> {
        self.bindings
            .iter()
            .find(|(candidate, _)| *candidate == key)
            .map(|(_, action)| *action)
    }

    /// Every binding, in the order they were added.
    #[must_use]
    pub fn bindings(&self) -> &[(KeyPress, EditAction)] {
        &self.bindings
    }
}

impl Default for Keymap {
    fn default() -> Self {
        Self::emacs()
    }
}
