//! The thin layer between the terminal and the editor: it translates and it paints, nothing more.

use crossterm::event::{KeyCode as TerminalKeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ono_editor::{Editor, KeyCode, KeyPress, Renderer, key_press};
use ono_render::{Presentation, Theme};

#[test]
fn should_translate_a_control_combination_into_the_binding_a_user_pressed() {
    let event = KeyEvent::new(TerminalKeyCode::Char('a'), KeyModifiers::CONTROL);
    assert_eq!(key_press(event), Some(KeyPress::ctrl('a')));
}

#[test]
fn should_translate_a_named_key_into_its_editor_key() {
    let event = KeyEvent::new(TerminalKeyCode::Home, KeyModifiers::NONE);
    assert_eq!(key_press(event), Some(KeyPress::key(KeyCode::Home)));
}

#[test]
fn should_ignore_a_key_release_so_a_key_is_never_acted_on_twice() {
    let mut event = KeyEvent::new(TerminalKeyCode::Char('a'), KeyModifiers::NONE);
    event.kind = KeyEventKind::Release;
    assert_eq!(key_press(event), None);
}

#[test]
fn should_write_every_line_of_the_frame_when_it_is_drawn() {
    let mut editor = Editor::new().with_prompt("ono> ");
    editor.set_line("get process");
    let frame = editor.frame(80, Presentation::Plain, &Theme::default());

    let mut renderer = Renderer::new(Vec::new());
    renderer.draw(&frame).expect("a vector accepts bytes");
    let written = String::from_utf8(renderer.output().clone()).expect("the frame is valid UTF-8");
    assert!(written.contains("ono> get process"), "got {written:?}");
}

#[test]
fn should_write_each_wrapped_row_on_its_own_terminal_line() {
    let mut editor = Editor::new().with_prompt("> ");
    editor.set_line("a".repeat(20));
    let frame = editor.frame(10, Presentation::Plain, &Theme::default());

    let mut renderer = Renderer::new(Vec::new());
    renderer.draw(&frame).expect("a vector accepts bytes");
    let written = String::from_utf8(renderer.output().clone()).expect("the frame is valid UTF-8");
    assert_eq!(
        written.matches("\r\n").count(),
        frame.lines.len() - 1,
        "one line break between rows and none after the last"
    );
}
