//! Table layout is presentation, and presentation has a contract of its own: the same values
//! must look intentional at 80 columns and at 200 (docs/ACCEPTANCE.md §4.2), truncation must be
//! visible (spec §13.3), and headers must stay stable (spec §4.5).
//!
//! These tests assert what appears on the terminal, never how it was computed.

#![allow(
    clippy::panic,
    reason = "AGENTS.md §16: a helper shared by tests states its preconditions the same way a test does. clippy's allow-panic-in-tests only covers `#[test]` functions."
)]

use ono_render::{Align, Cell, Column, Layout, Table};

fn processes() -> Table {
    Table::new(vec![
        Column::new("PID").align(Align::Right),
        Column::new("NAME"),
        Column::new("CPU").align(Align::Right),
        Column::new("MEM").align(Align::Right),
        Column::new("USER"),
    ])
    .with_row(vec![
        Cell::new("812"),
        Cell::new("postgres"),
        Cell::new("24.8%"),
        Cell::new("1.20 GiB"),
        Cell::new("postgres"),
    ])
    .with_row(vec![
        Cell::new("4419"),
        Cell::new("nginx"),
        Cell::new("0.4%"),
        Cell::new("18.2 MiB"),
        Cell::new("www-data"),
    ])
}

fn width_of(line: &str) -> usize {
    unicode_width::UnicodeWidthStr::width(line)
}

/// The terminal column a value starts at, which is what "lined up" means to a reader. A byte
/// offset is not the same thing the moment a row contains a wide character.
fn column_of(line: &str, needle: &str) -> usize {
    let at = line
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} missing from {line:?}"));
    width_of(&line[..at])
}

#[test]
fn should_print_a_header_and_a_row_per_value_when_it_fits() {
    let lines = Layout::new(80).render(&processes());
    assert_eq!(lines.len(), 3, "header plus two rows, got {lines:#?}");
    assert!(lines[0].contains("PID"));
    assert!(lines[0].contains("USER"));
    assert!(lines[1].contains("postgres"));
    assert!(lines[2].contains("nginx"));
}

#[test]
fn should_align_columns_so_values_line_up_when_rendered() {
    let lines = Layout::new(80).render(&processes());
    // Every row starts its NAME column at the same terminal column, which is what makes a
    // table readable.
    assert_eq!(
        column_of(&lines[1], "postgres"),
        column_of(&lines[2], "nginx"),
        "columns must line up:\n{}",
        lines.join("\n")
    );
    assert!(
        !lines[0].contains("postgres") && !lines[0].contains("nginx"),
        "the header row holds headers, got {:?}",
        lines[0]
    );
}

#[test]
fn should_right_align_numeric_columns_so_magnitudes_compare_when_rendered() {
    let lines = Layout::new(80).render(&processes());
    let end_of = |line: &str, needle: &str| column_of(line, needle) + width_of(needle);
    assert_eq!(
        end_of(&lines[1], "24.8%"),
        end_of(&lines[2], "0.4%"),
        "right-aligned values must end at the same column:\n{}",
        lines.join("\n")
    );
}

#[test]
fn should_never_exceed_the_terminal_width_when_rendered_at_any_width() {
    for width in [20usize, 40, 60, 80, 120, 200] {
        for line in Layout::new(width).render(&processes()) {
            assert!(
                width_of(&line) <= width,
                "a {}-cell line escaped a {width}-column terminal: {line:?}",
                width_of(&line)
            );
        }
    }
}

#[test]
fn should_mark_a_shortened_value_so_the_reader_knows_it_was_cut_when_space_runs_out() {
    let table = Table::new(vec![Column::new("PATH")]).with_row(vec![Cell::new(
        "/var/lib/postgresql/16/main/base/16384/2608_vm",
    )]);
    let lines = Layout::new(24).render(&table);
    let row = &lines[1];
    assert!(
        row.contains("..."),
        "truncation must be visible (spec §13.3), got {row:?}"
    );
    assert!(width_of(row) <= 24);
    assert!(
        !row.contains("2608_vm"),
        "the tail was cut, so it must not still be printed: {row:?}"
    );
}

#[test]
fn should_keep_the_whole_value_available_even_when_the_row_shows_less_when_truncated() {
    // Spec §13.3: copy, export and serialization must retain the full value. The layout is a
    // view over the table; it never edits it.
    let table =
        Table::new(vec![Column::new("PATH")]).with_row(vec![Cell::new("/very/long/path/indeed")]);
    let _ = Layout::new(12).render(&table);
    assert_eq!(table.row(0)[0].text(), "/very/long/path/indeed");
}

#[test]
fn should_switch_to_a_stacked_record_when_the_terminal_is_too_narrow_for_a_table() {
    // Spec §13.2: "It MAY switch to stacked records for very narrow terminals." The underlying
    // result is identical — every field is still shown.
    let lines = Layout::new(18).render(&processes());
    let shown = lines.join("\n");
    for value in ["812", "postgres", "24.8%", "1.20 GiB"] {
        assert!(shown.contains(value), "{value:?} vanished:\n{shown}");
    }
    assert!(
        shown.lines().count() > 3,
        "a stacked record uses one line per field:\n{shown}"
    );
    for line in shown.lines() {
        assert!(width_of(line) <= 18, "line escaped the terminal: {line:?}");
    }
}

#[test]
fn should_measure_display_cells_rather_than_bytes_when_laying_out_wide_characters() {
    let table = Table::new(vec![Column::new("NAME"), Column::new("N")])
        .with_row(vec![Cell::new("日本語のプロセス"), Cell::new("1")])
        .with_row(vec![Cell::new("ascii"), Cell::new("2")]);
    let lines = Layout::new(40).render(&table);
    for line in &lines {
        assert!(width_of(line) <= 40, "{line:?}");
    }
    assert_eq!(
        column_of(&lines[1], "1"),
        column_of(&lines[2], "2"),
        "wide characters must be counted as two cells:\n{}",
        lines.join("\n")
    );
}

#[test]
fn should_not_split_a_multi_byte_character_when_truncating() {
    let table =
        Table::new(vec![Column::new("NAME")]).with_row(vec![Cell::new("日本語のプロセス名")]);
    for width in 6..20 {
        let lines = Layout::new(width).render(&table);
        for line in lines {
            assert!(line.is_char_boundary(line.len()), "{line:?}");
            assert!(width_of(&line) <= width, "{line:?} at width {width}");
        }
    }
}

#[test]
fn should_show_an_empty_result_as_such_rather_than_as_a_bare_header_when_there_are_no_rows() {
    let lines = Layout::new(80).render(&Table::new(vec![Column::new("PID"), Column::new("NAME")]));
    assert_eq!(lines.len(), 1, "got {lines:#?}");
    assert!(
        lines[0].contains("no ") || lines[0].contains("empty"),
        "an empty result must say so, got {:?}",
        lines[0]
    );
}

#[test]
fn should_render_identically_for_the_same_input_and_width_when_rendered_twice() {
    // Deterministic output is the contract for redirected and non-interactive use (spec §4.6).
    let table = processes();
    assert_eq!(
        Layout::new(80).render(&table),
        Layout::new(80).render(&table)
    );
}

#[test]
fn should_limit_the_rows_it_prints_and_say_how_many_it_left_out_when_asked() {
    let mut table = Table::new(vec![Column::new("N")]);
    for n in 0..500 {
        table.push_row(vec![Cell::new(n.to_string())]);
    }
    let lines = Layout::new(80).max_rows(10).render(&table);
    assert_eq!(
        lines.len(),
        12,
        "header, ten rows and a note, got {lines:#?}"
    );
    assert!(
        lines[11].contains("490"),
        "the note must say what was left out, got {:?}",
        lines[11]
    );
}

#[test]
fn should_keep_headers_in_the_same_order_as_the_columns_when_rendered() {
    let lines = Layout::new(200).render(&processes());
    let header = &lines[0];
    let mut previous = 0;
    for name in ["PID", "NAME", "CPU", "MEM", "USER"] {
        let at = header
            .find(name)
            .unwrap_or_else(|| panic!("{name} missing from {header:?}"));
        assert!(at >= previous, "columns reordered in {header:?}");
        previous = at;
    }
}
