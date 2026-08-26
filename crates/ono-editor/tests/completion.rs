//! Tab completion: nothing, one candidate, a common prefix, and the candidate list.

mod support;

use ono_editor::{Editor, KeyCode, KeyPress, Outcome};
use ono_render::{Presentation, Theme};
use support::{WordCompleter, type_text};
use unicode_width::UnicodeWidthStr;

fn tab(editor: &mut Editor) -> Outcome {
    editor.feed(KeyPress::key(KeyCode::Tab))
}

#[test]
fn should_leave_the_line_alone_when_nothing_completes() {
    let mut editor = Editor::new().with_completer(WordCompleter::new(vec!["process"]));
    type_text(&mut editor, "get zz");
    assert_eq!(tab(&mut editor), Outcome::Continue);
    assert_eq!(editor.line(), "get zz");
    let frame = editor.frame(80, Presentation::Plain, &Theme::default());
    assert_eq!(frame.lines.len(), 1, "nothing is listed");
}

#[test]
fn should_insert_the_candidate_when_exactly_one_matches() {
    let mut editor = Editor::new().with_completer(WordCompleter::new(vec!["process", "service"]));
    type_text(&mut editor, "get pro");
    tab(&mut editor);
    assert_eq!(editor.line(), "get process");
    assert_eq!(editor.cursor(), 11);
}

#[test]
fn should_insert_the_longest_common_prefix_when_several_candidates_match() {
    let mut editor =
        Editor::new().with_completer(WordCompleter::new(vec!["process", "procfs", "profile"]));
    type_text(&mut editor, "get pro");
    tab(&mut editor);
    assert_eq!(
        editor.line(),
        "get pro",
        "`pro` is already the common prefix"
    );

    let mut editor = Editor::new().with_completer(WordCompleter::new(vec!["process", "procfs"]));
    type_text(&mut editor, "get pro");
    tab(&mut editor);
    assert_eq!(editor.line(), "get proc");
}

#[test]
fn should_list_the_candidates_when_tab_is_pressed_a_second_time() {
    let mut editor =
        Editor::new().with_completer(WordCompleter::new(vec!["process", "procfs", "profile"]));
    type_text(&mut editor, "get pro");
    tab(&mut editor);
    let frame = editor.frame(80, Presentation::Plain, &Theme::default());
    assert_eq!(frame.lines.len(), 1, "the first Tab does not list");

    tab(&mut editor);
    let frame = editor.frame(80, Presentation::Plain, &Theme::default());
    let listed = frame.lines[1..].join(" ");
    assert!(listed.contains("process"), "got {listed:?}");
    assert!(listed.contains("procfs"), "got {listed:?}");
    assert!(listed.contains("profile"), "got {listed:?}");
    assert_eq!(
        frame.cursor_row, 0,
        "the cursor stays on the line being edited"
    );
}

#[test]
fn should_stop_listing_the_candidates_when_the_next_key_edits_the_line() {
    let mut editor = Editor::new().with_completer(WordCompleter::new(vec!["process", "procfs"]));
    type_text(&mut editor, "get pro");
    tab(&mut editor);
    tab(&mut editor);
    assert!(
        editor
            .frame(80, Presentation::Plain, &Theme::default())
            .lines
            .len()
            > 1
    );
    type_text(&mut editor, "e");
    assert_eq!(
        editor
            .frame(80, Presentation::Plain, &Theme::default())
            .lines
            .len(),
        1
    );
}

#[test]
fn should_lay_the_candidate_list_out_within_the_terminal_width() {
    let candidates = vec![
        "interface",
        "route",
        "neighbor",
        "socket",
        "connection",
        "service",
        "process",
        "mount",
        "filesystem",
        "user",
        "group",
        "environment",
    ];
    let mut editor = Editor::new().with_completer(WordCompleter::new(candidates.clone()));
    type_text(&mut editor, "get ");
    tab(&mut editor);
    tab(&mut editor);

    for width in [20_usize, 40, 80] {
        let frame = editor.frame(width, Presentation::Plain, &Theme::default());
        for line in &frame.lines {
            assert!(
                line.width() <= width,
                "a listed line must fit the terminal: {line:?} in width {width}"
            );
        }
        let listed = frame.lines[1..].join(" ");
        for candidate in &candidates {
            assert!(
                listed.contains(candidate),
                "{candidate} missing at width {width}"
            );
        }
    }
}

#[test]
fn should_complete_a_multi_byte_candidate_without_splitting_a_character() {
    let mut editor = Editor::new().with_completer(WordCompleter::new(vec!["日本語ファイル"]));
    type_text(&mut editor, "get 日本");
    tab(&mut editor);
    assert_eq!(editor.line(), "get 日本語ファイル");
    assert_eq!(editor.cursor(), editor.line().len());
}
