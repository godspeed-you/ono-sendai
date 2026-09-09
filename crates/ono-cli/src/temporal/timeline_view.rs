//! The terminal side of the full-screen timeline (spec v0.5 §19).
//!
//! §19.1 gives it two doors — `timeline --view`, and `T` from a temporal-capable map — and one
//! room behind them. This module is that room: it borrows the screen, reads keys, and turns
//! §19.3's keys into questions asked of the canonical query path.
//!
//! Nothing here decides what a timeline *is*. The window comes from
//! [`ono_temporal_query::timeline::timeline`] — the same call the non-interactive `timeline`
//! makes, over the same [`TimelineRequest`] — and [`ono_temporal_render::timeline_view`] draws
//! it. §39.3 forbids provider calls from a renderer and §55.7 forbids temporal logic in
//! `ono-cli`; between them, all this file may hold is a cursor, a scroll position and the
//! translation from a key press to a call somebody else implements (ADR-0781).
//!
//! Three promises are kept here:
//!
//! - **The shell's screen survives.** `open` takes raw mode and the alternate buffer as guards,
//!   in that order, so however the loop ends — Esc, Ctrl-C, an error, a panic unwinding — the
//!   terminal is cooked and the screen is back before the next prompt (v0.4 §49.8, §52.2,
//!   §44.10). `drive` is the same loop for a caller that already owns the terminal, which is
//!   how `T` opens this view from inside the map without a second set of guards over one screen.
//! - **A script never gets a screen.** `may_open` is false unless the evaluator says these
//!   values are being shown to a person at a terminal, so `timeline --view` in a pipe, in `ono
//!   -c` or under `TERM=dumb` answers with the text timeline instead (v0.2 §50, v0.4 §29.1).
//! - **The view queries, it does not assemble.** Every key that changes what is shown changes
//!   the request or the coordinate and asks again; no row is built here.

use std::sync::Arc;
use std::time::Duration;

use jiff::Timestamp;
use ono_command::Invocation;
use ono_core::ErrorCode;
use ono_editor::{AlternateScreen, KeyCode, KeyPress, RawMode, TerminalEvent};
use ono_temporal_core::TemporalContext;
use ono_temporal_query::timeline::TimelineRequest;
use ono_temporal_render::RenderOptions;
use ono_value::{ErrorValue, RecordValue, Value};

/// How long the view blocks on a key before coming up for air.
///
/// The timeline has nothing to do between key presses — it is a window over a window, not a live
/// view — so this is only how often the loop notices a resize it was not signalled about.
const IDLE_TICK: Duration = Duration::from_millis(120);

/// The rows §19.2's header takes before the first event row.
const HEADER_ROWS: usize = 2;

/// Whether the full-screen timeline may take this terminal (§19.1, v0.4 §29.1).
///
/// Three things must hold: the evaluator says this stage's values are being shown rather than
/// consumed, the shell is interactive at a terminal, and the terminal can be driven at all.
/// `spatial.map.mode` is deliberately not consulted — it is a preference about the *map*, and a
/// user who asked for the text map has said nothing about the timeline.
#[must_use]
pub fn may_open(ctx: &Invocation<'_>) -> bool {
    ctx.displays()
        && crate::spatial::at_terminal()
        && !crate::spatial::interactive::terminal_is_dumb()
}

/// Opens the full-screen timeline on this terminal (§19.1).
///
/// # Errors
///
/// A terminal that cannot be driven at all, or whatever §34 refusal the ledger raised while the
/// window was being read.
pub async fn open(
    request: TimelineRequest,
    context: Arc<TemporalContext>,
) -> Result<(), ErrorValue> {
    // Both guards, and in this order: the screen is given back before the line discipline, so a
    // terminal that dies mid-view is left cooked either way (v0.4 §44.10).
    let _raw = RawMode::enter().map_err(terminal_refused)?;
    let _screen = AlternateScreen::enter().map_err(terminal_refused)?;
    drive(request, context).await
}

/// The same view, for a caller that already owns raw mode and the alternate screen.
///
/// §18.3's `T` opens the timeline from inside the map, which is already holding both. Taking
/// them again would enter one alternate buffer twice and leave it once, so the map would paint
/// the rest of its life onto the shell's own screen; the guards therefore stay with whoever took
/// them first.
///
/// # Errors
///
/// Whatever §34 refusal the ledger raised while the window was being read.
pub async fn drive(
    request: TimelineRequest,
    context: Arc<TemporalContext>,
) -> Result<(), ErrorValue> {
    let (columns, rows) = ono_editor::terminal_size().unwrap_or((80, 24));
    let mut view = View::open(request, context, columns, rows).await?;
    view.run().await
}

/// What a key press asked the loop to do next.
enum Flow {
    /// Redraw and read the next key.
    Stay,
    /// Show these lines until the next key press, then redraw.
    Overlay(Vec<String>),
    /// Leave the view.
    Leave,
}

/// One selectable row of §19.2's body.
///
/// A row is an event, or the event standing for the several §19.4 folded into it. Which of the
/// two is what §19.5 asks about, so the count travels with the row.
struct Row {
    /// The identity of the event the row draws, as the cursor and the renderer both spell it.
    event_id: String,
    /// The canonical event itself, for the keys that ask something about it.
    event: Value,
    /// How many further events the row stands for (§19.4).
    hidden: usize,
}

/// The full-screen timeline: a window, a cursor over its rows, and the keys of §19.3.
struct View {
    /// The window being asked for — the horizon, the bounds and the kinds (§11.2).
    request: TimelineRequest,
    /// The coordinate the window is read at (§11.8).
    context: Arc<TemporalContext>,
    /// What the renderer is told: the zone, the source tags, the grouping and the cursor.
    options: RenderOptions,
    /// The `ono.temporal-timeline/1` the query answered with.
    record: RecordValue,
    /// The selectable rows of that record, in the order the renderer draws them.
    rows: Vec<Row>,
    /// Which of them the cursor is on.
    selected: usize,
    /// The terminal, as it was last known to be.
    columns: usize,
    height: usize,
    /// What the last key press had to say, until the next one.
    status: Option<String>,
    /// The search being typed, where one is (§19.3's `/`).
    search: Option<String>,
}

impl View {
    /// Reads the first window and puts the cursor on its most recent row.
    async fn open(
        request: TimelineRequest,
        context: Arc<TemporalContext>,
        columns: usize,
        height: usize,
    ) -> Result<Self, ErrorValue> {
        let mut options = crate::sink::temporal_options();
        // §19.4: "Large event sets MUST be semantically grouped rather than rendered as an
        // unreadable firehose." The default text rendering of §11.5 is a row per event and stays
        // one; this is the view §19.4 is written about, so grouping is on here.
        options.group_repeats = true;
        let record = read_window(&request, &context).await?;
        let rows = rows_of(&record, options.group_repeats);
        let selected = rows.len().saturating_sub(1);
        options.cursor = rows.get(selected).map(|row| row.event_id.clone());
        Ok(Self {
            request,
            context,
            options,
            record,
            rows,
            selected,
            columns,
            height,
            status: None,
            search: None,
        })
    }

    /// Draw, read a key, act, redraw (§19.2, §19.3).
    async fn run(&mut self) -> Result<(), ErrorValue> {
        let mut painted: Vec<String> = Vec::new();
        loop {
            let frame = self.frame();
            if frame != painted {
                let _ = ono_editor::paint(&frame);
                painted = frame;
            }
            let Some(event) =
                ono_editor::read_event_timeout(IDLE_TICK).map_err(terminal_refused)?
            else {
                continue;
            };
            match event {
                TerminalEvent::Resize(columns, rows) => {
                    // A resize is geometry. The window, the coordinate and the selection are
                    // untouched; only the width the rows are laid out at changes (v0.4 §43.4).
                    self.columns = columns;
                    self.height = rows;
                    ono_editor::remember_terminal_size(columns, rows);
                    painted.clear();
                }
                TerminalEvent::Key(press) if self.search.is_some() => self.type_into_search(press),
                TerminalEvent::Key(press) => {
                    let flow = self.act(press).await;
                    match flow {
                        Flow::Leave => return Ok(()),
                        Flow::Stay => {}
                        Flow::Overlay(lines) => {
                            self.show(&lines)?;
                            painted.clear();
                        }
                    }
                }
            }
        }
    }

    /// §19.3's keys.
    async fn act(&mut self, press: KeyPress) -> Flow {
        self.status = None;
        match press.code() {
            // v0.4 §43.4: Ctrl-C leaves the view and not the shell, exactly as it does in the map.
            KeyCode::Char('c') if press.modifiers().has_ctrl() => Flow::Leave,
            KeyCode::Esc => Flow::Leave,
            KeyCode::Up | KeyCode::Char('k') => {
                self.select(self.selected.saturating_sub(1));
                Flow::Stay
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.select(self.selected.saturating_add(1));
                Flow::Stay
            }
            KeyCode::Enter => self.inspect(),
            // §19.5's expansion, which §19.3 fixes no key for. `?` names it, because a key the
            // legend has no room for is otherwise a key nobody knows about.
            KeyCode::Char('x' | 'X') => {
                self.expand();
                Flow::Stay
            }
            KeyCode::Char('w' | 'W') => self.why().await,
            KeyCode::Char('a' | 'A') => {
                self.stand_at_the_selected_event().await;
                Flow::Stay
            }
            KeyCode::Char('/') => {
                self.search = Some(String::new());
                Flow::Stay
            }
            KeyCode::Char('g' | 'G') => self.gaps(),
            KeyCode::Char('n' | 'N') => {
                self.read_the_window_at_now().await;
                Flow::Stay
            }
            KeyCode::Char('?') => Flow::Overlay(help()),
            // §19.3's `M` and `C`, deliberately not built (ADR-0781). Saying so is the honest
            // answer; drawing them in the legend and doing nothing would not be.
            KeyCode::Char('m' | 'M') => {
                self.status = Some(
                    "this view does not open the map yet; `A` stands the session at the event, \
                     and `map` there draws it (v0.5 §19.3, ADR-0781)"
                        .to_owned(),
                );
                Flow::Stay
            }
            KeyCode::Char('c' | 'C') => {
                self.status = Some(
                    "correlations are not drawn in the window yet; `W` shows the selected \
                     event's, with the evidence behind them (v0.5 §16.5, ADR-0781)"
                        .to_owned(),
                );
                Flow::Stay
            }
            _ => Flow::Stay,
        }
    }

    /// Reads the window again and keeps the cursor on the event it was on (§11, §39.3).
    ///
    /// This is the only place a timeline is read, and it reads it the way the command does: one
    /// [`TimelineRequest`], one call, one record. Every key that changes what is shown changes
    /// the request or the coordinate and comes back through here.
    async fn reload(&mut self) -> Result<(), ErrorValue> {
        let held = self.selected_id();
        let record = read_window(&self.request, &self.context).await?;
        self.rows = rows_of(&record, self.options.group_repeats);
        self.record = record;
        self.selected = held
            .and_then(|id| self.rows.iter().position(|row| row.event_id == id))
            // §19.2's cursor opens on the most recent row: a timeline is read from the present
            // backwards, and the newest event is what a reader is looking at when it opens.
            .unwrap_or_else(|| self.rows.len().saturating_sub(1));
        self.mark_the_cursor();
        Ok(())
    }

    /// The event the cursor is on, where there is one.
    fn selected_id(&self) -> Option<String> {
        self.rows.get(self.selected).map(|row| row.event_id.clone())
    }

    /// Tells the renderer which row the cursor is on (§19.2).
    fn mark_the_cursor(&mut self) {
        self.options.cursor = self.selected_id();
    }

    /// Moves the cursor to `row`, clamped to the rows there are.
    fn select(&mut self, row: usize) {
        if self.rows.is_empty() {
            return;
        }
        self.selected = row.min(self.rows.len() - 1);
        self.mark_the_cursor();
    }

    /// §19.3's `Enter`: what the selected event is, in the fields the record carries.
    fn inspect(&mut self) -> Flow {
        let Some((event, hidden)) = self
            .rows
            .get(self.selected)
            .map(|row| (row.event.clone(), row.hidden))
        else {
            return self.nothing_selected();
        };
        let mut lines = vec![
            format!(
                " {}  {}",
                reference_of(&event).unwrap_or_default(),
                text_of(&event, "kind").unwrap_or_default()
            ),
            String::new(),
        ];
        if let Some(subject) = field_of(&event, "subject")
            .and_then(|subject| text_of(subject, "label").or_else(|| text_of(subject, "described")))
        {
            lines.push(format!("  {:<12}{subject}", "subject"));
        }
        for (label, name) in [
            ("kind", "kind"),
            ("subtype", "subtype"),
            ("source", "source"),
            ("scope", "scope"),
            ("source time", "source_time"),
            ("observed", "observed_at"),
            ("ingested", "ingested_at"),
        ] {
            if let Some(shown) = shown_field(&event, name) {
                lines.push(format!("  {label:<12}{shown}"));
            }
        }
        lines.push(format!(
            "  {:<12}{}",
            "evidence",
            list_of(&event, "evidence").len()
        ));
        if hidden > 0 {
            lines.push(format!(
                "  {:<12}{hidden} further events (v0.5 §19.4)",
                "stands for"
            ));
        }
        Flow::Overlay(lines)
    }

    /// §19.5's expansion: a grouped row opens into the individuals it stands for.
    ///
    /// The members travel on the record the planner produced, so opening one asks nothing new of
    /// the ledger. A row that stands for one event has nothing to open, and says so rather than
    /// pretending (§2.17).
    fn expand(&mut self) {
        let Some((id, hidden)) = self
            .rows
            .get(self.selected)
            .map(|row| (row.event_id.clone(), row.hidden))
        else {
            self.status = Some("there is no row to expand".to_owned());
            return;
        };
        if hidden == 0 {
            self.status =
                Some("this row stands for one event, so there is nothing to expand".to_owned());
            return;
        }
        if let Some(at) = self.options.expanded.iter().position(|held| held == &id) {
            self.options.expanded.remove(at);
            self.status = Some(format!("{hidden} events folded back into one row"));
        } else {
            self.options.expanded.push(id);
            self.status = Some(format!("{hidden} further events shown"));
        }
    }

    /// §19.3's `W`: the causal explanation of the selected event (§16.2's `why event @e42`).
    async fn why(&mut self) -> Flow {
        let Some(reference) = self.rows.get(self.selected).and_then(|row| {
            reference_of(&row.event).or_else(|| Some(format!("@{}", row.event_id)))
        }) else {
            return self.nothing_selected();
        };
        let explained = self.explanation_of(&reference).await;
        match explained {
            Ok(explanation) => Flow::Overlay(ono_temporal_render::causal_explanation(
                &explanation,
                self.columns,
                &self.options,
            )),
            Err(error) => {
                self.status = Some(error.message().to_owned());
                Flow::Stay
            }
        }
    }

    /// Asks the canonical causal engine about one event (§16.1, §16.3, §32.3).
    async fn explanation_of(&self, reference: &str) -> Result<RecordValue, ErrorValue> {
        use ono_temporal_query::causal::{CausalContext, CausalEngine, WhyOptions, WhyRequest};

        let now = Timestamp::now();
        let mut state = crate::temporal::session::temporal_session().await;
        let anchor = state.context().instant().unwrap_or(now);
        let ledger = state.ledger_handle();
        let event = state
            .references()
            .resolve(reference, ledger.as_ref())?
            .ok_or_else(|| {
                ono_temporal_core::error::invalid_time(
                    reference,
                    "no event with that reference is retained",
                )
            })?;
        let request = WhyRequest::event(&event.event_id);
        // §33's `temporal.why.max_candidates` bounds the question, so it bounds the read as well
        // as the engine — the same discipline the `why` command keeps.
        let max_candidates = crate::temporal::session::why_max_candidates();
        let mut events = ledger.events(&ono_temporal_core::EventQuery {
            scope: Some(crate::spatial::local_scope()),
            subjects: Vec::new(),
            kinds: Vec::new(),
            range: ono_temporal_core::TimeRange::until(anchor),
            limit: Some(max_candidates),
            order: ono_temporal_core::QueryOrder::Descending,
        })?;
        events.reverse();
        let evidence_ids: Vec<ono_temporal_core::EvidenceId> = events
            .iter()
            .flat_map(|held| held.evidence.iter().cloned())
            .collect();
        let causal = CausalContext::new(ledger.evidence(&evidence_ids)?);
        let options = WhyOptions::at(anchor).with_max_candidates(max_candidates);
        CausalEngine::builtin()
            .explain(&request, &events, &causal, &options)?
            .to_record()
    }

    /// §19.3's `A`: the session stands at the selected event (§4.2, §12.2).
    ///
    /// The instant is spelled and resolved, rather than assigned: §4.5 forbids a second
    /// historical code path, so this goes through the one function `at` and `--at` both use, and
    /// an instant no source reaches is refused here exactly as `at` would refuse it (§12.3).
    async fn stand_at_the_selected_event(&mut self) {
        let Some(at) = self
            .rows
            .get(self.selected)
            .and_then(|row| instant_of(&row.event))
        else {
            self.status = Some("there is no event to stand at".to_owned());
            return;
        };
        let spelling = at.to_string();
        let now = Timestamp::now();
        let moved = {
            let mut state = crate::temporal::session::temporal_session().await;
            match crate::temporal::coordinate::resolve(&state, &spelling, now) {
                Ok(context) => {
                    state.commit(context.clone(), &spelling, now);
                    Ok(context)
                }
                Err(error) => Err(error),
            }
        };
        match moved {
            Ok(context) => {
                // §55.9: a coordinate the spatial layer does not evaluate against is a past mode
                // that is only cosmetic.
                crate::temporal::session::install_evidence();
                self.status = context
                    .prompt_marker()
                    .map(|marker| format!("the session is standing at the selected event {marker}"))
                    .or_else(|| Some("the session is standing at the selected event".to_owned()));
                self.context = Arc::new(context);
                if let Err(error) = self.reload().await {
                    self.status = Some(error.message().to_owned());
                }
            }
            Err(error) => self.status = Some(error.message().to_owned()),
        }
    }

    /// §19.3's `N`: the window reads the events near now again.
    ///
    /// This moves the *view*, not the session: §4.3's `now` is a command the user types, and a
    /// view that quietly returned the session to the present would take back a coordinate they
    /// had asked for.
    async fn read_the_window_at_now(&mut self) {
        self.context = Arc::new(TemporalContext::Present);
        self.request.since = None;
        self.request.until = None;
        let read = self.reload().await;
        match read {
            Ok(()) => {
                self.select(self.rows.len().saturating_sub(1));
                self.status = Some("the window ends at now".to_owned());
            }
            Err(error) => self.status = Some(error.message().to_owned()),
        }
    }

    /// §19.3's `G`: the coverage gaps inside the window, in full (§11.7, §55.5).
    ///
    /// The rows never hide one — §11.7 makes that a MUST and the renderer holds it — so what this
    /// key adds is the whole of each gap where a row has room only for its opening line.
    fn gaps(&mut self) -> Flow {
        let gaps = list_of_record(&self.record, "gaps").to_vec();
        if gaps.is_empty() {
            self.status = Some("no coverage gap falls inside this window".to_owned());
            return Flow::Stay;
        }
        let mut lines = vec![
            format!(
                " {} coverage {} in this window (v0.5 §11.7)",
                gaps.len(),
                if gaps.len() == 1 { "gap" } else { "gaps" }
            ),
            String::new(),
        ];
        for gap in &gaps {
            if let Value::Record(gap) = gap {
                lines.extend(ono_temporal_render::gap_frame(
                    gap,
                    self.columns,
                    &self.options,
                ));
                lines.push(String::new());
            }
        }
        Flow::Overlay(lines)
    }

    /// §19.3's `/`: the next event whose canonical form carries what was typed.
    fn jump_to(&mut self, needle: &str) {
        let needle = needle.trim().to_lowercase();
        if needle.is_empty() {
            self.status = Some("nothing to search for".to_owned());
            return;
        }
        let count = self.rows.len();
        let found = (1..=count)
            .map(|step| (self.selected + step) % count)
            .find(|row| {
                self.rows
                    .get(*row)
                    .is_some_and(|row| matches(&row.event, &needle))
            });
        match found {
            Some(row) => {
                self.select(row);
                self.status = Some(format!("/{needle}"));
            }
            None => self.status = Some(format!("no event in this window matches `{needle}`")),
        }
    }

    /// Reads one key of a search, and runs it on Return (§19.3's `/`).
    fn type_into_search(&mut self, press: KeyPress) {
        let mut needle = self.search.take().unwrap_or_default();
        match press.code() {
            KeyCode::Esc => {
                self.status = Some("search cancelled".to_owned());
                return;
            }
            KeyCode::Enter => {
                self.jump_to(&needle);
                return;
            }
            KeyCode::Backspace => {
                needle.pop();
            }
            KeyCode::Char(character) => needle.push(character),
            _ => {}
        }
        self.search = Some(needle);
    }

    /// The answer to a key that needs a row when there is none.
    fn nothing_selected(&mut self) -> Flow {
        self.status = Some("this window holds no event to act on".to_owned());
        Flow::Stay
    }

    /// §19.2's screen: the header, as many rows as the terminal holds, the evidence line, the
    /// legend, and whatever the last key press had to say.
    ///
    /// The renderer lays the whole window out and this takes a viewport out of the middle of it,
    /// so the header and the legend stay where §19.2 puts them however long the window is. The
    /// body ends at the last blank line the renderer wrote, which is the one it separates the
    /// evidence line with.
    fn frame(&self) -> Vec<String> {
        let drawn = ono_temporal_render::timeline_view(&self.record, self.columns, &self.options);
        let head = drawn.len().min(HEADER_ROWS);
        let tail_at = drawn
            .iter()
            .rposition(String::is_empty)
            .unwrap_or(drawn.len())
            .max(head);
        let tail = &drawn[tail_at..];
        let said = self.said();
        let room = self
            .height
            .saturating_sub(head + tail.len() + usize::from(said.is_some()))
            .max(1);
        let body = &drawn[head..tail_at];
        // The cursor stays on the screen: while it fits above the fold the window starts at the
        // top, and below it the window ends on the cursor's own row.
        let offset = match body.iter().position(|line| line.starts_with('>')) {
            Some(at) if at >= room => at + 1 - room,
            _ => 0,
        };

        let mut frame: Vec<String> = drawn[..head].to_vec();
        frame.extend(body.iter().skip(offset).take(room).cloned());
        while frame.len() < head + room {
            frame.push(String::new());
        }
        frame.extend(tail.iter().cloned());
        if let Some(said) = said {
            frame.push(clip(&format!(" {said}"), self.columns));
        }
        // A terminal too short to hold the header, the legend and one row still gets a frame it
        // fits. Writing past the last row scrolls the alternate buffer, and a view whose rows have
        // shifted under it draws every later frame in the wrong place.
        frame.truncate(self.height.max(1));
        frame
    }

    /// What the footer line says, where it says anything.
    fn said(&self) -> Option<String> {
        match (&self.search, &self.status) {
            (Some(needle), _) => Some(format!("/{needle}")),
            (None, Some(status)) => Some(status.clone()),
            (None, None) => None,
        }
    }

    /// Shows `lines` over the whole screen until the next key press.
    fn show(&mut self, lines: &[String]) -> Result<(), ErrorValue> {
        let room = self.height.saturating_sub(1).max(1);
        let mut frame: Vec<String> = lines
            .iter()
            .take(room)
            .map(|line| clip(line, self.columns))
            .collect();
        while frame.len() < room {
            frame.push(String::new());
        }
        frame.push(clip(" any key returns to the timeline", self.columns));
        let _ = ono_editor::paint(&frame);
        loop {
            match ono_editor::read_event_timeout(IDLE_TICK).map_err(terminal_refused)? {
                Some(TerminalEvent::Key(_)) => return Ok(()),
                Some(TerminalEvent::Resize(columns, rows)) => {
                    self.columns = columns;
                    self.height = rows;
                    ono_editor::remember_terminal_size(columns, rows);
                    return Ok(());
                }
                None => {}
            }
        }
    }
}

/// The window a request asks for, read the way the `timeline` command reads it (§11, §55.7).
///
/// One [`TimelineRequest`], one call into `ono-temporal-query`, one `ono.temporal-timeline/1`.
/// There is no second query path: what a key press changes is the request or the coordinate, and
/// the answer comes back through here.
async fn read_window(
    request: &TimelineRequest,
    context: &TemporalContext,
) -> Result<RecordValue, ErrorValue> {
    let now = Timestamp::now();
    let mut state = crate::temporal::session::temporal_session().await;
    let ledger = state.ledger_handle();
    let timeline = ono_temporal_query::timeline::timeline(ledger.as_ref(), request, context, now)?;
    // §11.6: the reference a row prints is the one `at event`, `inspect event` and `why event`
    // accept — minted once per session, and short only as far as this ledger can tell it apart.
    timeline
        .with_references(ledger.as_ref(), state.references())
        .to_record()
}

/// §19.3's keys as `?` lists them, plus the two this view answers differently.
fn help() -> Vec<String> {
    [
        " the full-screen timeline (v0.5 §19.3)",
        "",
        "  Up / k        select the event above",
        "  Down / j      select the event below",
        "  Enter         inspect the selected event",
        "  X             expand a grouped row into the events it stands for (§19.5)",
        "  W             why the selected event happened",
        "  A             stand the session at the selected event",
        "  /             search the window; Return jumps, Esc cancels",
        "  G             the coverage gaps inside the window in full",
        "  N             read the window at now again",
        "  ?             this help",
        "  Esc / Ctrl-C  leave the view",
        "",
        "  M and C — the map at the event's time, and correlation display — are not built in",
        "  this view (ADR-0781). `A` then `map` reaches the first, `W` the second.",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

/// The selectable rows of a window, in the order [`ono_temporal_render::timeline_view`] draws
/// them.
///
/// With grouping on, the planner's own `groups` are the rows — the judgement is made once, in
/// `ono-temporal-query`, and both the renderer and this cursor read it rather than re-deriving
/// it (§19.4, §55.7). A group naming no retained event is dropped, exactly as the renderer drops
/// it, so the cursor never points at a row nobody drew.
fn rows_of(record: &RecordValue, grouped: bool) -> Vec<Row> {
    let events = list_of_record(record, "events");
    if grouped {
        let groups = list_of_record(record, "groups");
        if !groups.is_empty() {
            let mut rows = Vec::with_capacity(groups.len());
            for group in groups {
                let members = list_of(group, "members");
                let Some(event) = members
                    .iter()
                    .filter_map(|member| member.as_str().ok())
                    .find_map(|id| {
                        events
                            .iter()
                            .find(|event| text_of(event, "event_id").is_some_and(|held| held == id))
                    })
                else {
                    continue;
                };
                rows.push(Row {
                    event_id: text_of(event, "event_id").unwrap_or_default(),
                    event: event.clone(),
                    hidden: match field_of(group, "hidden") {
                        Some(Value::Int(hidden)) => usize::try_from(*hidden).unwrap_or(0),
                        _ => 0,
                    },
                });
            }
            if !rows.is_empty() {
                return rows;
            }
        }
    }
    events
        .iter()
        .map(|event| Row {
            event_id: text_of(event, "event_id").unwrap_or_default(),
            event: event.clone(),
            hidden: 0,
        })
        .collect()
}

/// Whether an event carries `needle` anywhere in its canonical form (§19.3's `/`).
///
/// The canonical text is the value as the rest of the shell writes it, so a search matches what
/// `to json` would show rather than a rendering this view invented.
fn matches(event: &Value, needle: &str) -> bool {
    ono_value::canonical_text(event)
        .map(|text| text.to_lowercase().contains(needle))
        .unwrap_or(false)
}

/// A field of a record or of the map a nested sub-record is written as.
fn field_of<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    match value {
        Value::Record(record) => record.get(name),
        Value::Map(map) => map.get(name),
        _ => None,
    }
}

/// A text field of an event.
fn text_of(value: &Value, name: &str) -> Option<String> {
    match field_of(value, name) {
        Some(Value::String(text)) if !text.is_empty() => Some(text.to_string()),
        _ => None,
    }
}

/// A field as a person reads it, absent where the record carries none (§35.3).
fn shown_field(event: &Value, name: &str) -> Option<String> {
    match field_of(event, name) {
        None | Some(Value::Null) => None,
        Some(value) => ono_value::canonical_text(value).ok(),
    }
}

/// A list field of an event.
fn list_of<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    match field_of(value, name) {
        Some(Value::List(items)) => items,
        _ => &[],
    }
}

/// A list field of a record.
fn list_of_record<'a>(record: &'a RecordValue, name: &str) -> &'a [Value] {
    match record.get(name) {
        Some(Value::List(items)) => items,
        _ => &[],
    }
}

/// The `@e…` reference a row printed, which is the spelling every command accepts (§11.6).
fn reference_of(event: &Value) -> Option<String> {
    text_of(event, "reference").map(|given| {
        if given.starts_with('@') {
            given
        } else {
            format!("@{given}")
        }
    })
}

/// The instant a person navigates by: the source's own time where it gave one, else the
/// observation (§3.3).
fn instant_of(event: &Value) -> Option<Timestamp> {
    match field_of(event, "source_time").or_else(|| field_of(event, "observed_at")) {
        Some(Value::Timestamp(at)) => Some(*at),
        _ => None,
    }
}

/// A line clipped to the terminal.
fn clip(line: &str, width: usize) -> String {
    line.chars().take(width.max(1)).collect::<String>()
}

/// A terminal that refused to be driven at all.
fn terminal_refused(error: std::io::Error) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::TemporalUnsupportedSource,
        format!("this terminal cannot show the full-screen timeline: {error}"),
    )
    .with_help("`timeline` writes the same window as text and needs no terminal (v0.5 §11.5)")
}
