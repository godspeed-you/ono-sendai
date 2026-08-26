//! Table and stacked-record layout for a terminal of a known width.

use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

/// The marker that tells a reader a value was shortened.
///
/// Spec §13.3 requires truncation to be visible and favours ASCII-safe output, so the marker is
/// three full stops rather than a single-character ellipsis.
const TRUNCATION_MARKER: &str = "...";

/// Cells between two columns.
const COLUMN_GAP: usize = 2;

/// The narrowest a column may become before the table is abandoned for stacked records.
const MIN_COLUMN_WIDTH: usize = 4;

/// How a column's values sit within their width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// Text reads from the left. The default for names, paths and prose.
    #[default]
    Left,
    /// Values end at the same column, so magnitudes can be compared by eye.
    Right,
}

/// A column of a table: a stable header and how its values are aligned.
#[derive(Debug, Clone)]
pub struct Column {
    header: String,
    align: Align,
}

impl Column {
    /// A left-aligned column with the given header.
    #[must_use]
    pub fn new(header: impl Into<String>) -> Self {
        Self {
            header: header.into(),
            align: Align::Left,
        }
    }

    /// Sets the column's alignment.
    #[must_use]
    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// The column's header.
    #[must_use]
    pub fn header(&self) -> &str {
        &self.header
    }

    /// How the column's values are aligned.
    #[must_use]
    pub fn alignment(&self) -> Align {
        self.align
    }
}

/// One value's presentation text.
///
/// A cell holds the complete text. Shortening happens during layout and never here, so the full
/// value stays available for copy, export and serialization as spec §13.3 requires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    text: String,
}

impl Cell {
    /// A cell showing `text`.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    /// The complete text, whatever a layout chose to show.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The cell's width in terminal cells.
    #[must_use]
    pub fn width(&self) -> usize {
        self.text.width()
    }
}

/// A collection of records to be presented, as columns and rows of cell text.
#[derive(Debug, Clone, Default)]
pub struct Table {
    columns: Vec<Column>,
    rows: Vec<Vec<Cell>>,
}

impl Table {
    /// An empty table with the given columns.
    #[must_use]
    pub fn new(columns: Vec<Column>) -> Self {
        Self {
            columns,
            rows: Vec::new(),
        }
    }

    /// Appends a row, in builder style.
    ///
    /// A row shorter than the column list is padded with empty cells; a longer one is truncated,
    /// so a malformed row can never make the layout panic in front of a user.
    #[must_use]
    pub fn with_row(mut self, cells: Vec<Cell>) -> Self {
        self.push_row(cells);
        self
    }

    /// Appends a row.
    pub fn push_row(&mut self, mut cells: Vec<Cell>) {
        cells.resize(self.columns.len(), Cell::new(""));
        self.rows.push(cells);
    }

    /// The table's columns.
    #[must_use]
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// The table's rows.
    #[must_use]
    pub fn rows(&self) -> &[Vec<Cell>] {
        &self.rows
    }

    /// The cells of row `index`.
    ///
    /// # Panics
    ///
    /// Panics if `index` is out of range, like any slice index.
    #[must_use]
    pub fn row(&self, index: usize) -> &[Cell] {
        &self.rows[index]
    }

    /// How many rows the table holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether the table holds no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// A terminal of a known width, and the choices a renderer may make within it.
#[derive(Debug, Clone)]
pub struct Layout {
    width: usize,
    max_rows: Option<usize>,
}

impl Layout {
    /// A layout for a terminal `width` cells across.
    #[must_use]
    pub fn new(width: usize) -> Self {
        Self {
            width: width.max(1),
            max_rows: None,
        }
    }

    /// Shows at most `max_rows` rows, followed by a note saying how many were left out.
    #[must_use]
    pub fn max_rows(mut self, max_rows: usize) -> Self {
        self.max_rows = Some(max_rows);
        self
    }

    /// Lays `table` out as the lines a terminal would show.
    ///
    /// The choice between a table and stacked records is made here, from the width alone
    /// (spec §13.2). Both forms show every field: the underlying result is identical.
    #[must_use]
    pub fn render(&self, table: &Table) -> Vec<String> {
        if table.columns.is_empty() || table.rows.is_empty() {
            return vec![self.clip("(no results)")];
        }

        let shown = self
            .max_rows
            .unwrap_or(table.rows.len())
            .min(table.rows.len());
        let omitted = table.rows.len() - shown;
        let rows = &table.rows[..shown];

        let mut lines = match self.column_widths(table, rows) {
            Some(widths) => self.render_table(table, rows, &widths),
            None => self.render_stacked(table, rows),
        };

        if omitted > 0 {
            lines.push(self.clip(&format!("... {omitted} more")));
        }
        lines
    }

    /// The width to give each column, or `None` when even the minimum does not fit.
    ///
    /// Columns start at their natural width — the widest of the header and every value. When
    /// that overflows, the widest columns give up cells first, so a table of short identifiers
    /// beside one long path shortens the path rather than everything equally.
    fn column_widths(&self, table: &Table, rows: &[Vec<Cell>]) -> Option<Vec<usize>> {
        let count = table.columns.len();
        let mut widths: Vec<usize> = table
            .columns
            .iter()
            .enumerate()
            .map(|(index, column)| {
                let widest_value = rows
                    .iter()
                    .filter_map(|row| row.get(index))
                    .map(Cell::width)
                    .max()
                    .unwrap_or(0);
                column.header.width().max(widest_value)
            })
            .collect();

        let gaps = COLUMN_GAP * count.saturating_sub(1);
        if gaps + MIN_COLUMN_WIDTH * count > self.width {
            return None;
        }

        let budget = self.width - gaps;
        while widths.iter().sum::<usize>() > budget {
            let (widest, _) = widths
                .iter()
                .enumerate()
                .max_by_key(|(index, width)| (**width, std::cmp::Reverse(*index)))?;
            if widths[widest] <= MIN_COLUMN_WIDTH {
                return None;
            }
            widths[widest] -= 1;
        }
        Some(widths)
    }

    fn render_table(&self, table: &Table, rows: &[Vec<Cell>], widths: &[usize]) -> Vec<String> {
        let mut lines = Vec::with_capacity(rows.len() + 1);

        let headers: Vec<Cell> = table
            .columns
            .iter()
            .map(|column| Cell::new(column.header.clone()))
            .collect();
        lines.push(self.render_row(table, &headers, widths));

        for row in rows {
            lines.push(self.render_row(table, row, widths));
        }
        lines
    }

    fn render_row(&self, table: &Table, row: &[Cell], widths: &[usize]) -> String {
        let mut line = String::new();
        for (index, width) in widths.iter().enumerate() {
            if index > 0 {
                line.push_str(&" ".repeat(COLUMN_GAP));
            }
            let text = row.get(index).map_or("", Cell::text);
            let shortened = shorten(text, *width);
            let padding = width.saturating_sub(shortened.width());
            let align = table
                .columns
                .get(index)
                .map_or(Align::Left, Column::alignment);
            let last = index + 1 == widths.len();
            match align {
                Align::Left if last => line.push_str(&shortened),
                Align::Left => {
                    line.push_str(&shortened);
                    line.push_str(&" ".repeat(padding));
                }
                Align::Right => {
                    line.push_str(&" ".repeat(padding));
                    line.push_str(&shortened);
                }
            }
        }
        line
    }

    /// One line per field, for a terminal too narrow to hold a table (spec §13.2).
    fn render_stacked(&self, table: &Table, rows: &[Vec<Cell>]) -> Vec<String> {
        let label_width = table
            .columns
            .iter()
            .map(|column| column.header.width())
            .max()
            .unwrap_or(0)
            .min(self.width.saturating_sub(2));

        let mut lines = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            if index > 0 {
                lines.push(String::new());
            }
            for (column, cell) in table.columns.iter().zip(row) {
                let label = shorten(&column.header.to_lowercase(), label_width);
                let padding = label_width.saturating_sub(label.width());
                let available = self.width.saturating_sub(label_width + 1);
                let value = shorten(cell.text(), available);
                lines.push(format!("{label}{} {value}", " ".repeat(padding)));
            }
        }
        lines
    }

    fn clip(&self, text: &str) -> String {
        shorten(text, self.width)
    }
}

/// Shortens `text` to at most `width` terminal cells, marking that it was cut.
///
/// Never splits a character, and never returns something wider than asked for — a line that
/// escapes the terminal wraps and destroys the alignment of everything below it.
fn shorten(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    if width <= TRUNCATION_MARKER.len() {
        return take_cells(text, width);
    }
    let kept = take_cells(text, width - TRUNCATION_MARKER.len());
    format!("{kept}{TRUNCATION_MARKER}")
}

/// The longest prefix of `text` that fits in `width` terminal cells.
///
/// A wide character that would straddle the limit is dropped rather than half-printed, so the
/// result is always at most `width` cells and always valid UTF-8.
fn take_cells(text: &str, width: usize) -> String {
    let mut used = 0;
    let mut kept = String::new();
    for character in text.chars() {
        let cells = character.width().unwrap_or(0);
        if used + cells > width {
            break;
        }
        used += cells;
        kept.push(character);
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_return_the_text_unchanged_when_it_already_fits() {
        assert_eq!(shorten("nginx", 10), "nginx");
        assert_eq!(shorten("nginx", 5), "nginx");
    }

    #[test]
    fn should_never_exceed_the_requested_width_when_shortening_wide_characters() {
        for width in 0..12 {
            assert!(
                shorten("日本語のプロセス", width).width() <= width,
                "width {width}"
            );
        }
    }

    #[test]
    fn should_drop_a_character_that_would_straddle_the_limit_when_taking_cells() {
        // A wide character in a one-cell budget cannot be half-printed.
        assert_eq!(take_cells("日", 1), "");
        assert_eq!(take_cells("日", 2), "日");
    }
}
