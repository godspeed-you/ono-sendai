//! The terminal side of a package's view (spec §31.27, §31.28; ADR-0572).
//!
//! The package submits trees of the thirteen components; this host lays them out for the
//! terminal's size, sanitises every string, paints them on the alternate screen in raw mode,
//! and forwards every key, resize and cancellation to the package. It owns every byte on the
//! terminal and the two exits — `Esc` and `Ctrl-C` — whatever the package does, and when
//! standard output is not a terminal it takes nothing, so the package emits its fallback.

use std::io::IsTerminal;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ono_editor::{AlternateScreen, KeyCode, KeyPress, RawMode, TerminalEvent};
use ono_kuang_protocol::{ViewContribution, ViewEvent, ViewSize};
use ono_kuang_supervisor::{MountedView, ViewHost};
use ono_render::{Presentation, Theme, Token};
use serde_json::Value as Json;
use tokio::sync::mpsc;

/// What draws a package's view: the shell's own terminal.
pub struct ShellViews {
    theme: Arc<Theme>,
}

impl std::fmt::Debug for ShellViews {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellViews").finish_non_exhaustive()
    }
}

impl ShellViews {
    /// A view host painting with `theme`.
    #[must_use]
    pub fn new(theme: Arc<Theme>) -> Self {
        Self { theme }
    }
}

/// What the actor asks the terminal thread to do.
enum Command {
    Draw(Json),
    Close,
}

/// A view on the terminal: the thread that owns the screen, and the channel to it.
struct TerminalView {
    commands: std::sync::mpsc::Sender<Command>,
    size: ViewSize,
    thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl MountedView for TerminalView {
    fn size(&self) -> ViewSize {
        self.size
    }

    fn submit(&self, tree: &Json) -> Result<(), String> {
        self.commands
            .send(Command::Draw(tree.clone()))
            .map_err(|_| "the terminal view has ended".to_owned())
    }

    fn close(&self) {
        let _ = self.commands.send(Command::Close);
        if let Some(thread) = lock(&self.thread).take() {
            let _ = thread.join();
        }
    }
}

impl ViewHost for ShellViews {
    fn open(
        &self,
        _package: &str,
        _view: &ViewContribution,
        events: mpsc::Sender<ViewEvent>,
    ) -> Result<Option<Box<dyn MountedView>>, String> {
        if !std::io::stdout().is_terminal() {
            return Ok(None);
        }
        // A terminal that reports no size — one behind `script(1)` on a pipe — gets the
        // classic one rather than a one-row view.
        let (columns, rows) = match ono_editor::terminal_size() {
            Ok((columns, rows)) if columns >= 4 && rows >= 2 => (columns, rows),
            _ => (80, 24),
        };
        let size = ViewSize {
            rows: u16::try_from(rows).unwrap_or(u16::MAX),
            columns: u16::try_from(columns).unwrap_or(u16::MAX),
        };
        let (commands, inbox) = std::sync::mpsc::channel();
        let (ready, taken) = std::sync::mpsc::channel();
        let theme = Arc::clone(&self.theme);
        let thread = std::thread::Builder::new()
            .name("kuang-view".to_owned())
            .spawn(move || serve(&inbox, &events, &ready, &theme, columns, rows))
            .map_err(|error| format!("no thread for the view: {error}"))?;
        match taken.recv() {
            Ok(Ok(())) => Ok(Some(Box::new(TerminalView {
                commands,
                size,
                thread: Mutex::new(Some(thread)),
            }))),
            Ok(Err(why)) => {
                let _ = thread.join();
                Err(why)
            }
            Err(_) => {
                let _ = thread.join();
                Err("the terminal thread ended before it took the screen".to_owned())
            }
        }
    }
}

/// The terminal thread: both guards, and in this order, so the screen is given back before the
/// line discipline and a terminal that dies mid-view is left cooked either way.
fn serve(
    inbox: &std::sync::mpsc::Receiver<Command>,
    events: &mpsc::Sender<ViewEvent>,
    ready: &std::sync::mpsc::Sender<Result<(), String>>,
    theme: &Theme,
    mut columns: usize,
    mut rows: usize,
) {
    let _raw = match RawMode::enter() {
        Ok(guard) => guard,
        Err(error) => {
            let _ = ready.send(Err(format!("raw mode was refused: {error}")));
            return;
        }
    };
    let _screen = match AlternateScreen::enter() {
        Ok(guard) => guard,
        Err(error) => {
            let _ = ready.send(Err(format!("the alternate screen was refused: {error}")));
            return;
        }
    };
    let _ = ready.send(Ok(()));
    let _ = ono_editor::paint(&[String::new()]);
    let mut tree: Option<Json> = None;
    loop {
        loop {
            match inbox.try_recv() {
                Ok(Command::Draw(next)) => {
                    let _ = ono_editor::paint(&render(&next, columns, rows, theme));
                    tree = Some(next);
                }
                Ok(Command::Close) | Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
            }
        }
        match ono_editor::read_event_timeout(Duration::from_millis(25)) {
            Ok(Some(TerminalEvent::Key(key))) => {
                let event = if is_cancel(key) {
                    ViewEvent {
                        kind: "cancel".to_owned(),
                        key: None,
                        size: None,
                    }
                } else if let Some(name) = key_name(key) {
                    ViewEvent {
                        kind: "key".to_owned(),
                        key: Some(name),
                        size: None,
                    }
                } else {
                    continue;
                };
                let _ = events.blocking_send(event);
            }
            Ok(Some(TerminalEvent::Resize(next_columns, next_rows))) => {
                if next_columns < 4 || next_rows < 2 {
                    continue;
                }
                columns = next_columns;
                rows = next_rows;
                ono_editor::remember_terminal_size(columns, rows);
                if let Some(tree) = &tree {
                    let _ = ono_editor::paint(&render(tree, columns, rows, theme));
                }
                let _ = events.blocking_send(ViewEvent {
                    kind: "resize".to_owned(),
                    key: None,
                    size: Some(ViewSize {
                        rows: u16::try_from(rows).unwrap_or(u16::MAX),
                        columns: u16::try_from(columns).unwrap_or(u16::MAX),
                    }),
                });
            }
            Ok(None) => {}
            Err(_) => {
                let _ = events.blocking_send(ViewEvent {
                    kind: "cancel".to_owned(),
                    key: None,
                    size: None,
                });
            }
        }
    }
}

/// The host's exits (spec §31.27): the package hears `cancel`, never the key.
fn is_cancel(key: KeyPress) -> bool {
    matches!(key.code(), KeyCode::Esc)
        || (key.modifiers().has_ctrl() && matches!(key.code(), KeyCode::Char('c')))
}

/// The key as `view.event` names it.
fn key_name(key: KeyPress) -> Option<String> {
    let base = match key.code() {
        KeyCode::Char(character) => character.to_string(),
        KeyCode::Enter => "enter".to_owned(),
        KeyCode::Tab => "tab".to_owned(),
        KeyCode::BackTab => "backtab".to_owned(),
        KeyCode::Backspace => "backspace".to_owned(),
        KeyCode::Delete => "delete".to_owned(),
        KeyCode::Insert => "insert".to_owned(),
        KeyCode::Left => "left".to_owned(),
        KeyCode::Right => "right".to_owned(),
        KeyCode::Up => "up".to_owned(),
        KeyCode::Down => "down".to_owned(),
        KeyCode::Home => "home".to_owned(),
        KeyCode::End => "end".to_owned(),
        KeyCode::PageUp => "pageup".to_owned(),
        KeyCode::PageDown => "pagedown".to_owned(),
        KeyCode::Esc => return None,
    };
    let modifiers = key.modifiers();
    Some(if modifiers.has_ctrl() {
        format!("ctrl-{base}")
    } else if modifiers.has_alt() {
        format!("alt-{base}")
    } else {
        base
    })
}

// --- layout ------------------------------------------------------------------------------------

/// Lays a validated tree out for a terminal `columns` wide and `rows` high.
#[must_use]
pub fn render(tree: &Json, columns: usize, rows: usize, theme: &Theme) -> Vec<String> {
    let columns = columns.max(4);
    let rows = rows.max(1);
    let mut lines = component(tree, columns, rows, theme);
    lines.truncate(rows);
    lines
}

fn component(node: &Json, width: usize, height: usize, theme: &Theme) -> Vec<String> {
    match node.get("component").and_then(Json::as_str).unwrap_or("") {
        "Text" => wrap(&clean(&text(node, "text")), width),
        "Table" => table(node, width, theme),
        "Tree" => tree(node, width),
        "Graph" => graph(node, width),
        "KeyValue" => key_value(node, width, theme),
        "LogStream" => log_stream(node, width, height),
        "Sparkline" => sparkline(node, width),
        "Gauge" => gauge(node, width, theme),
        "Tabs" => tabs(node, width, height, theme),
        "Split" => split(node, width, height, theme),
        "CommandPalette" => palette(node, width, height, theme),
        "ObjectPicker" => picker(node, width, height, theme),
        "StatusLine" => vec![theme.paint(
            &pad(&fit(&clean(&text(node, "text")), width), width),
            Token::Selection,
            Presentation::Terminal,
        )],
        _ => Vec::new(),
    }
}

/// Every string a package sends is cleaned of control characters before it reaches the
/// terminal (ADR-0015 T1, T2, T9): no escape sequence, no line break, no tab.
fn clean(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn text(node: &Json, field: &str) -> String {
    match node.get(field) {
        Some(Json::String(text)) => text.clone(),
        Some(Json::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

fn fit(line: &str, width: usize) -> String {
    let count = line.chars().count();
    if count <= width {
        return line.to_owned();
    }
    let mut cut: String = line.chars().take(width.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

fn pad(line: &str, width: usize) -> String {
    let count = line.chars().count();
    let mut padded = line.to_owned();
    padded.extend(std::iter::repeat_n(' ', width.saturating_sub(count)));
    padded
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let next = if line.is_empty() {
                word.to_owned()
            } else {
                format!("{line} {word}")
            };
            if next.chars().count() > width && !line.is_empty() {
                lines.push(line);
                line = word.to_owned();
            } else {
                line = next;
            }
        }
        lines.push(fit(&line, width));
    }
    lines
}

fn strings(node: &Json, field: &str) -> Vec<String> {
    node.get(field)
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .map(|item| match item {
            Json::String(text) => clean(text),
            Json::Object(object) => clean(
                &object
                    .get("label")
                    .or_else(|| object.get("text"))
                    .or_else(|| object.get("id"))
                    .map(|value| match value {
                        Json::String(text) => text.clone(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default(),
            ),
            other => clean(&other.to_string()),
        })
        .collect()
}

fn cell_text(value: &Json) -> String {
    match value {
        Json::String(text) => clean(text),
        Json::Null => String::new(),
        other => clean(&other.to_string()),
    }
}

fn table(node: &Json, width: usize, theme: &Theme) -> Vec<String> {
    let rows_json: Vec<&Json> = node
        .get("rows")
        .and_then(Json::as_array)
        .map(|rows| rows.iter().collect())
        .unwrap_or_default();
    let mut headers: Vec<String> = strings(node, "columns");
    if headers.is_empty()
        && let Some(Json::Object(first)) = rows_json.first()
    {
        headers = first.keys().cloned().collect();
    }
    if headers.is_empty() {
        return vec!["(no columns)".to_owned()];
    }
    let mut table = ono_render::Table::new(
        headers
            .iter()
            .map(|header| ono_render::Column::new(header.clone()))
            .collect(),
    );
    for row in &rows_json {
        let cells: Vec<ono_render::Cell> = match row {
            Json::Array(values) => headers
                .iter()
                .enumerate()
                .map(|(at, _)| {
                    ono_render::Cell::new(values.get(at).map(cell_text).unwrap_or_default())
                })
                .collect(),
            Json::Object(object) => headers
                .iter()
                .map(|header| {
                    ono_render::Cell::new(object.get(header).map(cell_text).unwrap_or_default())
                })
                .collect(),
            other => std::iter::once(ono_render::Cell::new(cell_text(other)))
                .chain(headers.iter().skip(1).map(|_| ono_render::Cell::new("")))
                .collect(),
        };
        table.push_row(cells);
    }
    let mut lines = ono_render::Layout::new(width).render(&table);
    if let Some(selected) = node.get("selected").and_then(Json::as_u64) {
        let header_lines = lines.len().saturating_sub(rows_json.len());
        let at = header_lines + usize::try_from(selected).unwrap_or(usize::MAX);
        if let Some(line) = lines.get_mut(at) {
            *line = theme.paint(&pad(line, width), Token::Selection, Presentation::Terminal);
        }
    }
    lines.into_iter().map(|line| fit(&line, width)).collect()
}

fn tree(node: &Json, width: usize) -> Vec<String> {
    fn walk(node: &Json, prefix: &str, last: bool, root: bool, out: &mut Vec<String>) {
        let label = clean(
            &node
                .get("label")
                .or_else(|| node.get("text"))
                .map(|value| match value {
                    Json::String(text) => text.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_default(),
        );
        let line = if root {
            label
        } else {
            format!("{prefix}{} {label}", if last { "└─" } else { "├─" })
        };
        out.push(line);
        let children: Vec<&Json> = node
            .get("children")
            .and_then(Json::as_array)
            .map(|children| children.iter().collect())
            .unwrap_or_default();
        let next_prefix = if root {
            String::new()
        } else {
            format!("{prefix}{}", if last { "   " } else { "│  " })
        };
        let count = children.len();
        for (at, child) in children.into_iter().enumerate() {
            walk(child, &next_prefix, at + 1 == count, false, out);
        }
    }
    let mut out = Vec::new();
    match node.get("root") {
        Some(root) => walk(root, "", true, true, &mut out),
        None => {
            let children: Vec<&Json> = node
                .get("children")
                .and_then(Json::as_array)
                .map(|children| children.iter().collect())
                .unwrap_or_default();
            let count = children.len();
            for (at, child) in children.into_iter().enumerate() {
                walk(child, "", at + 1 == count, false, &mut out);
            }
        }
    }
    out.into_iter().map(|line| fit(&line, width)).collect()
}

fn graph(node: &Json, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = strings(node, "nodes")
        .into_iter()
        .map(|label| format!("● {label}"))
        .collect();
    for edge in node
        .get("edges")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
    {
        let from = clean(&text(edge, "from"));
        let to = clean(&text(edge, "to"));
        let label = clean(&text(edge, "label"));
        lines.push(if label.is_empty() {
            format!("{from} ─▶ {to}")
        } else {
            format!("{from} ─{label}─▶ {to}")
        });
    }
    lines.into_iter().map(|line| fit(&line, width)).collect()
}

fn key_value(node: &Json, width: usize, theme: &Theme) -> Vec<String> {
    let mut lines = Vec::new();
    let title = clean(&text(node, "title"));
    if !title.is_empty() {
        lines.push(theme.paint(
            &fit(&title, width),
            Token::TableHeader,
            Presentation::Terminal,
        ));
    }
    let pairs: Vec<(String, String)> = match node.get("pairs") {
        Some(Json::Array(pairs)) => pairs
            .iter()
            .map(|pair| match pair {
                Json::Array(both) => (
                    both.first().map(cell_text).unwrap_or_default(),
                    both.get(1).map(cell_text).unwrap_or_default(),
                ),
                Json::Object(object) => (
                    object.get("key").map(cell_text).unwrap_or_default(),
                    object.get("value").map(cell_text).unwrap_or_default(),
                ),
                other => (cell_text(other), String::new()),
            })
            .collect(),
        Some(Json::Object(object)) => object
            .iter()
            .map(|(key, value)| (clean(key), cell_text(value)))
            .collect(),
        _ => Vec::new(),
    };
    let key_width = pairs
        .iter()
        .map(|(key, _)| key.chars().count())
        .max()
        .unwrap_or(0);
    for (key, value) in pairs {
        let painted = theme.paint(
            &pad(&key, key_width),
            Token::TableKey,
            Presentation::Terminal,
        );
        lines.push(format!(
            "{painted}  {}",
            fit(&value, width.saturating_sub(key_width + 2))
        ));
    }
    lines
}

fn log_stream(node: &Json, width: usize, height: usize) -> Vec<String> {
    let lines = strings(node, "lines");
    let skip = lines.len().saturating_sub(height);
    lines
        .into_iter()
        .skip(skip)
        .map(|line| fit(&line, width))
        .collect()
}

fn sparkline(node: &Json, width: usize) -> Vec<String> {
    const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let values: Vec<f64> = node
        .get("values")
        .and_then(Json::as_array)
        .into_iter()
        .flatten()
        .filter_map(Json::as_f64)
        .collect();
    let title = clean(&text(node, "title"));
    let room = width.saturating_sub(if title.is_empty() {
        0
    } else {
        title.chars().count() + 1
    });
    let skip = values.len().saturating_sub(room);
    let shown = &values[skip..];
    let top = shown.iter().copied().fold(f64::MIN, f64::max);
    let bottom = shown.iter().copied().fold(f64::MAX, f64::min);
    let span = if top > bottom { top - bottom } else { 1.0 };
    let bars: String = shown
        .iter()
        .map(|value| {
            let level = ((value - bottom) / span * 7.0).round();
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "rounded and clamped to the eight block characters"
            )]
            let at = level.clamp(0.0, 7.0) as usize;
            BLOCKS[at]
        })
        .collect();
    vec![if title.is_empty() {
        bars
    } else {
        format!("{title} {bars}")
    }]
}

fn gauge(node: &Json, width: usize, theme: &Theme) -> Vec<String> {
    let value = node.get("value").and_then(Json::as_f64).unwrap_or(0.0);
    let max = node
        .get("max")
        .and_then(Json::as_f64)
        .filter(|max| *max > 0.0)
        .unwrap_or(100.0);
    let ratio = (value / max).clamp(0.0, 1.0);
    let label = clean(&text(node, "label"));
    let bar_width = width.saturating_sub(label.chars().count() + 8).max(4);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a ratio in [0, 1] times a small width"
    )]
    let filled = (ratio * bar_width as f64).round() as usize;
    let bar = format!(
        "{}{}",
        "█".repeat(filled),
        "░".repeat(bar_width.saturating_sub(filled))
    );
    let token = if ratio >= 0.9 {
        Token::Danger
    } else if ratio >= 0.7 {
        Token::Warning
    } else {
        Token::Success
    };
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a percentage"
    )]
    let percent = (ratio * 100.0).round() as u64;
    vec![format!(
        "{label}{}{} {percent:>3}%",
        if label.is_empty() { "" } else { " " },
        theme.paint(&bar, token, Presentation::Terminal)
    )]
}

fn tabs(node: &Json, width: usize, height: usize, theme: &Theme) -> Vec<String> {
    let tabs: Vec<&Json> = node
        .get("tabs")
        .and_then(Json::as_array)
        .map(|tabs| tabs.iter().collect())
        .unwrap_or_default();
    let active = node
        .get("active")
        .and_then(Json::as_u64)
        .and_then(|at| usize::try_from(at).ok())
        .unwrap_or(0)
        .min(tabs.len().saturating_sub(1));
    let bar: Vec<String> = tabs
        .iter()
        .enumerate()
        .map(|(at, tab)| {
            let title = clean(&text(tab, "title"));
            if at == active {
                theme.paint(
                    &format!(" {title} "),
                    Token::Selection,
                    Presentation::Terminal,
                )
            } else {
                theme.paint(&format!(" {title} "), Token::Dim, Presentation::Terminal)
            }
        })
        .collect();
    let mut lines = vec![bar.join("")];
    if let Some(body) = tabs.get(active).and_then(|tab| tab.get("body")) {
        lines.extend(component(body, width, height.saturating_sub(1), theme));
    }
    lines
}

fn split(node: &Json, width: usize, height: usize, theme: &Theme) -> Vec<String> {
    let panes: Vec<&Json> = node
        .get("panes")
        .and_then(Json::as_array)
        .map(|panes| panes.iter().collect())
        .unwrap_or_default();
    if panes.is_empty() {
        return Vec::new();
    }
    if text(node, "direction") == "horizontal" {
        let count = panes.len();
        let each = width.saturating_sub(count - 1) / count;
        let columns: Vec<Vec<String>> = panes
            .iter()
            .map(|pane| component(pane, each.max(1), height, theme))
            .collect();
        let tallest = columns.iter().map(Vec::len).max().unwrap_or(0).min(height);
        let border = theme.paint("│", Token::Border, Presentation::Terminal);
        return (0..tallest)
            .map(|row| {
                columns
                    .iter()
                    .map(|column| pad(&fit(column.get(row).map_or("", String::as_str), each), each))
                    .collect::<Vec<_>>()
                    .join(&border)
            })
            .collect();
    }
    // Vertical: the panes stack, and a trailing status line sits on the last row.
    let status_last = panes
        .last()
        .and_then(|pane| pane.get("component"))
        .and_then(Json::as_str)
        == Some("StatusLine");
    let body_height = if status_last {
        height.saturating_sub(1)
    } else {
        height
    };
    let mut lines = Vec::new();
    let body_panes = if status_last {
        &panes[..panes.len() - 1]
    } else {
        &panes[..]
    };
    for pane in body_panes {
        let room = body_height.saturating_sub(lines.len());
        if room == 0 {
            break;
        }
        let mut rendered = component(pane, width, room, theme);
        rendered.truncate(room);
        lines.extend(rendered);
    }
    if status_last {
        while lines.len() < body_height {
            lines.push(String::new());
        }
        if let Some(status) = panes.last() {
            lines.extend(component(status, width, 1, theme));
        }
    }
    lines
}

fn palette(node: &Json, width: usize, height: usize, theme: &Theme) -> Vec<String> {
    let query = clean(&text(node, "query"));
    let mut lines = vec![theme.paint(
        &fit(&format!("> {query}"), width),
        Token::Accent,
        Presentation::Terminal,
    )];
    lines.extend(list(node, "items", width, height.saturating_sub(1), theme));
    lines
}

fn picker(node: &Json, width: usize, height: usize, theme: &Theme) -> Vec<String> {
    let title = clean(&text(node, "title"));
    let mut lines = Vec::new();
    if !title.is_empty() {
        lines.push(theme.paint(
            &fit(&title, width),
            Token::TableHeader,
            Presentation::Terminal,
        ));
    }
    let room = height.saturating_sub(lines.len());
    lines.extend(list(node, "items", width, room, theme));
    lines
}

fn list(node: &Json, field: &str, width: usize, height: usize, theme: &Theme) -> Vec<String> {
    let items = strings(node, field);
    let selected = node
        .get("selected")
        .and_then(Json::as_u64)
        .and_then(|at| usize::try_from(at).ok());
    let first = selected
        .map(|at| at.saturating_sub(height.saturating_sub(1)))
        .unwrap_or(0);
    items
        .into_iter()
        .enumerate()
        .skip(first)
        .take(height)
        .map(|(at, item)| {
            if selected == Some(at) {
                theme.paint(
                    &pad(&fit(&format!("▶ {item}"), width), width),
                    Token::Selection,
                    Presentation::Terminal,
                )
            } else {
                fit(&format!("  {item}"), width)
            }
        })
        .collect()
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plain() -> Theme {
        Theme::default()
    }

    #[test]
    fn should_strip_control_characters_from_everything_the_package_sends() {
        let lines = render(
            &json!({"component": "Text", "text": "safe\x1b[31m text\x07"}),
            40,
            5,
            &plain(),
        );
        assert_eq!(lines, ["safe [31m text"]);
    }

    #[test]
    fn should_pin_a_trailing_status_line_to_the_last_row() {
        let lines = render(
            &json!({"component": "Split", "direction": "vertical", "panes": [
                {"component": "Text", "text": "body"},
                {"component": "StatusLine", "text": "status"},
            ]}),
            20,
            5,
            &plain(),
        );
        assert_eq!(lines.len(), 5);
        assert_eq!(lines[0], "body");
        assert!(lines[4].contains("status"));
    }

    #[test]
    fn should_never_exceed_the_terminal() {
        let lines = render(
            &json!({"component": "LogStream", "lines": (0..50).map(|at| format!("line {at} {}", "x".repeat(100))).collect::<Vec<_>>()}),
            30,
            10,
            &plain(),
        );
        assert_eq!(lines.len(), 10);
        assert!(lines.iter().all(|line| line.chars().count() <= 30));
        assert!(
            lines[9].starts_with("line 49"),
            "the newest lines are shown"
        );
    }

    #[test]
    fn should_lay_out_every_component() {
        for tree in [
            json!({"component": "Table", "columns": ["a"], "rows": [["1"]], "selected": 0}),
            json!({"component": "Tree", "root": {"label": "r", "children": [{"label": "c"}]}}),
            json!({"component": "Graph", "nodes": ["a", "b"], "edges": [{"from": "a", "to": "b"}]}),
            json!({"component": "KeyValue", "pairs": [["k", "v"]]}),
            json!({"component": "Sparkline", "values": [1, 5, 3]}),
            json!({"component": "Gauge", "value": 42, "label": "cpu"}),
            json!({"component": "Tabs", "tabs": [{"title": "one", "body": {"component": "Text", "text": "1"}}]}),
            json!({"component": "Split", "direction": "horizontal", "panes": [{"component": "Text", "text": "l"}, {"component": "Text", "text": "r"}]}),
            json!({"component": "CommandPalette", "query": "q", "items": ["one"], "selected": 0}),
            json!({"component": "ObjectPicker", "items": [{"label": "x"}], "selected": 0}),
        ] {
            let lines = render(&tree, 40, 10, &plain());
            assert!(!lines.is_empty(), "{tree}");
        }
    }
}
