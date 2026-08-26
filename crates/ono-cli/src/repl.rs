//! The interactive loop.
//!
//! Everything a person sees is here: the identity line of spec §4.1, the prompt as a HUD of
//! spec §4.2, the editor, and history. The loop itself is small because every decision it needs
//! has already been made somewhere that can be tested without a terminal.

use std::io::{IsTerminal, Write};

use ono_core::{ExitStatus, Span};
use ono_editor::{Completer, Completion, Editor, Highlighter, Outcome};
use ono_history::{History, Outcome as HistoryOutcome, Policy};
use ono_render::{Presentation, Theme, Token};

use crate::invocation::Options;
use crate::report::Reporter;
use crate::resolve;
use crate::session::Session;

/// Highlighting driven by the real parser, which is what keeps what the user sees and what the
/// shell runs the same thing (spec §24.4).
struct ParserHighlighter;

impl Highlighter for ParserHighlighter {
    fn highlight(&self, line: &str) -> Vec<(Span, Token)> {
        ono_parser::tokens(line)
            .into_iter()
            .map(|token| (token.span, token_colour(token.kind)))
            .collect()
    }

    fn is_complete(&self, line: &str) -> bool {
        // ADR-0009: only `parse.incomplete` means "keep typing". A syntax error is submitted, so
        // the user sees the diagnostic instead of a prompt that will not let go.
        ono_parser::parse(line).is_complete()
    }
}

fn token_colour(kind: ono_parser::TokenKind) -> Token {
    use ono_parser::TokenKind;
    match kind {
        TokenKind::Str | TokenKind::RawStr => Token::ValueString,
        TokenKind::UnterminatedStr
        | TokenKind::UnterminatedRawStr
        | TokenKind::UnterminatedRegex => Token::Warning,
        TokenKind::Int | TokenKind::Float | TokenKind::Unit => Token::ValueNumber,
        TokenKind::Variable | TokenKind::CurrentValue => Token::PromptContext,
        TokenKind::Regex => Token::ValueUnit,
        TokenKind::Pipe | TokenKind::AndAnd | TokenKind::OrOr | TokenKind::Amp => Token::Accent,
        TokenKind::Gt | TokenKind::GtGt | TokenKind::Lt | TokenKind::GtAmp | TokenKind::LtAmp => {
            Token::Dim
        }
        _ => Token::Foreground,
    }
}

/// Completion over the names the shell can actually resolve (ADR-0011).
struct ShellCompleter {
    commands: Vec<String>,
}

impl Completer for ShellCompleter {
    fn complete(&self, line: &str, cursor: usize) -> Completion {
        let start = line[..cursor]
            .rfind(|c: char| c.is_whitespace() || c == '|')
            .map_or(0, |at| at + 1);
        let prefix = &line[start..cursor];

        // The first word is a command; anything later is a path.
        let is_head = line[..start].trim().is_empty() || line[..start].trim_end().ends_with('|');
        let candidates = if is_head {
            self.commands
                .iter()
                .filter(|name| name.starts_with(prefix))
                .cloned()
                .collect()
        } else {
            path_candidates(prefix)
        };

        Completion {
            span: Span::new(start as u32, cursor as u32),
            candidates,
        }
    }
}

fn path_candidates(prefix: &str) -> Vec<String> {
    let (directory, stem) = match prefix.rfind('/') {
        Some(at) => (&prefix[..=at], &prefix[at + 1..]),
        None => ("", prefix),
    };
    let base = if directory.is_empty() { "." } else { directory };
    let Ok(entries) = std::fs::read_dir(base) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with(stem) || (stem.is_empty() && name.starts_with('.')) {
                return None;
            }
            let suffix = if entry.path().is_dir() { "/" } else { "" };
            Some(format!("{directory}{name}{suffix}"))
        })
        .collect();
    found.sort_unstable();
    found
}

/// Runs the interactive loop until the user leaves.
pub fn run(session: &mut Session, options: &Options, reporter: &Reporter) -> ExitStatus {
    let theme = Theme::default();
    let presentation =
        Presentation::choose(std::io::stdout().is_terminal(), &environment_pairs(session));

    let mut history = open_history(session, options);
    let mut editor = Editor::new()
        .with_highlighter(ParserHighlighter)
        .with_completer(ShellCompleter {
            commands: resolve::candidates(session, ""),
        });
    editor.set_history(
        history
            .as_ref()
            .map(|history| {
                history
                    .entries()
                    .iter()
                    .map(|entry| entry.command_text().to_owned())
                    .collect()
            })
            .unwrap_or_default(),
    );

    if presentation.allows_color() || std::io::stdout().is_terminal() {
        print_identity_line(session, &theme, presentation);
    }

    // Raw mode is entered around reading and left around running, so a child program finds the
    // terminal exactly as it would have under any other shell (ADR-0013, spec §29.3).
    if ono_editor::RawMode::enter().is_err() {
        return run_from_reader(session, reporter, &mut std::io::stdin().lock());
    }

    loop {
        editor.set_prompt(prompt_of(session));
        let line = match read_line(&mut editor, &theme, presentation) {
            Some(line) => line,
            None => break,
        };
        if line.trim().is_empty() {
            continue;
        }

        let started = std::time::Instant::now();
        let status = run_source(session, &line, reporter);

        if let Some(history) = history.as_mut() {
            history.record(
                &line,
                session.cwd(),
                HistoryOutcome::new(status, started.elapsed()),
            );
            let _ = history.flush();
            editor.push_history(line.clone());
        }

        if let Some(status) = session.leaving() {
            return status;
        }
        let _ = session.executor().poll_jobs();
    }

    session.status()
}

fn read_line(editor: &mut Editor, theme: &Theme, presentation: Presentation) -> Option<String> {
    let raw = ono_editor::RawMode::enter().ok()?;
    loop {
        let width = ono_editor::terminal_size().map_or(80, |(columns, _)| columns);
        let frame = editor.frame(width, presentation, theme);
        let mut out = std::io::stdout().lock();
        let mut renderer = ono_editor::Renderer::new(&mut out);
        let _ = renderer.draw(&frame);
        drop(out);

        let key = ono_editor::read_key().ok()?;
        match editor.feed(key) {
            Outcome::Submit(line) => {
                let width = ono_editor::terminal_size().map_or(80, |(columns, _)| columns);
                let frame = editor.frame(width, presentation, theme);
                let mut out = std::io::stdout().lock();
                let mut renderer = ono_editor::Renderer::new(&mut out);
                let _ = renderer.finish(&frame);
                editor.reset();
                drop(raw);
                return Some(line);
            }
            Outcome::EndOfInput => {
                drop(raw);
                return None;
            }
            Outcome::Cancelled | Outcome::Continue | Outcome::Redraw => {}
        }
    }
}

/// The one-line identifier of spec §4.1 — printed only to a terminal, and never before a pipe.
fn print_identity_line(session: &Session, theme: &Theme, presentation: Presentation) {
    if !std::io::stdin().is_terminal() {
        return;
    }
    let link = theme.paint("local", Token::PromptLink, presentation);
    let _ = writeln!(
        std::io::stdout(),
        "{}/{}  {link}  {}/{}",
        ono_core::PRODUCT_NAME,
        ono_core::VERSION,
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    let _ = session;
}

/// The prompt as a HUD (spec §4.2): where commands will run, and where you are.
fn prompt_of(session: &mut Session) -> ono_editor::Prompt {
    let mut prompt = ono_editor::Prompt::plain("").segment("local", Token::PromptLink);
    prompt = prompt.segment("://", Token::Dim);

    let path = session.cwd().to_path_buf();
    let shown = match session.home() {
        Some(home) if path.starts_with(&home) => {
            let rest = path.strip_prefix(&home).unwrap_or(&path);
            if rest.as_os_str().is_empty() {
                "~".to_owned()
            } else {
                format!("~/{}", rest.display())
            }
        }
        _ => path.display().to_string(),
    };
    prompt = prompt.segment(shown, Token::PromptContext);

    let jobs = session.executor().jobs().len();
    if jobs > 0 {
        prompt = prompt.segment(format!(" +{jobs}"), Token::Accent);
    }
    prompt.segment(" > ", Token::Dim)
}

fn environment_pairs(session: &Session) -> Vec<(&str, &str)> {
    // `Presentation::choose` takes borrowed pairs so it stays testable; only the two names it
    // consults are worth materialising.
    let mut pairs = Vec::new();
    for name in ["NO_COLOR", "TERM"] {
        if let Some(value) = session.env_var(name)
            && let Some(value) = value.to_str()
        {
            pairs.push((name, value));
        }
    }
    pairs
}

fn open_history(session: &Session, options: &Options) -> Option<History> {
    if options.no_config {
        return None;
    }
    let path = config::history_path(session)?;
    History::open(&path, Policy::default()).ok()
}

mod config {
    use std::path::PathBuf;

    use crate::session::Session;

    pub fn history_path(session: &Session) -> Option<PathBuf> {
        crate::config::state_dir(session).map(|directory| directory.join("history.jsonl"))
    }
}

/// Parses and runs one piece of source, reporting anything that goes wrong.
pub fn run_source(session: &mut Session, source: &str, reporter: &Reporter) -> ExitStatus {
    let parsed = ono_parser::parse(source);
    if parsed.has_errors() || !parsed.is_complete() {
        for diagnostic in parsed.diagnostics() {
            reporter.diagnostic(source, diagnostic);
        }
        let status = ExitStatus::USAGE;
        session.set_status(status);
        return status;
    }

    let mut report = |error: &ono_value::ErrorValue| reporter.error(error);
    crate::eval::run_program(session, parsed.program(), source, &mut report)
}

/// Reads a whole script from a reader and runs it.
pub fn run_from_reader(
    session: &mut Session,
    reporter: &Reporter,
    reader: &mut impl std::io::Read,
) -> ExitStatus {
    let mut source = String::new();
    if let Err(error) = reader.read_to_string(&mut source) {
        reporter.error(&crate::builtin::io_error(
            std::path::Path::new("<stdin>"),
            &error,
        ));
        return ExitStatus::FAILURE;
    }
    run_source(session, &source, reporter)
}
