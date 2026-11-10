//! The width a redirected view is laid out at (spec v0.2 §4.6, v0.4 §39.3, ADR-0979).
//!
//! A stream that is not a terminal has no width of its own. It is laid out at eighty columns —
//! byte-for-byte reproducible, as §4.6 requires — unless the environment states a width, and then
//! at that one: `COLUMNS` is how a caller says how wide the reader is when no terminal can.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use ono_testkit::Shell;

/// The longest line `script` printed with `COLUMNS` set to `columns`, or left out.
fn widest_line(script: &str, columns: Option<&str>) -> usize {
    let shell = Shell::new().env("NO_COLOR", "1").args(["-c", script]);
    let shell = match columns {
        Some(columns) => shell.env("COLUMNS", columns),
        None => shell.env_remove("COLUMNS"),
    };
    let run = shell.run();
    run.assert_success();
    run.stdout()
        .lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0)
}

#[test]
fn should_lay_a_redirected_table_out_at_the_width_the_environment_states() {
    // The description column of the settings is far wider than any terminal, so this table is
    // as wide as it is allowed to be: the stated width when one is stated, eighty columns when
    // none is (ADR-0979).
    let table = "get config | select key description";
    let narrow = widest_line(table, Some("48"));
    assert!(
        narrow <= 48,
        "a view asked to fit forty-eight columns fits them, got a line of {narrow}"
    );

    let wide = widest_line(table, Some("132"));
    assert!(
        wide > 80 && wide <= 132,
        "a view told its reader is 132 columns wide uses them, got a line of {wide}"
    );

    let unstated = widest_line(table, None);
    assert!(
        unstated <= 80,
        "spec v0.2 §4.6: with no width stated a redirected view is laid out at eighty columns, \
         got a line of {unstated}"
    );
}

// `map` and `look` belong to the spatial tier, which a core build leaves out (ADR-0925).
#[cfg(feature = "spatial")]
#[test]
fn should_lay_every_redirected_view_out_by_the_same_rule() {
    // The map was the one view that honoured `COLUMNS` off a terminal (ADR-0166). It keeps doing
    // so, and so does everything else: one rule for where a width comes from.
    for script in ["map", "look", "get config"] {
        let narrow = widest_line(script, Some("40"));
        assert!(
            narrow <= 40,
            "`{script}` at forty columns fits them, got a line of {narrow}"
        );
    }
}
