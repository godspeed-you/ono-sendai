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
        TokenKind::Regex | TokenKind::Ip => Token::ValueUnit,
        TokenKind::Pipe | TokenKind::AndAnd | TokenKind::OrOr | TokenKind::Amp => Token::Accent,
        TokenKind::Gt | TokenKind::GtGt | TokenKind::Lt | TokenKind::GtAmp | TokenKind::LtAmp => {
            Token::Dim
        }
        _ => Token::Foreground,
    }
}

/// Completion over what the shell can actually resolve.
///
/// Three sources, in the order a user would expect them: the command registry, which knows every
/// verb, target and option and their documentation (spec §15.1, §27.1); the executables and
/// builtins a name could resolve to (ADR-0011); and the filesystem. The registry is consulted
/// first because it is the only one that can offer a *target* or an *option*, and spec §34
/// budgets 50 ms for the first results from local metadata — which is a lookup, not a search.
struct ShellCompleter {
    commands: Vec<String>,
    /// What only a provider can complete: the users on this machine, the services of this host
    /// (spec §15.1, ADR-0252). `None` where no provider should be asked at all.
    values: Option<crate::complete::ProviderValues>,
    /// The adapter registry, so completion after an adapted program knows the schema its
    /// records have, and before the pipe offers only the flags the adapter declares (spec v0.3
    /// §1.59): what the contracts say, and nothing invented.
    adapters: Option<std::sync::Arc<ono_adapter::Registry>>,
    resolver: Option<ono_command::Resolver>,
}

/// Completes a selector from whichever source can answer it.
///
/// An expression-mode selector — `where <field>`, `select <field>` — names a field of the schema
/// flowing into the stage, which only the contracts know. A words-mode selector names an object,
/// which only a provider knows (spec §15.1). One hook, two questions, and the command's own
/// argument mode says which is being asked.
struct SelectorCompleter {
    fields: Vec<String>,
    values: Option<crate::complete::ProviderValues>,
}

impl ono_command::ValueCompleter for SelectorCompleter {
    fn complete(
        &self,
        command: &ono_command::CommandContract,
        parameter: &ono_command::ParameterSpec,
        prefix: &str,
    ) -> Vec<ono_command::Candidate> {
        if command.argument_mode() == ono_command::ArgumentMode::Expression {
            return self
                .fields
                .iter()
                .filter(|field| field.starts_with(prefix))
                .map(ono_command::Candidate::value)
                .collect();
        }
        self.values
            .as_ref()
            .map(|values| ono_command::ValueCompleter::complete(values, command, parameter, prefix))
            .unwrap_or_default()
    }
}

impl ShellCompleter {
    /// The schema flowing out of the stages before the one under the cursor, planned the way
    /// the pipeline would be — so an adapted `ps aux |` answers with Process fields exactly as
    /// `get process |` does (spec v0.3 §1.59, §1.61).
    fn upstream_fields(&self, line: &str, cursor: usize) -> Vec<String> {
        let typed = &line[..cursor.min(line.len())];
        let Some(cut) = typed.rfind('|') else {
            return Vec::new();
        };
        let upstream = typed[..cut].trim();
        if upstream.is_empty() || upstream.ends_with([';', '&']) {
            return Vec::new();
        }
        let Ok(registry) = ono_command::CommandRegistry::embedded() else {
            return Vec::new();
        };
        // Planned with a structured consumer after it, because that is what the stage under
        // the cursor is about to be: alone, a program at the end of a line is raw bytes.
        let upstream = format!("{upstream} | count");
        let parsed = ono_parser::parse(&upstream);
        let Some(pipeline) = parsed
            .program()
            .statements
            .first()
            .and_then(ono_parser::Statement::as_pipeline)
        else {
            return Vec::new();
        };
        let resolver = self.resolver.clone();
        let executables = |name: &str| resolver.as_ref().and_then(|resolve| resolve(name));
        let plan = ono_command::plan_with(
            registry,
            None,
            pipeline,
            &upstream,
            &ono_command::PlanContext {
                stdout: ono_adapter::Stdout::Stream,
                adapters: self.adapters.as_deref(),
                executables: Some(&executables),
            },
        );
        let stages = plan.stages();
        stages
            .len()
            .checked_sub(2)
            .and_then(|producer| stages.get(producer))
            .and_then(ono_command::StagePlan::element_schema)
            .and_then(|id| id.parse::<ono_value::SchemaId>().ok())
            .and_then(|id| ono_value::builtin_schemas().get(&id))
            .map(|schema| {
                schema
                    .fields()
                    .iter()
                    .map(|field| field.name().to_owned())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The flags an adapter declares for the program at the head of the stage under the
    /// cursor; empty when nothing adapts it, so ordinary completion applies.
    fn declared_flags(&self, line: &str, start: usize) -> Vec<String> {
        let Some(adapters) = self.adapters.as_deref() else {
            return Vec::new();
        };
        let stage = &line[..start];
        let stage = &stage[stage.rfind(['|', ';', '&']).map_or(0, |at| at + 1)..];
        let mut words = stage.split_whitespace();
        let head = match words.next() {
            Some("raw" | "adapt") => words.next(),
            head => head,
        };
        head.map(|program| adapters.declared_flags(program))
            .unwrap_or_default()
    }
}

impl Completer for ShellCompleter {
    fn complete(&self, line: &str, cursor: usize) -> Completion {
        let start = line[..cursor]
            .rfind(|c: char| c.is_whitespace() || c == '|')
            .map_or(0, |at| at + 1);
        let prefix = &line[start..cursor];
        let is_head = line[..start].trim().is_empty() || line[..start].trim_end().ends_with('|');

        // §9.4: after a spatial verb, completion is a lightweight local map — the places or
        // relations the session can see from where it stands, offered at once and *before* the
        // broader matches the registry and the filesystem know about.
        let neighbourhood = if is_head || prefix.starts_with('-') {
            Vec::new()
        } else {
            spatial_offers(line, start, prefix)
        };

        let mut candidates: Vec<String> = Vec::new();

        if let Ok(registry) = ono_command::CommandRegistry::embedded() {
            let context = ono_command::StageContext::from_line(line, cursor);
            let fields = SelectorCompleter {
                fields: if is_head {
                    Vec::new()
                } else {
                    self.upstream_fields(line, cursor)
                },
                values: if is_head { None } else { self.values.clone() },
            };
            candidates.extend(
                ono_command::complete(registry, &context, Some(&fields))
                    .into_iter()
                    .map(|candidate| candidate.text().to_owned()),
            );
        }

        if is_head {
            candidates.extend(
                self.commands
                    .iter()
                    .filter(|name| name.starts_with(prefix))
                    .cloned(),
            );
        } else if prefix.starts_with('-') {
            // An adapter's declared invocations are the only flags it can vouch for (spec v0.3
            // §1.59); an undeclared flag is not offered, and not refused either.
            candidates.extend(
                self.declared_flags(line, start)
                    .into_iter()
                    .filter(|flag| flag.starts_with(prefix)),
            );
        } else {
            // An option is the registry's business; a path is the filesystem's.
            candidates.extend(path_candidates(prefix));
        }

        candidates.sort_unstable();
        candidates.dedup();

        let span = Span::new(start as u32, cursor as u32);
        if neighbourhood.is_empty() {
            return Completion::new(span, candidates);
        }

        // §9.4: "prioritize services visible in the current neighborhood and then offer broader
        // matches" — in that order, and shown, because the point is to teach the neighbourhood.
        let mut listing: Vec<String> = neighbourhood
            .iter()
            .map(|offer| offer.line.clone())
            .collect();
        let mut merged: Vec<String> = neighbourhood
            .into_iter()
            .map(|offer| offer.insert)
            .collect();
        for candidate in candidates {
            if !merged.contains(&candidate) {
                listing.push(format!("  {candidate}"));
                merged.push(candidate);
            }
        }
        Completion::new(span, merged).shown(listing)
    }
}

/// The neighbourhood a spatial verb is asking about, where the word under the cursor is the one
/// that names a place or a relation (spec v0.4 §9.4).
///
/// Only the verb's *first* word is answered this way: `enter` takes one place and `follow` takes
/// one relation, and a second word is a selector inside that relation, which is another question.
/// The answer is prepended to the ordinary candidates rather than replacing them, because
/// `enter service nginx` is still the v0.2 spelling and its targets are still offered — §9.4 asks
/// for the neighbourhood *first*, not alone.
fn spatial_offers(line: &str, start: usize, prefix: &str) -> Vec<crate::spatial::complete::Offer> {
    let stage = &line[..start];
    let stage = &stage[stage.rfind(['|', ';', '&']).map_or(0, |at| at + 1)..];
    let offers = match stage.trim() {
        // §6.3, §6.5, §6.9: all three take a place, and the places are the same ones.
        "enter" | "jump" | "map" => crate::spatial::complete::places_here(),
        // §6.4, §9.4's second half: the relations this place actually has.
        "follow" => crate::spatial::complete::relations_here(),
        _ => return Vec::new(),
    };
    let prefix = prefix.to_lowercase();
    offers
        .into_iter()
        .filter(|offer| offer.insert.to_lowercase().starts_with(&prefix))
        .collect()
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
            values: Some(crate::complete::ProviderValues::new(
                session
                    .env()
                    .iter()
                    .map(|(name, value)| {
                        (
                            name.to_string_lossy().into_owned(),
                            value.to_string_lossy().into_owned(),
                        )
                    })
                    .collect(),
            )),
            adapters: Some(session.shared_adapters()),
            resolver: Some(resolve::resolver(session)),
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

    // From here on this process is an interactive session: a picker may open, and a map may take
    // the screen (spec v0.4 §29.1, §29.3).
    crate::spatial::mark_interactive();
    if std::io::stdin().is_terminal() {
        print_startup_horizon(session, reporter);
    }

    // The shell ignores the signals a terminal generates for the foreground job, so Ctrl-C
    // reaches the running command and leaves the prompt standing rather than killing the shell
    // out from under the user (spec §18.1). Children have these reset before `exec`, so a
    // program still sees a normal signal environment.
    if let Err(error) = ono_process::install_shell_signals() {
        reporter.error(&ono_value::ErrorValue::new(
            error.code(),
            error.message().to_owned(),
        ));
    }
    if let Err(error) = ono_process::install_child_watch() {
        reporter.error(&ono_value::ErrorValue::new(
            error.code(),
            error.message().to_owned(),
        ));
    }

    // The renderer is stateful: it remembers how tall the last frame was so it can paint over it.
    // A fresh one per keystroke would leave every previous frame on the screen.
    let mut renderer = ono_editor::Renderer::new(std::io::stdout());

    loop {
        editor.set_prompt(prompt_of(session));
        let line = match read_line(&mut editor, &mut renderer, &theme, presentation) {
            Some(line) => line,
            None => break,
        };
        if line.trim().is_empty() {
            continue;
        }

        let started = std::time::Instant::now();
        let _ = session.take_adaptations();
        let status = run_source(session, &line, reporter);
        let (adapters, plans): (Vec<String>, Vec<String>) =
            session.take_adaptations().into_iter().unzip();

        if let Some(history) = history.as_mut() {
            history.record(
                &line,
                session.cwd(),
                HistoryOutcome::new(status, started.elapsed()).adapted_by(adapters, plans),
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

/// Reads one line, in raw mode.
///
/// Raw mode is entered around reading and left around running, so a child program finds the
/// terminal exactly as it would have under any other shell (ADR-0013, spec §29.3).
fn read_line(
    editor: &mut Editor,
    renderer: &mut ono_editor::Renderer<std::io::Stdout>,
    theme: &Theme,
    presentation: Presentation,
) -> Option<String> {
    let raw = ono_editor::RawMode::enter().ok()?;

    loop {
        let frame = editor.frame(terminal_width(), presentation, theme);
        let _ = renderer.draw(&frame);

        let key = ono_editor::read_key().ok()?;
        match editor.feed(key) {
            Outcome::Submit(line) => {
                let frame = editor.frame(terminal_width(), presentation, theme);
                let _ = renderer.finish(&frame);
                editor.reset();
                drop(raw);
                return Some(line);
            }
            Outcome::EndOfInput => {
                let frame = editor.frame(terminal_width(), presentation, theme);
                let _ = renderer.finish(&frame);
                drop(raw);
                return None;
            }
            Outcome::Cancelled | Outcome::Continue | Outcome::Redraw => {}
        }
    }
}

/// The path as a prompt should show it.
///
/// Spec §4.2 asks that the prompt stay short, and a prompt wider than the terminal wraps and
/// takes the line the user is typing with it. Home becomes `~`, and once the path is long the
/// leading components shrink to their first character — the last component, which is the one
/// that tells you where you are, always stays whole.
fn short_path(path: &std::path::Path, home: Option<&std::path::Path>) -> String {
    const BUDGET: usize = 32;

    let mut shown = path.display().to_string();
    if let Some(home) = home
        && let Ok(rest) = path.strip_prefix(home)
    {
        shown = if rest.as_os_str().is_empty() {
            "~".to_owned()
        } else {
            format!("~/{}", rest.display())
        };
    }
    if shown.chars().count() <= BUDGET {
        return shown;
    }

    let absolute = shown.starts_with('/');
    let mut parts: Vec<String> = shown
        .split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect();
    if parts.len() < 2 {
        return shown;
    }

    // Shrink from the left, one component at a time, and stop as soon as it fits. The last
    // component is the one that says where you are, so it is never touched.
    let render = |parts: &[String]| {
        let joined = parts.join("/");
        if absolute {
            format!("/{joined}")
        } else {
            joined
        }
    };
    for index in 0..parts.len() - 1 {
        parts[index] = match parts[index].chars().next() {
            // A leading dot is part of the name, not decoration, so `.config` keeps two.
            Some('.') => parts[index].chars().take(2).collect(),
            Some(first) => first.to_string(),
            None => String::new(),
        };
        if render(&parts).chars().count() <= BUDGET {
            break;
        }
    }
    render(&parts)
}

/// How wide the terminal is, with a usable answer when nobody can say.
///
/// A pseudo-terminal opened without a window size reports zero columns, and a zero-wide terminal
/// wraps every single character onto its own line. Anything implausibly narrow is treated as
/// unknown, and `COLUMNS` is honoured because that is how a caller says what the size is when the
/// terminal itself cannot.
fn terminal_width() -> usize {
    const NARROWEST_USABLE: usize = 20;
    const ASSUMED: usize = 80;

    let reported = ono_editor::terminal_size().map_or(0, |(columns, _)| columns);
    if reported >= NARROWEST_USABLE {
        return reported;
    }
    std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|columns| *columns >= NARROWEST_USABLE)
        .unwrap_or(ASSUMED)
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
    // Spec §14.4: the active link frame determines where provider calls and processes run, and
    // the prompt MUST make that unambiguous — the host takes `local`'s place entirely.
    // v0.4 §19.2/§21.1: standing on a linked host is the same fact about where the next command
    // operates, whether `enter link` or `jump` put the session there, and §21.3 requires it to be
    // recognisable without colour — so the host takes `local`'s place in the text itself.
    let location = session
        .frames()
        .iter()
        .rev()
        .find_map(|frame| {
            matches!(frame.frame.kind(), ono_command::FrameKind::Link)
                .then(|| frame.frame.identity().to_string())
        })
        .or_else(|| {
            ono_spatial_core::space::standing_in().map(|scope| scope.host_scope().id().to_owned())
        })
        .unwrap_or_else(|| "local".to_owned());
    let mut prompt = ono_editor::Prompt::plain("").segment(location, Token::PromptLink);

    // Spec v0.4 §21.1: the current spatial place is a semantic component of the prompt beside the
    // link, and §21.2 keeps it to `<host>/<place-kind>/<display-name>` so a deep traversal never
    // takes the line the user is typing with it. The link segment above is the path's first
    // segment, so only what the place adds to it is painted here.
    if let Some(place) = spatial_place(session) {
        prompt = prompt.segment(place, Token::PromptContext);
    }

    // Spec §17.2: an elevated context must be impossible to miss. The kernel's answer, not
    // `$USER`'s — and painted in the token the theme reserves for exactly this.
    if ono_process::effective_uid() == 0 {
        prompt = prompt.segment(" root", Token::PromptRoot);
    }
    prompt = prompt.segment("://", Token::Dim);

    // Spec §14.3: inside an object context the prompt names the object — `local://service/nginx`
    // — because a frame that changes what commands act on must be impossible to miss
    // (ADR-0023). The working directory returns to the prompt when the frame is left.
    let object = session.frames().iter().rev().find_map(|entry| {
        matches!(entry.frame.kind(), ono_command::FrameKind::Object)
            .then(|| format!("{}/{}", entry.frame.target(), entry.frame.identity()))
    });
    match object {
        Some(entered) => {
            prompt = prompt.segment(entered, Token::PromptContext);
        }
        None => {
            prompt = prompt.segment(
                short_path(session.cwd(), session.home().as_deref()),
                Token::PromptContext,
            );
        }
    }

    // Spec §4.2's optional `vcs` segment: `git:main` when the working directory is inside a
    // checkout, and nothing at all when it is not (ADR-0250).
    if let Some(branch) = vcs_segment(session) {
        prompt = prompt.segment(format!(" {branch}"), Token::Dim);
    }

    let jobs = session.executor().jobs().len();
    if jobs > 0 {
        prompt = prompt.segment(format!(" +{jobs}"), Token::Accent);
    }
    let marker = if ono_process::effective_uid() == 0 {
        " # "
    } else {
        " > "
    };
    prompt.segment(marker, Token::Dim)
}

/// The source-control segment of spec §4.2, or nothing when there is none to show.
///
/// The segment is read from the repository's own files rather than from `git`: a prompt drawn
/// before every line must not fork a process, and spec §34 budgets the prompt. The branch is
/// what `.git/HEAD` says, which is the one fact that is both cheap and always true; the
/// specification's `*` for a dirty tree is deliberately not shown (ADR-0250). Switched off by
/// `prompt.vcs`.
fn vcs_segment(session: &Session) -> Option<String> {
    if session.settings().flag("prompt.vcs") == Some(false) {
        return None;
    }
    vcs_branch(session.cwd()).map(|branch| format!("git:{branch}"))
}

/// The branch `directory` is on, looking upwards for the checkout it belongs to.
fn vcs_branch(directory: &std::path::Path) -> Option<String> {
    let mut candidate = Some(directory);
    while let Some(here) = candidate {
        if let Some(branch) = branch_of(&here.join(".git")) {
            return Some(branch);
        }
        candidate = here.parent();
    }
    None
}

/// The branch named by the `HEAD` of the checkout `git` points at.
///
/// `git` is a directory in an ordinary clone and a file holding `gitdir: <path>` in a worktree
/// or a submodule; both are followed, once, because a chain deeper than that is git's business
/// and not a prompt's.
fn branch_of(git: &std::path::Path) -> Option<String> {
    let metadata = std::fs::metadata(git).ok()?;
    let directory = if metadata.is_dir() {
        git.to_path_buf()
    } else {
        let pointer = std::fs::read_to_string(git).ok()?;
        let target = pointer.trim().strip_prefix("gitdir:")?.trim();
        let target = std::path::Path::new(target);
        if target.is_absolute() {
            target.to_path_buf()
        } else {
            git.parent()?.join(target)
        }
    };
    let head = std::fs::read_to_string(directory.join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref: refs/heads/") {
        // A detached HEAD is a commit, and forty hex characters in a prompt is a wall: the
        // short form is what every other tool shows and what a person can compare.
        None => head
            .chars()
            .all(|c| c.is_ascii_hexdigit())
            .then(|| head.chars().take(7).collect::<String>())
            .filter(|short| short.len() == 7),
        Some(branch) => (!branch.is_empty()).then(|| branch.to_owned()),
    }
}

/// What the current spatial place adds to the link segment already painted (spec v0.4 §21.2).
///
/// `local` at the root adds nothing — the link says that — so the prompt stays exactly what v0.2
/// showed until the user has moved somewhere. Switched off entirely by `spatial.enabled`, which
/// §47 requires to leave the typed shell working.
fn spatial_place(session: &Session) -> Option<String> {
    if session.settings().flag("spatial.enabled") == Some(false) {
        return None;
    }
    let path = crate::spatial::place_segment()?;
    let (_, rest) = path.split_once('/')?;
    Some(format!("/{rest}"))
}

/// The compact spatial horizon of spec v0.4 §5, drawn once when an interactive session starts.
///
/// §5: "Starting an interactive Ono session MUST provide enough information to establish place
/// and nearby possibilities without requiring an explicit discovery command" — the host identity,
/// the canonical domains, their counts and the current landmarks. That is exactly what `look`
/// answers at the root, so the horizon is `look`, not a second renderer that could disagree with
/// it (§49.5). §29.1 is the other half: it is drawn only at a terminal, so a script's streams
/// carry nothing it did not ask for.
fn print_startup_horizon(session: &mut Session, reporter: &Reporter) {
    if session.settings().flag("spatial.enabled") == Some(false)
        || session.settings().flag("spatial.startup_horizon") == Some(false)
    {
        return;
    }
    let before = session.status();
    let _ = run_source(session, "look", reporter);
    session.set_status(before);
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ono_editor::Completer;

    use super::{ShellCompleter, short_path};

    fn completer() -> ShellCompleter {
        ShellCompleter {
            commands: vec!["cd".to_owned(), "git".to_owned()],
            // These tests are about the parts of a line the contracts and the filesystem
            // answer; a provider that would read the real machine has no place in them.
            values: None,
            adapters: None,
            resolver: None,
        }
    }

    #[test]
    fn should_offer_a_target_from_the_registry_when_a_verb_has_been_typed() {
        // Spec §15.1: completion is system exploration, and the registry is the only source that
        // knows a verb has targets at all.
        let completion = completer().complete("get pro", 7);
        assert!(
            completion.candidates.contains(&"process".to_owned()),
            "got {:?}",
            completion.candidates
        );
        assert_eq!(
            completion.span.start(),
            4,
            "the target word is what gets replaced"
        );
        assert_eq!(completion.span.end(), 7);
    }

    #[test]
    fn should_offer_a_verb_at_the_start_of_a_stage() {
        let completion = completer().complete("ge", 2);
        assert!(
            completion.candidates.contains(&"get".to_owned()),
            "got {:?}",
            completion.candidates
        );
    }

    #[test]
    fn should_offer_a_verb_after_a_pipe_because_a_new_stage_starts_there() {
        let completion = completer().complete("get process | whe", 17);
        assert!(
            completion.candidates.contains(&"where".to_owned()),
            "got {:?}",
            completion.candidates
        );
    }

    #[test]
    fn should_offer_an_option_the_command_declares_when_a_dash_has_been_typed() {
        let completion = completer().complete("get process --tr", 16);
        assert!(
            completion
                .candidates
                .iter()
                .any(|candidate| candidate.contains("tree")),
            "got {:?}",
            completion.candidates
        );
    }

    #[test]
    fn should_complete_the_line_when_the_editor_is_given_a_tab() {
        // The completer is only useful if the editor actually reaches it, and the wiring between
        // them is the kind of thing that looks right and is not.
        use ono_editor::{Editor, KeyCode, KeyPress, Outcome};

        let mut editor = Editor::new().with_completer(completer());
        for character in "get pro".chars() {
            assert!(matches!(
                editor.feed(KeyPress::char(character)),
                Outcome::Continue | Outcome::Redraw
            ));
        }
        editor.feed(KeyPress::key(KeyCode::Tab));
        assert_eq!(
            editor.line(),
            "get process",
            "Tab must complete the target the registry knows"
        );
    }

    #[test]
    fn should_not_offer_a_filesystem_path_where_an_option_was_asked_for() {
        // A path is the filesystem's business and an option is the registry's; offering both
        // would bury the answer the user asked for.
        let completion = completer().complete("get process --", 14);
        assert!(
            completion
                .candidates
                .iter()
                .all(|candidate| !candidate.contains('/')),
            "got {:?}",
            completion.candidates
        );
    }

    #[test]
    fn should_show_a_short_path_whole_when_it_already_fits() {
        assert_eq!(short_path(Path::new("/etc"), None), "/etc");
        assert_eq!(
            short_path(Path::new("/var/log/nginx"), None),
            "/var/log/nginx"
        );
    }

    #[test]
    fn should_write_the_home_directory_as_a_tilde_when_the_path_is_inside_it() {
        let home = Path::new("/home/case");
        assert_eq!(short_path(Path::new("/home/case"), Some(home)), "~");
        assert_eq!(short_path(Path::new("/home/case/src"), Some(home)), "~/src");
    }

    #[test]
    fn should_keep_the_last_component_whole_however_long_the_path_is() {
        let shown = short_path(
            Path::new("/home/case/projects/ono-sendai/crates/ono-cli/src"),
            None,
        );
        assert!(shown.ends_with("/src"), "got {shown}");
    }

    #[test]
    fn should_shrink_only_as_far_as_it_must_to_fit_the_prompt() {
        // Spec §4.2 asks the prompt to stay short; a prompt wider than the terminal wraps and
        // takes the line being typed with it.
        let shown = short_path(Path::new("/home/case/projects/ono-sendai/crates"), None);
        assert!(shown.chars().count() <= 32, "got {shown}");
        assert!(
            shown.contains("ono-sendai") || shown.contains("crates"),
            "the components nearest the end must survive, got {shown}"
        );
    }

    #[test]
    fn should_keep_two_characters_of_a_dotted_component_so_it_stays_recognisable() {
        let shown = short_path(
            Path::new("/home/case/.config/some-application/with/a/deep/tree"),
            None,
        );
        assert!(shown.contains(".c"), "got {shown}");
    }

    #[test]
    fn should_leave_a_single_component_alone_however_long_it_is() {
        let long = "/a-directory-with-an-unreasonably-long-single-name";
        assert_eq!(short_path(Path::new(long), None), long);
    }
}
