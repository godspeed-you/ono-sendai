//! Every default binding, stated as the behaviour a user sees.

mod support;

use ono_editor::{Editor, KeyCode, KeyPress, Modifiers, Outcome};
use support::{DemoHighlighter, type_text};

fn editor_with(text: &str) -> Editor {
    let mut editor = Editor::new();
    type_text(&mut editor, text);
    editor
}

#[test]
fn should_move_to_the_start_of_the_line_when_ctrl_a_is_pressed() {
    let mut editor = editor_with("get process");
    assert_eq!(editor.feed(KeyPress::ctrl('a')), Outcome::Continue);
    assert_eq!(editor.cursor(), 0);
}

#[test]
fn should_move_to_the_end_of_the_line_when_ctrl_e_is_pressed() {
    let mut editor = editor_with("get process");
    editor.feed(KeyPress::ctrl('a'));
    editor.feed(KeyPress::ctrl('e'));
    assert_eq!(editor.cursor(), 11);
}

#[test]
fn should_step_one_character_when_ctrl_b_and_ctrl_f_are_pressed() {
    let mut editor = editor_with("日本");
    editor.feed(KeyPress::ctrl('b'));
    assert_eq!(editor.cursor(), 3, "one whole wide character back");
    editor.feed(KeyPress::ctrl('f'));
    assert_eq!(editor.cursor(), 6);
}

#[test]
fn should_step_one_word_when_alt_b_and_alt_f_are_pressed() {
    let mut editor = editor_with("get process");
    editor.feed(KeyPress::alt('b'));
    assert_eq!(editor.cursor(), 4);
    editor.feed(KeyPress::alt('f'));
    assert_eq!(editor.cursor(), 11);
}

#[test]
fn should_delete_the_character_before_the_cursor_when_backspace_is_pressed() {
    let mut editor = editor_with("日本語");
    editor.feed(KeyPress::key(KeyCode::Backspace));
    assert_eq!(editor.line(), "日本");
    editor.feed(KeyPress::ctrl('h'));
    assert_eq!(editor.line(), "日", "Ctrl-H is the same key");
}

#[test]
fn should_delete_the_character_under_the_cursor_when_delete_is_pressed() {
    let mut editor = editor_with("abc");
    editor.feed(KeyPress::ctrl('a'));
    editor.feed(KeyPress::key(KeyCode::Delete));
    assert_eq!(editor.line(), "bc");
}

#[test]
fn should_kill_to_the_end_of_the_line_when_ctrl_k_is_pressed() {
    let mut editor = editor_with("get process");
    editor.feed(KeyPress::ctrl('a'));
    editor.feed(KeyPress::ctrl('f'));
    editor.feed(KeyPress::ctrl('f'));
    editor.feed(KeyPress::ctrl('f'));
    editor.feed(KeyPress::ctrl('k'));
    assert_eq!(editor.line(), "get");
}

#[test]
fn should_kill_to_the_start_of_the_line_when_ctrl_u_is_pressed() {
    let mut editor = editor_with("get process");
    editor.feed(KeyPress::alt('b'));
    editor.feed(KeyPress::ctrl('u'));
    assert_eq!(editor.line(), "process");
    assert_eq!(editor.cursor(), 0);
}

#[test]
fn should_delete_the_word_before_the_cursor_when_ctrl_w_is_pressed() {
    let mut editor = editor_with("get process | where cpu");
    editor.feed(KeyPress::ctrl('w'));
    assert_eq!(editor.line(), "get process | where ");
}

#[test]
fn should_delete_the_word_after_the_cursor_when_alt_d_is_pressed() {
    let mut editor = editor_with("get process");
    editor.feed(KeyPress::ctrl('a'));
    editor.feed(KeyPress::alt('d'));
    assert_eq!(editor.line(), " process");
}

#[test]
fn should_yank_the_last_kill_when_ctrl_y_is_pressed() {
    let mut editor = editor_with("get process");
    editor.feed(KeyPress::ctrl('w'));
    assert_eq!(editor.line(), "get ");
    editor.feed(KeyPress::ctrl('y'));
    assert_eq!(editor.line(), "get process");
}

#[test]
fn should_reach_the_kill_before_the_last_one_when_alt_y_is_pressed() {
    let mut editor = Editor::new();
    type_text(&mut editor, "one");
    editor.feed(KeyPress::ctrl('u'));
    type_text(&mut editor, "two");
    editor.feed(KeyPress::ctrl('u'));
    editor.feed(KeyPress::ctrl('y'));
    assert_eq!(editor.line(), "two");
    editor.feed(KeyPress::alt('y'));
    assert_eq!(editor.line(), "one");
}

#[test]
fn should_swap_the_last_two_characters_when_ctrl_t_is_pressed() {
    let mut editor = editor_with("ab");
    editor.feed(KeyPress::ctrl('t'));
    assert_eq!(editor.line(), "ba");
}

#[test]
fn should_change_the_case_of_a_word_when_the_case_bindings_are_pressed() {
    let mut editor = editor_with("get process");
    editor.feed(KeyPress::ctrl('a'));
    editor.feed(KeyPress::alt('u'));
    assert_eq!(editor.line(), "GET process");
    editor.feed(KeyPress::ctrl('a'));
    editor.feed(KeyPress::alt('l'));
    assert_eq!(editor.line(), "get process");
    editor.feed(KeyPress::ctrl('a'));
    editor.feed(KeyPress::alt('c'));
    assert_eq!(editor.line(), "Get process");
}

#[test]
fn should_ask_for_a_repaint_and_keep_the_line_when_ctrl_l_is_pressed() {
    let mut editor = editor_with("get process");
    assert_eq!(editor.feed(KeyPress::ctrl('l')), Outcome::Redraw);
    assert_eq!(
        editor.line(),
        "get process",
        "clearing the screen keeps the line"
    );
}

#[test]
fn should_clear_the_line_and_survive_when_ctrl_c_is_pressed() {
    let mut editor = editor_with("rm -rf /");
    assert_eq!(editor.feed(KeyPress::ctrl('c')), Outcome::Continue);
    assert_eq!(editor.line(), "", "the line is abandoned, the shell is not");
}

#[test]
fn should_report_a_cancelled_line_when_ctrl_c_is_pressed_on_an_empty_line() {
    let mut editor = Editor::new();
    assert_eq!(
        editor.feed(KeyPress::ctrl('c')),
        Outcome::Cancelled,
        "with nothing to clear, the caller starts a fresh prompt"
    );
}

#[test]
fn should_report_end_of_input_when_ctrl_d_is_pressed_on_an_empty_line() {
    let mut editor = Editor::new();
    assert_eq!(editor.feed(KeyPress::ctrl('d')), Outcome::EndOfInput);
}

#[test]
fn should_delete_forwards_when_ctrl_d_is_pressed_inside_a_line() {
    let mut editor = editor_with("abc");
    editor.feed(KeyPress::ctrl('a'));
    assert_eq!(editor.feed(KeyPress::ctrl('d')), Outcome::Continue);
    assert_eq!(editor.line(), "bc");
}

#[test]
fn should_submit_the_line_when_enter_is_pressed_and_the_statement_is_complete() {
    let mut editor = Editor::new().with_highlighter(DemoHighlighter);
    type_text(&mut editor, "get process");
    assert_eq!(
        editor.feed(KeyPress::key(KeyCode::Enter)),
        Outcome::Submit("get process".to_owned())
    );
    assert_eq!(editor.line(), "", "the editor starts the next line empty");
}

#[test]
fn should_continue_onto_a_new_line_when_enter_is_pressed_and_the_statement_is_open() {
    let mut editor = Editor::new().with_highlighter(DemoHighlighter);
    type_text(&mut editor, "each {");
    assert_eq!(
        editor.feed(KeyPress::key(KeyCode::Enter)),
        Outcome::Continue
    );
    assert_eq!(editor.line(), "each {\n");
    type_text(&mut editor, "  restart @ }");
    assert_eq!(
        editor.feed(KeyPress::key(KeyCode::Enter)),
        Outcome::Submit("each {\n  restart @ }".to_owned())
    );
}

#[test]
fn should_continue_onto_a_new_line_when_a_quote_is_still_open() {
    let mut editor = Editor::new().with_highlighter(DemoHighlighter);
    type_text(&mut editor, "echo \"hello");
    assert_eq!(
        editor.feed(KeyPress::key(KeyCode::Enter)),
        Outcome::Continue
    );
    assert_eq!(editor.line(), "echo \"hello\n");
}

#[test]
fn should_force_a_new_line_when_alt_enter_is_pressed_on_a_complete_statement() {
    let mut editor = Editor::new().with_highlighter(DemoHighlighter);
    type_text(&mut editor, "ls");
    assert_eq!(
        editor.feed(KeyPress::new(KeyCode::Enter, Modifiers::ALT)),
        Outcome::Continue
    );
    assert_eq!(editor.line(), "ls\n");
}

#[test]
fn should_edit_the_current_line_of_a_multi_line_buffer_when_moving_to_its_start() {
    let mut editor = Editor::new().with_highlighter(DemoHighlighter);
    type_text(&mut editor, "each {");
    editor.feed(KeyPress::key(KeyCode::Enter));
    type_text(&mut editor, "  restart @");
    editor.feed(KeyPress::ctrl('a'));
    assert_eq!(editor.cursor(), 7, "the start of the second line");
    editor.feed(KeyPress::ctrl('k'));
    assert_eq!(editor.line(), "each {\n");
}
