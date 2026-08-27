//! Bindings are data: the default map, and a user binding that displaces a default.

mod support;

use ono_editor::{EditAction, Editor, KeyCode, KeyPress, Keymap, Modifiers, Outcome};
use support::type_text;

#[test]
fn should_bind_the_emacs_defaults_when_no_keymap_is_configured() {
    let keymap = Keymap::emacs();
    assert_eq!(
        keymap.lookup(KeyPress::ctrl('a')),
        Some(EditAction::MoveLineStart)
    );
    assert_eq!(
        keymap.lookup(KeyPress::key(KeyCode::Home)),
        Some(EditAction::MoveLineStart)
    );
    assert_eq!(
        keymap.lookup(KeyPress::alt('b')),
        Some(EditAction::MoveWordLeft)
    );
    assert_eq!(
        keymap.lookup(KeyPress::key(KeyCode::Enter)),
        Some(EditAction::Accept)
    );
    assert_eq!(
        keymap.lookup(KeyPress::new(KeyCode::Enter, Modifiers::ALT)),
        Some(EditAction::InsertNewline)
    );
}

#[test]
fn should_leave_a_printable_character_unbound_so_it_inserts_itself() {
    assert_eq!(Keymap::emacs().lookup(KeyPress::char('a')), None);
}

#[test]
fn should_treat_a_control_combination_as_the_same_binding_whatever_its_case() {
    let keymap = Keymap::emacs();
    assert_eq!(
        keymap.lookup(KeyPress::ctrl('A')),
        keymap.lookup(KeyPress::ctrl('a')),
        "Ctrl-A and Ctrl-a are one key to a user"
    );
}

#[test]
fn should_use_the_user_binding_when_it_displaces_a_default() {
    let mut keymap = Keymap::emacs();
    keymap.bind(KeyPress::ctrl('t'), EditAction::MoveLineStart);
    let mut editor = Editor::new().with_keymap(keymap);
    type_text(&mut editor, "ab");

    assert_eq!(editor.feed(KeyPress::ctrl('t')), Outcome::Continue);
    assert_eq!(editor.line(), "ab", "the default transpose no longer runs");
    assert_eq!(editor.cursor(), 0, "the user binding ran instead");
}

#[test]
fn should_ignore_a_key_that_is_bound_to_nothing_and_prints_nothing() {
    let mut editor = Editor::new();
    type_text(&mut editor, "ls");
    assert_eq!(
        editor.feed(KeyPress::key(KeyCode::PageDown)),
        Outcome::Continue
    );
    assert_eq!(editor.line(), "ls");
}
