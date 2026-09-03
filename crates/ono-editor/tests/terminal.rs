//! The thin layer between the terminal and the editor: it translates and it paints, nothing more.

use crossterm::event::{KeyCode as TerminalKeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ono_editor::{Editor, KeyCode, KeyPress, Outcome, Renderer, key_press};
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

#[test]
fn should_clear_the_screen_and_home_the_cursor_before_repainting_at_the_top() {
    let mut editor = Editor::new().with_prompt("ono> ");
    editor.set_line("get process");
    let frame = editor.frame(80, Presentation::Plain, &Theme::default());

    let mut renderer = Renderer::new(Vec::new());
    renderer.draw(&frame).expect("a vector accepts bytes");
    renderer
        .redraw_from_top(&frame)
        .expect("a vector accepts bytes");
    let written = String::from_utf8(renderer.output().clone()).expect("the frame is valid UTF-8");

    let cleared = written
        .rfind("\x1b[2J")
        .expect("the whole screen is cleared, not just the rows below the cursor");
    let homed = written[cleared..]
        .find("\x1b[1;1H")
        .map(|at| cleared + at)
        .expect("the cursor is homed after the clear");
    let repainted = written[homed..]
        .find("ono> get process")
        .expect("the frame is painted again after the cursor is homed");
    assert!(
        repainted > 0,
        "clear, home, paint — in that order; got {written:?}"
    );
}

#[test]
fn should_keep_the_typed_line_when_ctrl_l_clears_the_screen() {
    let mut editor = Editor::new().with_prompt("ono> ");
    editor.set_line("get pro");
    assert_eq!(editor.feed(KeyPress::ctrl('l')), Outcome::Redraw);

    let frame = editor.frame(80, Presentation::Plain, &Theme::default());
    let mut renderer = Renderer::new(Vec::new());
    renderer
        .redraw_from_top(&frame)
        .expect("a vector accepts bytes");
    let written = String::from_utf8(renderer.output().clone()).expect("the frame is valid UTF-8");
    assert!(
        written.ends_with(&format!("ono> get pro\x1b[{}G", "ono> get pro".len() + 1)),
        "the line survives the clear and the cursor stands at its end; got {written:?}"
    );
}
