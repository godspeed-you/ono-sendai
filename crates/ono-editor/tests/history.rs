//! Recall: previous and next, prefix-anchored search, and incremental reverse search.

mod support;

use ono_editor::{Editor, KeyCode, KeyPress, Modifiers, Outcome};
use ono_render::{Presentation, Theme};
use support::{DemoHighlighter, type_text};

fn editor_with_history() -> Editor {
    let mut editor = Editor::new().with_highlighter(DemoHighlighter);
    editor.set_history(vec![
        "git status".to_owned(),
        "ls -la".to_owned(),
        "git log".to_owned(),
    ]);
    editor
}

#[test]
fn should_recall_the_previous_entry_when_up_is_pressed_on_an_empty_line() {
    let mut editor = editor_with_history();
    editor.feed(KeyPress::key(KeyCode::Up));
    assert_eq!(editor.line(), "git log");
    editor.feed(KeyPress::key(KeyCode::Up));
    assert_eq!(editor.line(), "ls -la");
    editor.feed(KeyPress::key(KeyCode::Up));
    assert_eq!(editor.line(), "git status");
    editor.feed(KeyPress::key(KeyCode::Up));
    assert_eq!(editor.line(), "git status", "there is nothing older");
}

#[test]
fn should_come_back_to_the_line_being_typed_when_down_walks_past_the_newest_entry() {
    let mut editor = editor_with_history();
    type_text(&mut editor, "who");
    editor.feed(KeyPress::ctrl('p'));
    assert_eq!(editor.line(), "who", "no entry starts with `who`");
    editor.set_line("");
    editor.feed(KeyPress::key(KeyCode::Up));
    assert_eq!(editor.line(), "git log");
    editor.feed(KeyPress::key(KeyCode::Down));
    assert_eq!(editor.line(), "", "the line the user was typing comes back");
}

#[test]
fn should_search_by_prefix_when_up_is_pressed_on_a_line_that_has_text() {
    let mut editor = editor_with_history();
    type_text(&mut editor, "git");
    editor.feed(KeyPress::key(KeyCode::Up));
    assert_eq!(editor.line(), "git log");
    editor.feed(KeyPress::key(KeyCode::Up));
    assert_eq!(
        editor.line(),
        "git status",
        "`ls -la` does not start with `git`"
    );
    editor.feed(KeyPress::key(KeyCode::Down));
    assert_eq!(editor.line(), "git log");
    editor.feed(KeyPress::key(KeyCode::Down));
    assert_eq!(editor.line(), "git", "and back to what was typed");
}

#[test]
fn should_show_the_search_prompt_and_the_match_when_ctrl_r_is_pressed() {
    let mut editor = editor_with_history();
    editor.feed(KeyPress::ctrl('r'));
    type_text(&mut editor, "sta");
    assert_eq!(editor.line(), "git status");

    let frame = editor.frame(80, Presentation::Plain, &Theme::default());
    assert!(
        frame.lines[0].starts_with("(reverse-i-search)`sta': "),
        "the search has its own prompt, got {:?}",
        frame.lines[0]
    );
    assert!(frame.lines[0].contains("git status"));
}

#[test]
fn should_report_a_failed_search_and_keep_the_line_when_nothing_matches() {
    let mut editor = editor_with_history();
    editor.feed(KeyPress::ctrl('r'));
    type_text(&mut editor, "zzz");
    assert_eq!(editor.line(), "", "a failed search changes nothing");

    let frame = editor.frame(80, Presentation::Plain, &Theme::default());
    assert!(
        frame.lines[0].starts_with("(failed reverse-i-search)`zzz': "),
        "got {:?}",
        frame.lines[0]
    );
}

#[test]
fn should_reach_an_older_match_when_ctrl_r_is_pressed_again() {
    let mut editor = editor_with_history();
    editor.feed(KeyPress::ctrl('r'));
    type_text(&mut editor, "git");
    assert_eq!(editor.line(), "git log");
    editor.feed(KeyPress::ctrl('r'));
    assert_eq!(editor.line(), "git status");
}

#[test]
fn should_restore_the_line_being_typed_when_a_reverse_search_is_cancelled() {
    let mut editor = editor_with_history();
    type_text(&mut editor, "who");
    editor.feed(KeyPress::ctrl('r'));
    type_text(&mut editor, "git");
    assert_eq!(editor.line(), "git log");
    editor.feed(KeyPress::ctrl('c'));
    assert_eq!(editor.line(), "who");

    let frame = editor.frame(80, Presentation::Plain, &Theme::default());
    assert!(
        !frame.lines[0].contains("reverse-i-search"),
        "the search prompt is gone"
    );
}

#[test]
fn should_submit_the_match_when_enter_ends_a_reverse_search() {
    let mut editor = editor_with_history();
    editor.feed(KeyPress::ctrl('r'));
    type_text(&mut editor, "sta");
    assert_eq!(
        editor.feed(KeyPress::key(KeyCode::Enter)),
        Outcome::Submit("git status".to_owned())
    );
}

#[test]
fn should_leave_the_search_and_keep_the_match_when_a_movement_key_is_pressed() {
    let mut editor = editor_with_history();
    editor.feed(KeyPress::ctrl('r'));
    type_text(&mut editor, "sta");
    editor.feed(KeyPress::ctrl('e'));
    assert_eq!(editor.line(), "git status");
    assert_eq!(
        editor.cursor(),
        10,
        "editing resumes at the end of the match"
    );

    let frame = editor.frame(80, Presentation::Plain, &Theme::default());
    assert!(!frame.lines[0].contains("reverse-i-search"));
}

#[test]
fn should_narrow_the_search_when_the_query_grows_and_widen_it_when_it_shrinks() {
    let mut editor = editor_with_history();
    editor.feed(KeyPress::ctrl('r'));
    type_text(&mut editor, "l");
    assert_eq!(editor.line(), "git log");
    type_text(&mut editor, "s");
    assert_eq!(
        editor.line(),
        "ls -la",
        "`ls` is the newest entry holding `ls`"
    );
    editor.feed(KeyPress::key(KeyCode::Backspace));
    assert_eq!(
        editor.line(),
        "git log",
        "the search restarts from the newest"
    );
}

#[test]
fn should_remember_a_submitted_line_when_it_is_pushed_into_the_history() {
    let mut editor = Editor::new().with_highlighter(DemoHighlighter);
    type_text(&mut editor, "uptime");
    editor.feed(KeyPress::key(KeyCode::Enter));
    editor.push_history("uptime");
    editor.feed(KeyPress::key(KeyCode::Up));
    assert_eq!(editor.line(), "uptime");
}

#[test]
fn should_walk_past_an_entry_that_does_not_match_when_ctrl_up_is_pressed() {
    // Issue #122, ADR-0564: the bare arrow is anchored on what has been typed, so with `get `
    // typed it can only ever reach `get x`. Ctrl-Up is the walk that reaches the rest.
    let mut editor = Editor::new().with_highlighter(DemoHighlighter);
    editor.set_history(vec!["a".to_owned(), "b".to_owned(), "get x".to_owned()]);
    type_text(&mut editor, "get ");

    editor.feed(KeyPress::new(KeyCode::Up, Modifiers::CTRL));
    assert_eq!(editor.line(), "get x");
    editor.feed(KeyPress::new(KeyCode::Up, Modifiers::CTRL));
    assert_eq!(
        editor.line(),
        "b",
        "the unanchored walk reaches an entry the anchor excludes"
    );
    editor.feed(KeyPress::new(KeyCode::Up, Modifiers::CTRL));
    assert_eq!(editor.line(), "a");
    editor.feed(KeyPress::new(KeyCode::Up, Modifiers::CTRL));
    assert_eq!(editor.line(), "a", "there is nothing older");
}

#[test]
fn should_keep_the_bare_arrow_anchored_when_ctrl_up_exists_beside_it() {
    let mut editor = Editor::new().with_highlighter(DemoHighlighter);
    editor.set_history(vec!["a".to_owned(), "b".to_owned(), "get x".to_owned()]);
    type_text(&mut editor, "get ");

    editor.feed(KeyPress::key(KeyCode::Up));
    assert_eq!(editor.line(), "get x");
    editor.feed(KeyPress::key(KeyCode::Up));
    assert_eq!(
        editor.line(),
        "get x",
        "the anchored walk stops where the anchor stops"
    );
}

#[test]
fn should_restore_the_line_being_typed_when_ctrl_down_walks_past_the_newest_entry() {
    let mut editor = Editor::new().with_highlighter(DemoHighlighter);
    editor.set_history(vec!["a".to_owned(), "b".to_owned(), "get x".to_owned()]);
    type_text(&mut editor, "get ");

    editor.feed(KeyPress::new(KeyCode::Up, Modifiers::CTRL));
    editor.feed(KeyPress::new(KeyCode::Up, Modifiers::CTRL));
    assert_eq!(editor.line(), "b");
    editor.feed(KeyPress::new(KeyCode::Down, Modifiers::CTRL));
    assert_eq!(editor.line(), "get x");
    editor.feed(KeyPress::new(KeyCode::Down, Modifiers::CTRL));
    assert_eq!(
        editor.line(),
        "get ",
        "the saved line comes back, as it does for the bare arrow"
    );
    editor.feed(KeyPress::new(KeyCode::Down, Modifiers::CTRL));
    assert_eq!(editor.line(), "get ", "and stays");
}

#[test]
fn should_let_the_anchored_walk_continue_from_where_ctrl_up_left_it() {
    // One walk, two kinds of step: the anchor is taken when the walk starts and applied only
    // by the anchored steps (ADR-0564).
    let mut editor = Editor::new().with_highlighter(DemoHighlighter);
    editor.set_history(vec![
        "get a".to_owned(),
        "b".to_owned(),
        "get c".to_owned(),
        "d".to_owned(),
    ]);
    type_text(&mut editor, "get ");

    editor.feed(KeyPress::new(KeyCode::Up, Modifiers::CTRL));
    assert_eq!(editor.line(), "d");
    editor.feed(KeyPress::key(KeyCode::Up));
    assert_eq!(
        editor.line(),
        "get c",
        "the anchored step skips what the anchor excludes"
    );
    editor.feed(KeyPress::key(KeyCode::Up));
    assert_eq!(editor.line(), "get a");
    editor.feed(KeyPress::new(KeyCode::Down, Modifiers::CTRL));
    assert_eq!(
        editor.line(),
        "b",
        "and the unanchored step takes the next entry whatever it is"
    );
}
