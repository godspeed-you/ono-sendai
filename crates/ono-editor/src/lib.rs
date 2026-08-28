//! The Ono-Sendai line editor: the part of the shell a user touches every second.
//!
//! Spec §24.1 names the editor as one component — "line editing, keymap, syntax highlight" —
//! and spec §34 budgets it: a keystroke must reach the screen in under 8 ms, and the highlight
//! must be recomputed in under 5 ms. Those budgets are why the editor is a plain state machine
//! over [`KeyPress`] values, why it re-parses nothing when a key only moves the cursor, and why
//! nothing here blocks on a terminal.
//!
//! Two seams keep the crate independent of the rest of the shell:
//!
//! - [`Highlighter`] supplies the colours and answers whether a statement is finished. The
//!   shell implements it over the incremental parse of ADR-0009; the editor never depends on
//!   `ono-parser`, so the layering of ADR-0005 holds and every test here runs without a grammar.
//! - [`Completer`] supplies candidates for the word under the cursor, from the registries of
//!   spec §15.1. The editor knows how to ask, insert and lay out, and nothing else.
//!
//! ```
//! use ono_editor::{Editor, KeyCode, KeyPress, Outcome};
//! use ono_render::{Presentation, Theme};
//!
//! let mut editor = Editor::new().with_prompt("local://~ > ");
//! for character in "get process".chars() {
//!     editor.feed(KeyPress::char(character));
//! }
//!
//! let frame = editor.frame(80, Presentation::Plain, &Theme::default());
//! assert_eq!(frame.lines, vec!["local://~ > get process".to_owned()]);
//! assert_eq!(frame.cursor_column, 23);
//!
//! assert_eq!(
//!     editor.feed(KeyPress::key(KeyCode::Enter)),
//!     Outcome::Submit("get process".to_owned())
//! );
//! ```

#![forbid(unsafe_code)]

mod buffer;
mod complete;
mod editor;
mod frame;
mod highlight;
mod key;
mod keymap;
mod prompt;
mod terminal;

pub use buffer::{KillRing, LineBuffer};
pub use complete::{Completer, Completion, NoCompleter};
pub use editor::{Editor, Outcome};
pub use frame::Frame;
pub use highlight::{Highlighter, PlainHighlighter};
pub use key::{KeyCode, KeyPress, Modifiers};
pub use keymap::{EditAction, Keymap};
pub use prompt::Prompt;
pub use terminal::{
    AlternateScreen, RawMode, Renderer, TerminalEvent, key_press, paint, read_event_timeout,
    read_key, terminal_size,
};
