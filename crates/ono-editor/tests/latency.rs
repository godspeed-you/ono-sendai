//! Spec §34 budgets a keystroke-to-render round trip at under 8 ms. This test does not measure
//! that budget — a wall-clock assertion that tight is flaky on shared hardware. It bounds the
//! whole run generously instead, so a catastrophic regression (a quadratic re-highlight, an
//! allocation storm) fails the gate while ordinary machine noise does not.

mod support;

use std::time::{Duration, Instant};

use ono_editor::{Editor, KeyCode, KeyPress};
use ono_render::{Presentation, Theme};
use support::{DemoHighlighter, WordCompleter};

const LINE: &str = "get process | where cpu > 20 | select pid name user | sort cpu desc";

#[test]
fn should_stay_far_inside_the_keystroke_budget_when_a_realistic_line_is_typed() {
    let mut editor = Editor::new()
        .with_highlighter(DemoHighlighter)
        .with_completer(WordCompleter::new(vec!["process", "service", "socket"]))
        .with_prompt("local://~ > ");
    let theme = Theme::default();

    let rounds = 100;
    let start = Instant::now();
    for _ in 0..rounds {
        for character in LINE.chars() {
            editor.feed(KeyPress::char(character));
            let _ = editor.frame(80, Presentation::Terminal, &theme);
        }
        editor.feed(KeyPress::ctrl('a'));
        editor.feed(KeyPress::ctrl('e'));
        editor.feed(KeyPress::key(KeyCode::Enter));
    }
    let elapsed = start.elapsed();

    let keystrokes = rounds * (LINE.chars().count() + 3);
    assert!(
        elapsed < Duration::from_secs(1),
        "{keystrokes} keystrokes with a frame each took {elapsed:?}"
    );
}
