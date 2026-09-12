//! Tab completion: nothing, one candidate, a common prefix, and the candidate list.

mod support;

use ono_editor::{Editor, KeyCode, KeyPress, Outcome};
use ono_render::{Presentation, Theme};
use support::{DocCompleter, WordCompleter, type_text};
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

/// The frame after two Tabs on `line`, at `width` cells.
fn listed_twice(completer: DocCompleter, line: &str, width: usize) -> (Editor, Vec<String>) {
    let mut editor = Editor::new().with_completer(completer);
    type_text(&mut editor, line);
    tab(&mut editor);
    tab(&mut editor);
    let frame = editor.frame(width, Presentation::Plain, &Theme::default());
    (editor, frame.lines[1..].to_vec())
}

#[test]
fn should_show_each_candidates_doc_beside_it_when_listing() {
    // Issue #136: the registry documents every field and operator, and a listing of bare names
    // never said what `~=` means. Each documented candidate gets a line of its own, with its doc
    // beside it, the way fish lists them.
    let (_, listed) = listed_twice(
        DocCompleter::new(&[
            ("cpu", Some("The share of one CPU the process used.")),
            ("cpu_window", Some("The window `cpu` is the share over.")),
            ("comm", None),
        ]),
        "where c",
        100,
    );
    assert_eq!(listed.len(), 3, "one line per candidate, got {listed:?}");
    let cpu = listed
        .iter()
        .find(|line| line.starts_with("cpu "))
        .unwrap_or_else(|| panic!("`cpu` is listed on a line of its own, got {listed:?}"));
    assert!(
        cpu.contains("The share of one CPU the process used."),
        "the doc stands beside its candidate, got {cpu:?}"
    );
    let column = |line: &str, doc: &str| line.find(doc).map(|at| line[..at].width());
    assert_eq!(
        column(cpu, "The share"),
        column(
            listed
                .iter()
                .find(|line| line.starts_with("cpu_window"))
                .map_or("", String::as_str),
            "The window"
        ),
        "the docs start in one column, so the names read as a list, got {listed:?}"
    );
    assert!(
        listed.iter().any(|line| line.trim_end() == "comm"),
        "a candidate without a doc is listed by its name alone, got {listed:?}"
    );
}

#[test]
fn should_insert_only_the_candidate_never_its_doc() {
    // Issue #136: what is shown beside a candidate is presentation; the line gets the text.
    let mut editor =
        Editor::new().with_completer(DocCompleter::new(&[("pid", Some("The process id."))]));
    type_text(&mut editor, "where pi");
    tab(&mut editor);
    assert_eq!(editor.line(), "where pid");

    let mut editor = Editor::new().with_completer(DocCompleter::new(&[
        ("cpu", Some("The share of one CPU the process used.")),
        ("cpu_window", Some("The window `cpu` is the share over.")),
    ]));
    type_text(&mut editor, "where c");
    tab(&mut editor);
    tab(&mut editor);
    assert_eq!(
        editor.line(),
        "where cpu",
        "the common prefix is inserted and the listing leaves the line alone"
    );
}

#[test]
fn should_trim_a_long_doc_to_the_terminal_width() {
    let long = "A doc long enough to run past any narrow terminal, and then some more words.";
    for width in [24_usize, 40, 60] {
        let (_, listed) = listed_twice(
            DocCompleter::new(&[("memory", Some(long)), ("mount", Some("短い説明です"))]),
            "get m",
            width,
        );
        for line in &listed {
            assert!(
                line.width() <= width,
                "a listed line fits the terminal: {line:?} is wider than {width}"
            );
        }
        assert!(
            listed.iter().any(|line| line.starts_with("memory")),
            "the candidate itself is never what gets trimmed away, got {listed:?}"
        );
    }
}

#[test]
fn should_list_the_names_alone_where_no_candidate_is_documented() {
    let (_, listed) = listed_twice(
        DocCompleter::new(&[("alpha.txt", None), ("beta.log", None)]),
        "cat ",
        80,
    );
    assert_eq!(
        listed,
        ["alpha.txt  beta.log"],
        "without docs the listing keeps its columns, exactly as before"
    );
}

#[test]
fn should_sanitise_a_hostile_doc_before_showing_it() {
    // ADR-0015 T4 holds for docs as it does for candidates: a plugin's schema is data.
    let (_, listed) = listed_twice(
        DocCompleter::new(&[
            ("pid", Some("id\u{1b}]0;pwned\u{7}\nsecond line")),
            ("ppid", Some("parent")),
        ]),
        "where p",
        80,
    );
    let joined = listed.join("\n");
    assert!(
        !joined.contains('\u{1b}') && !joined.contains('\u{7}'),
        "no doc byte may reach the terminal as a control sequence, got {joined:?}"
    );
    assert_eq!(
        listed.len(),
        2,
        "a doc stays on its candidate's line, got {listed:?}"
    );
}

#[test]
fn should_complete_a_multi_byte_candidate_without_splitting_a_character() {
    let mut editor = Editor::new().with_completer(WordCompleter::new(vec!["日本語ファイル"]));
    type_text(&mut editor, "get 日本");
    tab(&mut editor);
    assert_eq!(editor.line(), "get 日本語ファイル");
    assert_eq!(editor.cursor(), editor.line().len());
}

#[test]
fn should_sanitise_a_hostile_candidate_before_showing_it() {
    // ADR-0015 T4: a completion source is data. A candidate carrying escape bytes — a poisoned
    // history file, a filename — must not move the cursor or retitle the terminal from inside
    // the candidate list.
    let mut editor = Editor::new().with_completer(WordCompleter::new(vec![
        "pro\u{1b}[2Jcess",
        "pro\u{1b}]0;pwned\u{7}be",
    ]));
    type_text(&mut editor, "get pro");
    tab(&mut editor);
    tab(&mut editor);

    let frame = editor.frame(80, Presentation::Terminal, &Theme::default());
    let listed = frame.lines[1..].join(" ");
    assert!(
        !listed.contains('\u{1b}') && !listed.contains('\u{7}'),
        "no candidate byte may reach the terminal as a control sequence, got {listed:?}"
    );
    assert!(
        listed.contains("cess"),
        "the candidate is still shown, as data: {listed:?}"
    );
}
