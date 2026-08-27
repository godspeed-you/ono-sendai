//! Key presses, described without reference to any terminal.
//!
//! The editor is a state machine fed with values of this module's types, which is what makes it
//! testable without a PTY. Translating a real terminal event into a [`KeyPress`] is the only job
//! of [`crate::terminal`].

/// A key, independent of the modifiers held with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    /// A character the terminal reported, already resolved for shift and the keyboard layout.
    Char(char),
    /// Return.
    Enter,
    /// Tab.
    Tab,
    /// Shift-Tab, which terminals report as a key of its own.
    BackTab,
    /// Backspace, which deletes to the left.
    Backspace,
    /// Delete, which deletes to the right.
    Delete,
    /// Insert.
    Insert,
    /// Cursor left.
    Left,
    /// Cursor right.
    Right,
    /// Cursor up.
    Up,
    /// Cursor down.
    Down,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Escape.
    Esc,
}

/// The modifier keys held while a key was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    ctrl: bool,
    alt: bool,
    shift: bool,
}

impl Modifiers {
    /// No modifier at all.
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
    };

    /// Control alone.
    pub const CTRL: Self = Self {
        ctrl: true,
        alt: false,
        shift: false,
    };

    /// Alt (meta) alone.
    pub const ALT: Self = Self {
        ctrl: false,
        alt: true,
        shift: false,
    };

    /// Shift alone.
    pub const SHIFT: Self = Self {
        ctrl: false,
        alt: false,
        shift: true,
    };

    /// Whether control was held.
    #[must_use]
    pub const fn has_ctrl(self) -> bool {
        self.ctrl
    }

    /// Whether alt was held.
    #[must_use]
    pub const fn has_alt(self) -> bool {
        self.alt
    }

    /// Whether shift was held.
    #[must_use]
    pub const fn has_shift(self) -> bool {
        self.shift
    }

    /// The same modifiers with control added.
    #[must_use]
    pub const fn with_ctrl(mut self) -> Self {
        self.ctrl = true;
        self
    }

    /// The same modifiers with alt added.
    #[must_use]
    pub const fn with_alt(mut self) -> Self {
        self.alt = true;
        self
    }

    /// The same modifiers with shift added.
    #[must_use]
    pub const fn with_shift(mut self) -> Self {
        self.shift = true;
        self
    }
}

/// One key press: what was pressed, and what was held with it.
///
/// Presses are normalised on construction so that a binding matches what a user believes they
/// pressed. Shift is dropped from a character key, because the terminal has already applied it
/// to the character itself, and a character combined with control or alt is folded to lower
/// case, so `Ctrl-A` and `Ctrl-a` are one binding rather than two.
///
/// ```
/// use ono_editor::{KeyPress, Modifiers};
/// assert_eq!(KeyPress::ctrl('A'), KeyPress::ctrl('a'));
/// assert!(KeyPress::alt('b').modifiers().has_alt());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyPress {
    code: KeyCode,
    modifiers: Modifiers,
}

impl KeyPress {
    /// A key press with the given modifiers, normalised.
    #[must_use]
    pub fn new(code: KeyCode, modifiers: Modifiers) -> Self {
        match code {
            KeyCode::Char(character) => {
                let folded = if modifiers.ctrl || modifiers.alt {
                    character.to_ascii_lowercase()
                } else {
                    character
                };
                Self {
                    code: KeyCode::Char(folded),
                    modifiers: Modifiers {
                        shift: false,
                        ..modifiers
                    },
                }
            }
            other => Self {
                code: other,
                modifiers,
            },
        }
    }

    /// A bare character, as self-insertion delivers it.
    #[must_use]
    pub fn char(character: char) -> Self {
        Self::new(KeyCode::Char(character), Modifiers::NONE)
    }

    /// A character with control held.
    #[must_use]
    pub fn ctrl(character: char) -> Self {
        Self::new(KeyCode::Char(character), Modifiers::CTRL)
    }

    /// A character with alt held.
    #[must_use]
    pub fn alt(character: char) -> Self {
        Self::new(KeyCode::Char(character), Modifiers::ALT)
    }

    /// A named key with no modifier.
    #[must_use]
    pub fn key(code: KeyCode) -> Self {
        Self::new(code, Modifiers::NONE)
    }

    /// The key that was pressed.
    #[must_use]
    pub const fn code(self) -> KeyCode {
        self.code
    }

    /// The modifiers held with it.
    #[must_use]
    pub const fn modifiers(self) -> Modifiers {
        self.modifiers
    }

    /// The character this press would insert, if it inserts anything.
    ///
    /// A character held together with control or alt is a command, not text, so it inserts
    /// nothing however printable it looks.
    #[must_use]
    pub fn insertable(self) -> Option<char> {
        match self.code {
            KeyCode::Char(character)
                if !self.modifiers.ctrl && !self.modifiers.alt && !character.is_control() =>
            {
                Some(character)
            }
            _ => None,
        }
    }
}
