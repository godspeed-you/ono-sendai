//! Table and stacked-record layout for a terminal of a known width.

use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

use crate::theme::{Theme, Token};
use ono_value::Value;

use crate::{Presentation, Renderer, View};

/// The marker that tells a reader a value was shortened.
///
/// Spec §13.3 requires truncation to be visible and favours ASCII-safe output, so the marker is
/// three full stops rather than a single-character ellipsis.
const TRUNCATION_MARKER: &str = "...";

/// Cells between two columns.
const COLUMN_GAP: usize = 2;

/// The narrowest a column may become before the table is abandoned for stacked records.
const MIN_COLUMN_WIDTH: usize = 4;

/// The narrowest a column that had to give up cells may become while the table is still a
/// table (ADR-0073). Eight cells show a pid, a percentage, a byte size or a short name whole;
/// a column cut below that shows mostly the truncation marker, and at that point stacked
/// records — one field per line, every value whole — are the honest rendering (spec §13.2).
const READABLE_COLUMN_WIDTH: usize = 8;

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
#[derive(Debug, Clone, Eq)]
pub struct Cell {
    text: String,
    token: Token,
}

/// Two cells are the same cell when they show the same text. The token is how it is painted,
/// which spec §44 makes a theme's business and never a value's.
impl PartialEq for Cell {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text
    }
}

impl Cell {
    /// A cell showing `text`, painted as an ordinary value.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            token: Token::Foreground,
        }
    }

    /// Paints the cell with a semantic token, so a number, a unit, a path and an unknown stay
    /// distinguishable (spec §44).
    #[must_use]
    pub fn with_token(mut self, token: Token) -> Self {
        self.token = token;
        self
    }

    /// The token the cell is painted with.
    #[must_use]
    pub fn token(&self) -> Token {
        self.token
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
    max_depth: Option<usize>,
}

impl Layout {
    /// A layout for a terminal `width` cells across.
    #[must_use]
    pub fn new(width: usize) -> Self {
        Self {
            width: width.max(1),
            max_rows: None,
            max_depth: None,
        }
    }

    /// Shows at most `max_rows` rows, followed by a note saying how many were left out.
    #[must_use]
    pub fn max_rows(mut self, max_rows: usize) -> Self {
        self.max_rows = Some(max_rows);
        self
    }

    /// Descends at most `max_depth` levels into a tree, then says how much it left out.
    #[must_use]
    pub fn max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = Some(max_depth);
        self
    }

    /// Lays a tree out as the lines a terminal would show (spec §22.4).
    #[must_use]
    pub fn render_tree(&self, root: &crate::TreeNode) -> Vec<String> {
        crate::tree::render(root, self.width, self.max_depth)
    }

    /// Lays a tree out and paints it, as far as `presentation` allows.
    ///
    /// The shape is decided first and painted afterwards, so colour can never move a connector:
    /// stripping the escape sequences from this output gives exactly [`render_tree`]'s.
    ///
    /// [`render_tree`]: Self::render_tree
    #[must_use]
    pub fn render_tree_styled(
        &self,
        root: &crate::TreeNode,
        theme: &Theme,
        presentation: Presentation,
    ) -> Vec<String> {
        crate::tree::lines(root, self.max_depth)
            .iter()
            .map(|line| {
                let plain = match &line.key {
                    Some(key) => format!("{}{key}: {}", line.indent, line.label),
                    None => format!("{}{}", line.indent, line.label),
                };
                let shown = shorten(&plain, self.width);
                let Some(rest) = shown.strip_prefix(line.indent.as_str()) else {
                    return theme.paint(&shown, Token::Border, presentation);
                };
                let mut painted = theme.paint(&line.indent, Token::Border, presentation);
                match &line.key {
                    Some(key) if rest.starts_with(key.as_str()) && rest.len() >= key.len() + 2 => {
                        painted.push_str(&theme.paint(
                            &rest[..key.len() + 2],
                            Token::TableKey,
                            presentation,
                        ));
                        painted.push_str(&theme.paint(
                            &rest[key.len() + 2..],
                            line.token,
                            presentation,
                        ));
                    }
                    Some(_) => painted.push_str(&theme.paint(rest, Token::TableKey, presentation)),
                    None => painted.push_str(&theme.paint(rest, line.token, presentation)),
                }
                painted
            })
            .collect()
    }

    /// Lays `values` out in one of the built-in views of spec §13.6.
    #[must_use]
    pub fn render_view(&self, renderer: &Renderer, values: &[Value], view: View) -> Vec<String> {
        self.view_lines(renderer, values, view, None)
    }

    /// Lays `values` out in one of the built-in views and paints them.
    #[must_use]
    pub fn render_view_styled(
        &self,
        renderer: &Renderer,
        values: &[Value],
        view: View,
        theme: &Theme,
        presentation: Presentation,
    ) -> Vec<String> {
        self.view_lines(
            renderer,
            values,
            view,
            Some(Paint {
                theme,
                presentation,
            }),
        )
    }

    /// Renders a structured error for a human (spec §16.2).
    ///
    /// [`Detail::Terse`] is what a failing command prints: the message, what it was about and
    /// what to do. [`Detail::Full`] is what `inspect @error` shows: the stable code of spec §43,
    /// the metadata and the whole causal chain.
    #[must_use]
    pub fn render_error(
        &self,
        error: &ono_value::ErrorValue,
        detail: Detail,
        theme: &Theme,
        presentation: Presentation,
    ) -> Vec<String> {
        let text = match detail {
            Detail::Terse => error.render_terse(),
            Detail::Full => error.render_full(),
        };
        text.lines()
            .map(|line| {
                let token = if line.starts_with(' ') || line.contains("caused by") {
                    Token::Dim
                } else if detail == Detail::Full && line.contains(error.code().code()) {
                    Token::ErrorCode
                } else if line == error.help().unwrap_or_default() {
                    Token::ErrorHint
                } else {
                    Token::ErrorCode
                };
                theme.paint(&shorten(line, self.width), token, presentation)
            })
            .collect()
    }

    fn view_lines(
        &self,
        renderer: &Renderer,
        values: &[Value],
        view: View,
        paint: Option<Paint<'_>>,
    ) -> Vec<String> {
        match view {
            View::Table => {
                let table = renderer.table(values);
                self.render_with(&table, paint)
            }
            View::List => {
                let table = renderer.table(values);
                if table.columns.is_empty() || table.rows.is_empty() {
                    return vec![self.paint_line("(no results)", Token::Dim, paint)];
                }
                let shown = self
                    .max_rows
                    .unwrap_or(table.rows.len())
                    .min(table.rows.len());
                let mut lines = self.render_stacked(&table, &table.rows[..shown], paint);
                let omitted = table.rows.len() - shown;
                if omitted > 0 {
                    lines.push(self.paint_line(&format!("... {omitted} more"), Token::Dim, paint));
                }
                lines
            }
            View::Tree => {
                let mut lines = Vec::new();
                for value in values {
                    if !lines.is_empty() {
                        lines.push(String::new());
                    }
                    let tree = renderer.tree(value);
                    lines.extend(match paint {
                        Some(paint) => {
                            self.render_tree_styled(&tree, paint.theme, paint.presentation)
                        }
                        None => self.render_tree(&tree),
                    });
                }
                lines
            }
            View::Raw => values
                .iter()
                .map(|value| {
                    // Raw is the escape hatch from a shortened view, so it is never shortened —
                    // but it still passes through the sanitiser, because it still reaches a
                    // terminal (spec §49).
                    let text =
                        ono_value::canonical_text(value).unwrap_or_else(|_| value.to_string());
                    self.paint_full(&crate::theme::sanitise(&text), Token::Foreground, paint)
                })
                .collect(),
            View::Hex => {
                let mut lines = Vec::new();
                for value in values {
                    match ono_value::to_bytes(value) {
                        Ok(bytes) => lines.extend(
                            hex_dump(&bytes)
                                .into_iter()
                                .map(|line| self.paint_line(&line, Token::Foreground, paint)),
                        ),
                        Err(error) => lines.push(self.paint_line(
                            &format!("{}: {}", error.code().name(), error.message()),
                            Token::ErrorCode,
                            paint,
                        )),
                    }
                }
                lines
            }
        }
    }

    fn paint_line(&self, text: &str, token: Token, paint: Option<Paint<'_>>) -> String {
        self.paint_full(&shorten(text, self.width), token, paint)
    }

    /// Colours text that has already been marked, or that is part of the frame rather than a
    /// value: a header, a key, a connector. The frame carries no meaning a marker could add.
    fn colour_full(&self, text: &str, token: Token, paint: Option<Paint<'_>>) -> String {
        match paint {
            Some(paint) => paint.theme.colour(text, token, paint.presentation),
            None => text.to_owned(),
        }
    }

    fn paint_full(&self, text: &str, token: Token, paint: Option<Paint<'_>>) -> String {
        match paint {
            Some(paint) => paint.theme.paint(text, token, paint.presentation),
            None => text.to_owned(),
        }
    }

    /// Lays `table` out as the lines a terminal would show.
    ///
    /// The choice between a table and stacked records is made here, from the width alone
    /// (spec §13.2). Both forms show every field: the underlying result is identical.
    #[must_use]
    pub fn render(&self, table: &Table) -> Vec<String> {
        self.render_with(table, None)
    }

    /// Lays `table` out and paints it, as far as `presentation` allows.
    ///
    /// Widths are computed from the unpainted text and the escape sequences are added last, so
    /// colour can never change the alignment: stripping them gives exactly [`render`]'s output.
    ///
    /// [`render`]: Self::render
    #[must_use]
    pub fn render_styled(
        &self,
        table: &Table,
        theme: &Theme,
        presentation: Presentation,
    ) -> Vec<String> {
        self.render_with(
            table,
            Some(Paint {
                theme,
                presentation,
            }),
        )
    }

    fn render_with(&self, table: &Table, paint: Option<Paint<'_>>) -> Vec<String> {
        if table.columns.is_empty() || table.rows.is_empty() {
            return vec![self.paint_line("(no results)", Token::Dim, paint)];
        }

        let shown = self
            .max_rows
            .unwrap_or(table.rows.len())
            .min(table.rows.len());
        let omitted = table.rows.len() - shown;
        // A marker is part of what the cell says, so it is added before the widths are measured
        // and the colour is added after (ADR-0558). Doing it the other way round would let a
        // theme move a column, which is exactly what the escapes-added-last rule prevents.
        let marked = marked_rows(&table.rows[..shown], paint);
        let rows = marked.as_deref().unwrap_or(&table.rows[..shown]);

        let mut lines = match self.column_widths(table, rows) {
            Some(widths) => self.render_table(table, rows, &widths, paint),
            None => self.render_stacked(table, rows, paint),
        };

        if omitted > 0 {
            lines.push(self.paint_line(&format!("... {omitted} more"), Token::Dim, paint));
        }
        lines
    }

    /// The width to give each column, or `None` when even the minimum does not fit.
    ///
    /// Columns start at their natural width — the widest of the header and every value. When
    /// that overflows, the widest columns give up cells first, so a table of short identifiers
    /// beside one long path shortens the path rather than everything equally. A column is cut
    /// no further than [`READABLE_COLUMN_WIDTH`]: past that the width does not permit a table
    /// (spec §13.2) and the records stack instead (ADR-0073). A single column is the exception —
    /// stacking it shows nothing a table does not — so it shortens down to the marker.
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
        let floor = if count > 1 {
            READABLE_COLUMN_WIDTH
        } else {
            MIN_COLUMN_WIDTH
        };
        while widths.iter().sum::<usize>() > budget {
            let (widest, _) = widths
                .iter()
                .enumerate()
                .max_by_key(|(index, width)| (**width, std::cmp::Reverse(*index)))?;
            if widths[widest] <= floor {
                return None;
            }
            widths[widest] -= 1;
        }
        Some(widths)
    }

    fn render_table(
        &self,
        table: &Table,
        rows: &[Vec<Cell>],
        widths: &[usize],
        paint: Option<Paint<'_>>,
    ) -> Vec<String> {
        let mut lines = Vec::with_capacity(rows.len() + 1);

        let headers: Vec<Cell> = table
            .columns
            .iter()
            .map(|column| Cell::new(column.header.clone()).with_token(Token::TableHeader))
            .collect();
        lines.push(self.render_row(table, &headers, widths, paint));

        for row in rows {
            lines.push(self.render_row(table, row, widths, paint));
        }
        lines
    }

    fn render_row(
        &self,
        table: &Table,
        row: &[Cell],
        widths: &[usize],
        paint: Option<Paint<'_>>,
    ) -> String {
        let mut line = String::new();
        for (index, width) in widths.iter().enumerate() {
            if index > 0 {
                line.push_str(&" ".repeat(COLUMN_GAP));
            }
            let cell = row.get(index);
            let text = cell.map_or("", Cell::text);
            let shortened = shorten(text, *width);
            let padding = width.saturating_sub(shortened.width());
            let token = cell.map_or(Token::Foreground, Cell::token);
            let shortened = self.colour_full(&shortened, token, paint);
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
    fn render_stacked(
        &self,
        table: &Table,
        rows: &[Vec<Cell>],
        paint: Option<Paint<'_>>,
    ) -> Vec<String> {
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
                let label = self.colour_full(&label, Token::TableKey, paint);
                let value = self.colour_full(&value, cell.token(), paint);
                lines.push(format!("{label}{} {value}", " ".repeat(padding)));
            }
        }
        lines
    }
}

/// How much of a level of detail an error is rendered at (spec §16.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Detail {
    /// The message, its target and its help line — what a failing command prints.
    Terse,
    /// Code, kind, metadata and the whole causal chain — what `inspect @error` shows.
    Full,
}

/// The theme and destination a layout paints with, when it paints at all.
#[derive(Debug, Clone, Copy)]
struct Paint<'a> {
    theme: &'a Theme,
    presentation: Presentation,
}

/// The rows with every value cell carrying its token's marker, where the destination has no
/// colour to carry the meaning instead (spec §44, ADR-0558).
///
/// `None` where nothing would change, so the ordinary run neither copies the rows nor allocates.
fn marked_rows(rows: &[Vec<Cell>], paint: Option<Paint<'_>>) -> Option<Vec<Vec<Cell>>> {
    let paint = paint.filter(|paint| paint.presentation.marks())?;
    Some(
        rows.iter()
            .map(|row| {
                row.iter()
                    .map(|cell| {
                        let marked =
                            paint
                                .theme
                                .mark(cell.text(), cell.token(), paint.presentation);
                        Cell::new(marked).with_token(cell.token())
                    })
                    .collect()
            })
            .collect(),
    )
}

/// A `hexdump -C`-shaped view of `bytes`: offset, sixteen bytes, and their printable form.
fn hex_dump(bytes: &[u8]) -> Vec<String> {
    if bytes.is_empty() {
        return vec![format!("{:08x}", 0)];
    }
    bytes
        .chunks(16)
        .enumerate()
        .map(|(row, chunk)| {
            let mut hex = String::with_capacity(47);
            for (index, byte) in chunk.iter().enumerate() {
                if index > 0 {
                    hex.push(' ');
                }
                let _ = std::fmt::Write::write_fmt(&mut hex, format_args!("{byte:02x}"));
            }
            let text: String = chunk
                .iter()
                .map(|byte| {
                    if byte.is_ascii_graphic() || *byte == b' ' {
                        char::from(*byte)
                    } else {
                        '.'
                    }
                })
                .collect();
            format!("{:08x}  {hex:<47}  |{text}|", row * 16)
        })
        .collect()
}

/// Shortens `text` to at most `width` terminal cells, marking that it was cut.
///
/// Never splits a character, and never returns something wider than asked for — a line that
/// escapes the terminal wraps and destroys the alignment of everything below it.
pub(crate) fn shorten(text: &str, width: usize) -> String {
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
