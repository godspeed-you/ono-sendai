//! What the editor draws: prompt, highlight, soft wrapping and the cursor's display position.

mod support;

use ono_editor::{Editor, KeyCode, KeyPress, Prompt};
use ono_render::{Presentation, Theme, Token};
use support::{DemoHighlighter, type_text};
use unicode_width::UnicodeWidthStr;

fn theme() -> Theme {
    Theme::default()
}

#[test]
fn should_show_the_prompt_and_the_line_when_a_frame_is_rendered() {
    let mut editor = Editor::new().with_prompt("local://~ > ");
    type_text(&mut editor, "get process");
    let frame = editor.frame(80, Presentation::Plain, &theme());
    assert_eq!(frame.lines, vec!["local://~ > get process".to_owned()]);
    assert_eq!(frame.cursor_row, 0);
    assert_eq!(frame.cursor_column, 23);
}

#[test]
fn should_emit_no_escape_sequence_when_the_destination_takes_no_colour() {
    for presentation in [
        Presentation::Plain,
        Presentation::Pipe,
        Presentation::Redirect,
        Presentation::Script,
    ] {
        let mut editor = Editor::new().with_highlighter(DemoHighlighter);
        type_text(&mut editor, "echo \"hello\" | to json");
        let frame = editor.frame(80, presentation, &theme());
        for line in &frame.lines {
            assert!(
                !line.contains('\u{1b}'),
                "{presentation:?} must be free of escapes, got {line:?}"
            );
        }
    }
}

#[test]
fn should_paint_the_highlighted_span_when_the_destination_takes_colour() {
    let mut editor = Editor::new().with_highlighter(DemoHighlighter);
    type_text(&mut editor, "echo \"hello\"");
    let frame = editor.frame(80, Presentation::Terminal, &theme());
    let painted = theme().paint("echo", Token::Accent, Presentation::Terminal);
    assert!(
        frame.lines[0].contains(&painted),
        "the head word is painted with its token, got {:?}",
        frame.lines[0]
    );
    assert!(
        frame.lines[0].ends_with("\"hello\""),
        "the rest is unpainted plain text"
    );
}

#[test]
fn should_count_display_cells_and_not_bytes_when_the_line_holds_wide_characters() {
    let mut editor = Editor::new().with_prompt("> ");
    type_text(&mut editor, "日本");
    let frame = editor.frame(80, Presentation::Plain, &theme());
    assert_eq!(
        frame.cursor_column, 6,
        "two cells of prompt and two wide characters"
    );
}

#[test]
fn should_wrap_at_the_terminal_width_and_report_where_the_cursor_landed() {
    let mut editor = Editor::new().with_prompt("> ");
    type_text(&mut editor, &"a".repeat(20));
    let frame = editor.frame(10, Presentation::Plain, &theme());
    assert_eq!(frame.lines.len(), 3, "8 + 10 + 2 cells");
    assert_eq!(frame.lines[0], "> aaaaaaaa");
    assert_eq!(frame.lines[1], "aaaaaaaaaa");
    assert_eq!(frame.lines[2], "aa");
    assert_eq!(frame.cursor_row, 2);
    assert_eq!(frame.cursor_column, 2);
}

#[test]
fn should_never_split_a_wide_character_across_the_wrap() {
    let mut editor = Editor::new().with_prompt("> ");
    type_text(&mut editor, &"日".repeat(6));
    let frame = editor.frame(9, Presentation::Plain, &theme());
    for line in &frame.lines {
        assert!(line.width() <= 9, "{line:?} escapes the terminal");
    }
    assert_eq!(frame.lines[0], "> 日日日", "a fourth would need cell 10");
}

#[test]
fn should_show_a_continuation_prompt_when_the_statement_runs_over_several_lines() {
    let mut editor = Editor::new()
        .with_highlighter(DemoHighlighter)
        .with_prompt("> ");
    editor.set_continuation_prompt(".. ");
    type_text(&mut editor, "each {");
    editor.feed(KeyPress::key(KeyCode::Enter));
    type_text(&mut editor, "restart @");
    let frame = editor.frame(80, Presentation::Plain, &theme());
    assert_eq!(
        frame.lines,
        vec!["> each {".to_owned(), ".. restart @".to_owned()]
    );
    assert_eq!(frame.cursor_row, 1);
    assert_eq!(frame.cursor_column, 12);
}

#[test]
fn should_refuse_to_insert_a_control_character_that_arrives_as_a_key_press() {
    let mut editor = Editor::new();
    type_text(&mut editor, "a\u{1b}b");
    assert_eq!(editor.line(), "ab", "an escape is a command, never text");
}

#[test]
fn should_neutralise_a_control_character_that_reaches_the_line() {
    let mut editor = Editor::new();
    editor.set_line("a\u{1b}[31mb");
    let frame = editor.frame(80, Presentation::Terminal, &theme());
    assert!(
        !frame.lines[0].contains("\u{1b}[31m"),
        "line content must never drive the terminal, got {:?}",
        frame.lines[0]
    );
    assert!(
        frame.lines[0].contains("\\u{1b}"),
        "the escape is shown as inert text, got {:?}",
        frame.lines[0]
    );
    assert_eq!(
        frame.cursor_column,
        2 + 6 + 6,
        "prompt, the six cells of the escape, and the six ordinary characters"
    );
}

#[test]
fn should_place_the_cursor_after_the_prompt_when_the_line_is_empty() {
    let editor = Editor::new().with_prompt("ono> ");
    let frame = editor.frame(80, Presentation::Plain, &theme());
    assert_eq!(frame.lines, vec!["ono> ".to_owned()]);
    assert_eq!(frame.cursor_column, 5);
}

#[test]
fn should_paint_the_prompt_segments_with_their_tokens_when_colour_is_allowed() {
    let prompt = Prompt::plain("local")
        .segment("://", Token::Dim)
        .segment("~ > ", Token::PromptContext);
    let editor = Editor::new().with_prompt(prompt);
    let plain = editor.frame(80, Presentation::Plain, &theme());
    assert_eq!(plain.lines, vec!["local://~ > ".to_owned()]);
    assert_eq!(plain.cursor_column, 12);

    let rich = editor.frame(80, Presentation::Terminal, &theme());
    assert!(rich.lines[0].contains('\u{1b}'), "the prompt is themed");
}
