//! The text being edited: what the buffer holds and where the cursor sits after each operation.

use ono_editor::LineBuffer;

#[test]
fn should_insert_text_at_the_cursor_when_typing() {
    let mut buffer = LineBuffer::new();
    buffer.insert_str("get process");
    assert_eq!(buffer.text(), "get process");
    assert_eq!(buffer.cursor(), 11, "the cursor follows what was typed");
}

#[test]
fn should_insert_between_existing_text_when_the_cursor_is_not_at_the_end() {
    let mut buffer = LineBuffer::from_text("get process");
    buffer.move_line_start();
    buffer.insert_str("ono:");
    assert_eq!(buffer.text(), "ono:get process");
    assert_eq!(buffer.cursor(), 4);
}

#[test]
fn should_step_over_whole_characters_when_the_text_is_multi_byte() {
    let mut buffer = LineBuffer::from_text("äöü");
    assert_eq!(buffer.cursor(), 6, "three two-byte characters");
    assert!(buffer.move_left());
    assert_eq!(buffer.cursor(), 4);
    assert!(buffer.move_left());
    assert_eq!(buffer.cursor(), 2);
    assert!(buffer.move_left());
    assert_eq!(buffer.cursor(), 0);
    assert!(!buffer.move_left(), "there is nothing left of the start");
}

#[test]
fn should_delete_a_whole_character_backwards_when_it_is_multi_byte() {
    let mut buffer = LineBuffer::from_text("日本語");
    assert!(buffer.delete_backward_char());
    assert_eq!(buffer.text(), "日本");
    assert_eq!(buffer.cursor(), 6);
}

#[test]
fn should_delete_a_whole_character_forwards_when_it_is_multi_byte() {
    let mut buffer = LineBuffer::from_text("日本語");
    buffer.move_line_start();
    assert!(buffer.delete_forward_char());
    assert_eq!(buffer.text(), "本語");
    assert_eq!(buffer.cursor(), 0);
}

#[test]
fn should_move_to_the_start_of_the_previous_sub_word_when_moving_word_left() {
    let mut buffer = LineBuffer::from_text("get a-b c");
    buffer.move_word_left();
    assert_eq!(buffer.cursor(), 8, "the word `c`");
    buffer.move_word_left();
    assert_eq!(buffer.cursor(), 6, "punctuation ends a sub-word, so `b`");
    buffer.move_word_left();
    assert_eq!(buffer.cursor(), 4, "then `a`");
    buffer.move_word_left();
    assert_eq!(buffer.cursor(), 0, "then `get`");
}

#[test]
fn should_move_to_the_end_of_the_next_sub_word_when_moving_word_right() {
    let mut buffer = LineBuffer::from_text("get a-b c");
    buffer.move_line_start();
    buffer.move_word_right();
    assert_eq!(buffer.cursor(), 3);
    buffer.move_word_right();
    assert_eq!(buffer.cursor(), 5);
    buffer.move_word_right();
    assert_eq!(buffer.cursor(), 7);
    buffer.move_word_right();
    assert_eq!(buffer.cursor(), 9);
}

#[test]
fn should_move_within_the_current_line_when_the_buffer_has_several_lines() {
    let mut buffer = LineBuffer::from_text("each {\n  restart @");
    buffer.move_line_start();
    assert_eq!(
        buffer.cursor(),
        7,
        "the start of the second line, not of the buffer"
    );
    buffer.move_line_end();
    assert_eq!(buffer.cursor(), 18);
}

#[test]
fn should_kill_to_the_end_of_the_line_and_yank_it_back() {
    let mut buffer = LineBuffer::from_text("get process");
    buffer.set_cursor(3);
    buffer.kill_line_forward();
    assert_eq!(buffer.text(), "get");
    buffer.yank();
    assert_eq!(buffer.text(), "get process");
    assert_eq!(buffer.cursor(), 11);
}

#[test]
fn should_kill_to_the_start_of_the_line_when_asked() {
    let mut buffer = LineBuffer::from_text("get process");
    buffer.set_cursor(4);
    buffer.kill_line_backward();
    assert_eq!(buffer.text(), "process");
    assert_eq!(buffer.cursor(), 0);
}

#[test]
fn should_kill_the_whole_whitespace_delimited_word_before_the_cursor() {
    let mut buffer = LineBuffer::from_text("ls /usr/bin");
    buffer.kill_word_backward();
    assert_eq!(
        buffer.text(),
        "ls ",
        "a path is one word for the kill that shells bind to Ctrl-W"
    );
}

#[test]
fn should_kill_the_sub_word_after_the_cursor_when_killing_forward() {
    let mut buffer = LineBuffer::from_text("get process");
    buffer.move_line_start();
    buffer.kill_word_forward();
    assert_eq!(buffer.text(), " process");
    assert_eq!(buffer.cursor(), 0);
}

#[test]
fn should_yank_the_previous_kill_when_the_yank_is_popped() {
    let mut buffer = LineBuffer::new();
    buffer.insert_str("one");
    buffer.kill_line_backward();
    buffer.insert_str("two");
    buffer.kill_line_backward();
    buffer.yank();
    assert_eq!(
        buffer.text(),
        "two",
        "the most recent kill comes back first"
    );
    assert!(buffer.yank_pop());
    assert_eq!(buffer.text(), "one", "popping reaches the kill before it");
}

#[test]
fn should_refuse_to_pop_a_yank_when_the_last_operation_was_not_a_yank() {
    let mut buffer = LineBuffer::from_text("text");
    buffer.kill_line_backward();
    assert!(!buffer.yank_pop(), "there is nothing to replace");
    assert_eq!(buffer.text(), "");
}

#[test]
fn should_merge_consecutive_kills_into_one_yankable_entry() {
    let mut buffer = LineBuffer::from_text("get process list");
    buffer.kill_word_backward();
    buffer.kill_word_backward();
    assert_eq!(buffer.text(), "get ");
    buffer.yank();
    assert_eq!(buffer.text(), "get process list");
}

#[test]
fn should_swap_the_two_characters_before_the_cursor_at_the_end_of_a_line() {
    let mut buffer = LineBuffer::from_text("ab");
    buffer.transpose_chars();
    assert_eq!(buffer.text(), "ba");
    assert_eq!(buffer.cursor(), 2);
}

#[test]
fn should_swap_across_the_cursor_when_it_is_inside_the_line() {
    let mut buffer = LineBuffer::from_text("abc");
    buffer.set_cursor(1);
    buffer.transpose_chars();
    assert_eq!(buffer.text(), "bac");
    assert_eq!(buffer.cursor(), 2);
}

#[test]
fn should_transpose_multi_byte_characters_without_splitting_them() {
    let mut buffer = LineBuffer::from_text("aä");
    buffer.transpose_chars();
    assert_eq!(buffer.text(), "äa");
    assert_eq!(buffer.cursor(), 3);
}

#[test]
fn should_uppercase_the_word_after_the_cursor() {
    let mut buffer = LineBuffer::from_text("get process");
    buffer.move_line_start();
    buffer.uppercase_word();
    assert_eq!(buffer.text(), "GET process");
    assert_eq!(buffer.cursor(), 3);
}

#[test]
fn should_lowercase_the_word_after_the_cursor() {
    let mut buffer = LineBuffer::from_text("GET process");
    buffer.move_line_start();
    buffer.lowercase_word();
    assert_eq!(buffer.text(), "get process");
}

#[test]
fn should_capitalise_the_word_after_the_cursor() {
    let mut buffer = LineBuffer::from_text("gET process");
    buffer.move_line_start();
    buffer.capitalise_word();
    assert_eq!(buffer.text(), "Get process");
}

#[test]
fn should_keep_the_cursor_on_a_character_boundary_when_it_is_set_inside_a_character() {
    let mut buffer = LineBuffer::from_text("日本");
    buffer.set_cursor(1);
    assert_eq!(
        buffer.cursor(),
        0,
        "an offset inside a character snaps back"
    );
    buffer.set_cursor(99);
    assert_eq!(
        buffer.cursor(),
        6,
        "an offset past the end snaps to the end"
    );
}

#[test]
fn should_replace_a_range_and_carry_the_cursor_with_it() {
    let mut buffer = LineBuffer::from_text("get pro");
    buffer.replace_range(4..7, "process");
    assert_eq!(buffer.text(), "get process");
    assert_eq!(buffer.cursor(), 11);
}
