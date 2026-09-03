//! The terminal side of the full-screen map, and the ambiguity picker (spec v0.4 §23.3, §23.4,
//! §25.1, §27.2, §39, §43.4, §44.10, §49.8, §52.2).
//!
//! Everything that *decides* anything about the view lives in `ono-spatial-render`, which needs
//! no terminal to be tested. This module is the part that cannot be: it borrows the screen,
//! reads keys, notices a resize, and turns the effects the view asks for into the same movements
//! the commands make (`go_back`, `go_up`, `go_home` — one implementation each, §43.4).
//!
//! Three promises are kept here and nowhere else:
//!
//! - **The shell's screen survives.** The view runs on the alternate buffer and in raw mode, and
//!   both are guards: however the loop ends — Esc, Ctrl-C, an error, a panic unwinding — the
//!   terminal is cooked and the screen is back before the next prompt (§49.8, §52.2, §44.10).
//! - **Focus is not movement.** The view answers with an [`Effect`], and only `Enter`, `Follow`,
//!   `Back`, `Up` and `Home` touch the session's place (§23.4, §53).
//! - **A script never gets a screen.** [`may_open`] is false unless the evaluator says these
//!   values are being shown to a person at a terminal (§29.1), and [`pick`] is false in the same
//!   places, because §29.3 forbids a script from ever opening a picker.

use std::collections::VecDeque;
use std::future::Future;
use std::time::Duration;

use jiff::Timestamp;
use ono_command::Invocation;
use ono_core::ErrorCode;
use ono_editor::{AlternateScreen, KeyCode, KeyPress, RawMode, TerminalEvent};
use ono_spatial_core::{Movement, NavigationStep, SpatialId};
use ono_spatial_query::{Candidate, MapRequest};
use ono_spatial_render::{Action, Effect, Key, Keymap, MapView};
use ono_value::ErrorValue;

use crate::spatial::session::SpatialSessionState;

/// How long a live view waits for a key before asking the providers again (§25.1's polling
/// source). Short enough that a key never feels late, long enough that nothing is busy.
const LIVE_TICK: Duration = Duration::from_millis(250);

/// How long a still view blocks on a key. It has nothing to do between them; this is only how
/// often the loop comes up for air.
const IDLE_TICK: Duration = Duration::from_millis(200);

/// How often a live view asks the providers again (§25.1's explicit polling source).
const LIVE_INTERVAL: Duration = Duration::from_secs(1);

/// How often work the view is waiting for comes up for air to answer a key.
///
/// v0.4 §34 gives "focus/navigation inside rendered map" a 16 ms frame target, so that is the
/// longest a key should wait for a slice of the loop's attention.
const ANSWER_SLICE: Duration = Duration::from_millis(16);

/// The most keys drained in one [`ANSWER_SLICE`], so a held-down key cannot starve the work.
const KEYS_PER_SLICE: usize = 64;

/// How many keys may wait for the loop before draining stops and the terminal keeps the rest.
const KEY_BACKLOG: usize = 1024;

/// What came back from work the view waited for while it kept answering keys.
#[derive(Debug, PartialEq, Eq)]
enum Awaited<T> {
    /// The work finished, and this is what it said.
    Done(T),
    /// The user closed the view before the work finished, so its answer is not wanted.
    Left,
}

/// Awaits `work` without the view going deaf while it runs.
///
/// v0.4 §34: "the shell MUST remain interactive and progressively update rather than block
/// unnecessarily." Re-observing a space asks every provider that answers for it, and a provider
/// is allowed to be slow — the systemd bus alone gives each call ten seconds, and one
/// re-observation makes several. Awaiting that on the loop's own thread read no key at all while
/// it ran, so `Esc` could not close a map that was busy: the one key whose whole purpose is to
/// get out was the one key that stopped working, and the view held a terminal nobody could take
/// back (ADR-0424).
///
/// `keys` must not block; it is polled every [`ANSWER_SLICE`] and returns `None` when the
/// terminal has nothing more to say. Keys go on `waiting` **in the order they were typed**, for
/// the loop to answer when the work is done: someone who types `Enter` and then `Backspace`
/// without pausing meant both, and a view that swallowed the second would lose a keystroke the
/// user made.
///
/// The one key that does not wait is [`Action::Close`] with nothing queued ahead of it. That is
/// the case this exists for — a user watching a view that has stopped answering — and it is the
/// only case where leaving cannot overtake an instruction the user gave first.
///
/// Draining stops at [`KEY_BACKLOG`]; what is left stays in the terminal's own buffer, which is
/// where unread input belongs.
async fn while_answering<T>(
    work: impl Future<Output = T>,
    mut keys: impl FnMut() -> Option<Key>,
    keymap: &Keymap,
    waiting: &mut VecDeque<Key>,
) -> Awaited<T> {
    let mut work = std::pin::pin!(work);
    loop {
        tokio::select! {
            biased;
            value = &mut work => return Awaited::Done(value),
            () = tokio::time::sleep(ANSWER_SLICE) => {
                for _ in 0..KEYS_PER_SLICE {
                    if waiting.len() >= KEY_BACKLOG {
                        break;
                    }
                    let Some(key) = keys() else { break };
                    if keymap.action(key) == Some(Action::Close) && waiting.is_empty() {
                        return Awaited::Left;
                    }
                    waiting.push_back(key);
                }
            }
        }
    }
}

/// What a redraw needs from the terminal side, kept together: how to draw, what the keys mean,
/// and where a key pressed while it runs waits for its turn.
struct Ui<'a> {
    /// The character set the view draws with (§39.2).
    charset: ono_spatial_render::Charset,
    /// What a key means, so the one that closes the view is recognised while a provider is slow.
    keymap: &'a Keymap,
    /// Keys typed during the observation, answered by the loop once it is done.
    waiting: &'a mut VecDeque<Key>,
}

/// The frame drawn before the first projection exists (v0.4.1 §33.1, §35.2).
///
/// §35.2 permits a first frame that does not hold every edge, and requires it to be "truthful
/// about omitted/pending detail". Nothing is projected yet, so the only truthful thing to draw is
/// where the user is and that the picture is on its way — which is exactly §65.9's "results,
/// progress or a bounded refusal", in the one of the three that fits.
fn opening_frame(place: &str, columns: usize, rows: usize) -> Vec<String> {
    let width = columns.max(1);
    let mut frame = Vec::with_capacity(rows);
    frame.push(truncated(place, width));
    frame.push(String::new());
    frame.push(truncated(
        "projecting this place — no detail is drawn yet",
        width,
    ));
    while frame.len() < rows {
        frame.push(String::new());
    }
    frame
}

/// `text`, cut to `width` characters so a narrow terminal is not written past its edge.
fn truncated(text: &str, width: usize) -> String {
    text.chars().take(width).collect()
}

/// The next key the terminal has ready, without waiting for one.
fn ready_key() -> Option<Key> {
    match ono_editor::read_event_timeout(Duration::ZERO) {
        Ok(Some(TerminalEvent::Key(press))) => translate(press),
        _ => None,
    }
}

/// Whether the full-screen view may take this terminal (§23.3, §29.1, §47's `spatial.map.mode`).
///
/// Four things must hold: the shell is interactive at a terminal, the evaluator says this
/// stage's values are being shown rather than consumed, the terminal can be driven at all, and
/// the user has not asked for the text map with `spatial.map.mode = "text"`.
pub fn may_open(ctx: &Invocation<'_>) -> bool {
    // §29.1 holds whatever the mode says: values that are about to be consumed, redirected or
    // captured are values, and a screen is not one of the things a pipeline can read.
    if !ctx.displays() || !crate::spatial::at_terminal() {
        return false;
    }
    match map_mode().as_str() {
        "text" => false,
        // The user asked for the view outright, so the `TERM` guess is not consulted.
        "fullscreen" => true,
        // `auto`, and anything a future version spells that this one does not know.
        _ => !terminal_is_dumb(),
    }
}

/// Whether `spatial.map.live` asks every map to subscribe (§25.1, §47).
#[must_use]
pub fn live_by_default() -> bool {
    setting_flag("spatial.map.live")
}

/// A terminal that cannot be driven at all. `TERM=dumb` is the honest case: no cursor
/// addressing, no alternate screen, so §23.2's text map is the whole answer (§39.2).
fn terminal_is_dumb() -> bool {
    std::env::var("TERM").is_ok_and(|term| term.is_empty() || term == "dumb")
}

/// The full-screen map: draw, read a key, act, redraw (spec v0.4 §23.3).
///
/// # Errors
///
/// Whatever the providers refused with while a projection was being built. A refusal from a
/// *movement* — `back` at the start of the trail, `up` at the root — is shown in the view's
/// footer instead, because it is an answer to a key press and not a reason to close the screen.
pub async fn run_map_view(
    ctx: &Invocation<'_>,
    session: &mut SpatialSessionState,
    pins: Option<&crate::spatial::PinStore>,
    center: SpatialId,
    request: MapRequest,
    live: bool,
    now: Timestamp,
) -> Result<(), ErrorValue> {
    let charset = crate::sink::map_charset();
    let mut request = request;
    let mut center = center;

    let (columns, rows) = ono_editor::terminal_size().unwrap_or((80, 24));
    let keymap = configured_keymap();

    // Both guards, and in this order: the screen is given back before the line discipline, so a
    // terminal that dies mid-view is left cooked either way.
    //
    // They are taken *before* the opening projection, and a frame is painted before it too. v0.4
    // §34 requires the shell to "remain interactive and progressively update rather than block
    // unnecessarily", and v0.4.1 §33.1 makes time to first useful result a first-class target;
    // projecting COMPUTE on a busy host takes about a second, and doing it with the terminal
    // still cooked meant no frame, no key read and nothing on the screen for the whole of it
    // (issue #20, ADR-0493).
    let _raw = RawMode::enter().map_err(terminal_refused)?;
    let _screen = AlternateScreen::enter().map_err(terminal_refused)?;

    // Keys typed while an observation was running. They are answered in order once it is done,
    // so nothing a user pressed is lost to a slow provider (ADR-0424).
    let mut waiting: VecDeque<Key> = VecDeque::new();
    let opening = opening_frame(&place_path(session, &center), columns, rows);
    let _ = ono_editor::paint(&opening);
    let mut painted: Vec<String> = opening;

    // The projection now runs where a key can reach it: §35.2's "the UI MAY progressively refine
    // after the first frame", with the first frame saying that the detail is still pending rather
    // than pretending the place is empty (§2.17).
    let mut record = match while_answering(
        crate::spatial::map::projection(ctx, session, &center, &request, now),
        ready_key,
        &keymap,
        &mut waiting,
    )
    .await
    {
        Awaited::Done(projected) => projected?,
        Awaited::Left => return Ok(()),
    };

    let mut view = MapView::new(&record, columns, rows, charset, keymap.clone());
    view.set_live(live, "polled");
    view.set_place(place_path(session, &center));

    let mut refreshed = std::time::Instant::now();
    loop {
        // §25.2 forbids motion that is not a state change, and §39.4 asks that a reduced-motion
        // setting leave nothing moving. A frame identical to the one already on the screen is
        // therefore not written at all: the view is still exactly as long as the system is.
        let frame = view.frame();
        if frame != painted {
            let _ = ono_editor::paint(&frame);
            painted = frame;
        }

        // A key typed while an observation was running is answered before the terminal is asked
        // for a new one, so the view acts in the order the user typed (ADR-0424).
        let key = match waiting.pop_front() {
            Some(waited) => waited,
            None => {
                let patience = if view.is_live() { LIVE_TICK } else { IDLE_TICK };
                let event = ono_editor::read_event_timeout(patience).map_err(terminal_refused)?;

                let Some(event) = event else {
                    // A live view has a second reason to redraw: the machine changed. §25.1 allows an
                    // explicit polling source where no event stream exists, and §25.3 makes the view say
                    // so — the freshness word beside the heading is `polled`, never `event driven`.
                    if view.is_live() && refreshed.elapsed() >= LIVE_INTERVAL {
                        record = match redraw(
                            ctx,
                            session,
                            &mut view,
                            &center,
                            &request,
                            record,
                            Ui {
                                charset,
                                keymap: &keymap,
                                waiting: &mut waiting,
                            },
                        )
                        .await
                        {
                            Awaited::Done(drawn) => drawn,
                            Awaited::Left => return Ok(()),
                        };
                        refreshed = std::time::Instant::now();
                    }
                    continue;
                };

                let press = match event {
                    TerminalEvent::Resize(columns, rows) => {
                        // §43.4: a resize preserves the current place and the focus. Neither is touched
                        // here; only the width the projection is drawn at and the viewport are (§39.3).
                        let at = Timestamp::now();
                        let observed = while_answering(
                            crate::spatial::map::projection(ctx, session, &center, &request, at),
                            ready_key,
                            &keymap,
                            &mut waiting,
                        )
                        .await;
                        match observed {
                            Awaited::Done(drawn) => record = drawn.unwrap_or(record),
                            Awaited::Left => return Ok(()),
                        }
                        view.set_place(place_path(session, &center));
                        view.resize(&record, columns, rows, charset);
                        // The view has drawn itself at the new size, so the change is answered.
                        // Until this is said, the terminal keeps reporting it — which is what
                        // stops `ready_key` from swallowing a resize that arrives while a
                        // projection is running (issue #6).
                        ono_editor::remember_terminal_size(columns, rows);
                        continue;
                    }
                    TerminalEvent::Key(press) => press,
                };
                let Some(key) = translate(press) else {
                    continue;
                };
                key
            }
        };

        match view.apply(key) {
            Effect::Stay => {}
            Effect::Close => return Ok(()),
            Effect::Enter(node) => {
                match SpatialId::parse(&node) {
                    Some(id) => {
                        enter(session, &id, Timestamp::now());
                        center = id;
                        // A movement starts the view again at the new place, exactly as typing
                        // `map` there would: the clusters the old place had expanded and the
                        // node the old map focused are answers to a question nobody is asking
                        // any more.
                        request = MapRequest::new();
                    }
                    // A cluster stands for objects rather than being one, so Enter on a cluster
                    // draws what it stood for. §8.3: "expansion is a view action", and this is
                    // the view; the place does not move.
                    None => request = request.clone().expand(vec![node]),
                }
                record = match redraw(
                    ctx,
                    session,
                    &mut view,
                    &center,
                    &request,
                    record,
                    Ui {
                        charset,
                        keymap: &keymap,
                        waiting: &mut waiting,
                    },
                )
                .await
                {
                    Awaited::Done(drawn) => drawn,
                    Awaited::Left => return Ok(()),
                };
            }
            Effect::Follow { relation, node } => match SpatialId::parse(&node) {
                Some(there) => {
                    follow(session, &relation, &there, Timestamp::now());
                    center = there;
                    request = MapRequest::new();
                    record = match redraw(
                        ctx,
                        session,
                        &mut view,
                        &center,
                        &request,
                        record,
                        Ui {
                            charset,
                            keymap: &keymap,
                            waiting: &mut waiting,
                        },
                    )
                    .await
                    {
                        Awaited::Done(drawn) => drawn,
                        Awaited::Left => return Ok(()),
                    };
                }
                None => view.say("that edge points at a cluster, not at one place"),
            },
            effect @ (Effect::Back | Effect::Up | Effect::Home) => {
                let at = Timestamp::now();
                let moved = match effect {
                    Effect::Back => crate::spatial::movement::go_back(session, at),
                    Effect::Up => crate::spatial::movement::go_up(session, at),
                    _ => {
                        crate::spatial::commands::go_home(session, at);
                        Ok(())
                    }
                };
                match moved {
                    Ok(()) => {
                        center = session.current_place().clone();
                        request = MapRequest::new();
                        record = match redraw(
                            ctx,
                            session,
                            &mut view,
                            &center,
                            &request,
                            record,
                            Ui {
                                charset,
                                keymap: &keymap,
                                waiting: &mut waiting,
                            },
                        )
                        .await
                        {
                            Awaited::Done(drawn) => drawn,
                            Awaited::Left => return Ok(()),
                        };
                    }
                    // A refusal from a movement is an answer to a key press, not a reason to
                    // take the screen away: `back` at the start of the trail says so and stays.
                    Err(error) => view.say(error.message().to_owned()),
                }
            }
            Effect::Refresh => {
                record = match redraw(
                    ctx,
                    session,
                    &mut view,
                    &center,
                    &request,
                    record,
                    Ui {
                        charset,
                        keymap: &keymap,
                        waiting: &mut waiting,
                    },
                )
                .await
                {
                    Awaited::Done(drawn) => drawn,
                    Awaited::Left => return Ok(()),
                };
            }
            Effect::ToggleLive => {
                let live = !view.is_live();
                view.set_live(live, "polled");
                refreshed = std::time::Instant::now();
            }
            Effect::Zoom(level) => {
                request = request.clone().zoom(level);
                record = match redraw(
                    ctx,
                    session,
                    &mut view,
                    &center,
                    &request,
                    record,
                    Ui {
                        charset,
                        keymap: &keymap,
                        waiting: &mut waiting,
                    },
                )
                .await
                {
                    Awaited::Done(drawn) => drawn,
                    Awaited::Left => return Ok(()),
                };
            }
            Effect::Inspect(node) => view.show_detail(detail(session, &node)),
            Effect::Pin(node) => {
                let said = pin(pins, session, &node, Timestamp::now());
                view.say(said);
            }
        }
    }
}

/// Projects the map again and hands it to the view, keeping the old drawing where the providers
/// refused: a view that blanked because one answer was late would be lying about the system.
async fn redraw(
    ctx: &Invocation<'_>,
    session: &mut SpatialSessionState,
    view: &mut MapView,
    center: &SpatialId,
    request: &MapRequest,
    previous: ono_value::RecordValue,
    ui: Ui<'_>,
) -> Awaited<ono_value::RecordValue> {
    let at = Timestamp::now();
    let observed = while_answering(
        crate::spatial::map::projection(ctx, session, center, request, at),
        ready_key,
        ui.keymap,
        ui.waiting,
    )
    .await;
    let Awaited::Done(observed) = observed else {
        return Awaited::Left;
    };
    let record = match observed {
        Ok(record) => record,
        Err(error) => {
            view.say(error.message().to_owned());
            previous
        }
    };
    view.set_place(place_path(session, center));
    view.redraw(&record, ui.charset);
    Awaited::Done(record)
}

/// Where the view is, as §21.2 spells a place: `local`, `local/compute`, `local/process/nginx`.
fn place_path(session: &SpatialSessionState, center: &SpatialId) -> String {
    ono_spatial_query::resolve::concise_path(session.index(), center)
}

/// A terminal that refused to be driven at all.
fn terminal_refused(error: std::io::Error) -> ErrorValue {
    ErrorValue::new(
        ErrorCode::SpatialUnsupported,
        format!("this terminal cannot show a full-screen map: {error}"),
    )
    .with_help("`map --json` and the text map need no terminal at all (spec v0.4 §23.2, §29.1)")
}

/// Records the movement `Enter` made, exactly as the `enter` command records it (§20.1).
fn enter(session: &mut SpatialSessionState, there: &SpatialId, now: Timestamp) {
    let here = session.current_place().clone();
    if &here != there {
        session.trail_mut().record(NavigationStep::new(
            now,
            here,
            there.clone(),
            Movement::Enter,
        ));
    }
}

/// Records the traversal `f` made, as `follow` records it (§6.4, §20.1).
///
/// The selected edge is one drawn edge, so §23.3's "when unambiguous" holds by construction:
/// there is exactly one relation and exactly one far end, and both travel with the step.
fn follow(session: &mut SpatialSessionState, relation: &str, there: &SpatialId, now: Timestamp) {
    let here = session.current_place().clone();
    if &here == there {
        return;
    }
    let mut step = NavigationStep::new(now, here, there.clone(), Movement::Follow)
        .spelled(relation.to_owned());
    if let Some(spec) = ono_spatial_core::relation::spec(relation) {
        step = step.along(spec.relation_type());
    }
    session.trail_mut().record(step);
}

/// Everything this session knows about a node, for the `i` overlay (§6.1's `inspect`).
fn detail(session: &SpatialSessionState, node: &str) -> Vec<String> {
    let Some(id) = SpatialId::parse(node) else {
        return vec!["  a cluster stands for many objects; expand it to inspect one".to_owned()];
    };
    if let Some(space) = ono_spatial_query::resolve::space_of(&id) {
        return vec![
            format!("  {}", space.label),
            String::new(),
            format!("  holds     {}", space.object_type.as_str()),
            format!(
                "  path      {}",
                ono_spatial_query::resolve::place_path(session.index(), &id)
            ),
            String::new(),
            "  a canonical space: declared geography, not an observed object".to_owned(),
        ];
    }
    let Some(entry) = session.index().get(&id) else {
        return vec!["  this session no longer knows that place".to_owned()];
    };
    let object = entry.object();
    let mut lines = vec![
        format!("  {}", object.display_name()),
        String::new(),
        format!("  type      {}", object.object_type().as_str()),
        format!("  id        {id}"),
        format!(
            "  path      {}",
            ono_spatial_query::resolve::place_path(session.index(), &id)
        ),
        format!("  scope     {}", object.scope()),
        format!("  provider  {}", object.provenance().provider()),
    ];
    let edges = entry.edges();
    if !edges.is_empty() {
        lines.push(String::new());
        lines.push("  relations".to_owned());
        for edge in edges {
            lines.push(format!(
                "    {:<16} {}",
                edge.relation().as_str(),
                edge.confidence().as_str()
            ));
        }
    }
    lines.push(String::new());
    lines.push("  any key returns to the map".to_owned());
    lines
}

/// Pins or unpins the focused place from inside the view (§20.4, §26.4, §23.3's `p`).
fn pin(
    store: Option<&crate::spatial::PinStore>,
    session: &mut SpatialSessionState,
    node: &str,
    now: Timestamp,
) -> String {
    let Some(id) = SpatialId::parse(node) else {
        return "a cluster stands for many objects, so it is not a place to pin".to_owned();
    };
    let Some(store) = store else {
        return "this session has no state directory, so a pin could not outlive it".to_owned();
    };
    match crate::spatial::pins::toggle_pin(store, session, &id, now) {
        Ok(said) => said,
        Err(error) => error.message().to_owned(),
    }
}

/// The key bindings in force: §23.3's table, with whatever the user rebound (§23.3's last line).
fn configured_keymap() -> Keymap {
    let mut keymap = Keymap::default_bindings();
    if let Some(overrides) = setting_text("spatial.map.keys")
        && !overrides.trim().is_empty()
        && let Err(problem) = keymap.apply_overrides(&overrides)
    {
        eprintln!(
            "{}: spatial.map.keys — {problem}; the default bindings are in force",
            ono_core::SHORT_NAME
        );
        return Keymap::default_bindings();
    }
    keymap
}

/// The editor's key press as the view's key.
fn translate(press: KeyPress) -> Option<Key> {
    let key = match press.code() {
        KeyCode::Char(character) if press.modifiers().has_ctrl() => {
            Key::Ctrl(character.to_ascii_lowercase())
        }
        KeyCode::Char(character) => Key::Char(character),
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::BackTab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Delete | KeyCode::Insert => return None,
    };
    Some(key)
}

/// The picker §27.2 opens when a selector names several places.
///
/// Returns the candidate the user chose, or `None` when they left it — in which case the caller
/// raises the same `spatial.ambiguous_selector` a script would have got, because a picker that
/// was dismissed answered nothing.
pub fn pick(selector: &str, candidates: &[Candidate]) -> Option<usize> {
    let mut chosen = 0usize;
    let _raw = RawMode::enter().ok()?;
    let mut out = std::io::stdout();
    loop {
        draw_picker(&mut out, selector, candidates, chosen);
        let press = ono_editor::read_key().ok()?;
        match press.code() {
            KeyCode::Char('c') if press.modifiers().has_ctrl() => {
                clear_picker(&mut out, candidates.len() + 2);
                return None;
            }
            KeyCode::Esc => {
                clear_picker(&mut out, candidates.len() + 2);
                return None;
            }
            KeyCode::Enter => {
                clear_picker(&mut out, candidates.len() + 2);
                return Some(chosen);
            }
            KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => {
                chosen = (chosen + 1).min(candidates.len().saturating_sub(1));
            }
            KeyCode::Up | KeyCode::BackTab | KeyCode::Char('k') => {
                chosen = chosen.saturating_sub(1);
            }
            _ => {}
        }
    }
}

/// Draws the picker where it stands, over its own previous drawing.
fn draw_picker(out: &mut std::io::Stdout, selector: &str, candidates: &[Candidate], chosen: usize) {
    use std::io::Write;
    let mut screen = String::new();
    screen.push_str(&format!(
        "\r\x1b[J`{selector}` names {} places — Up/Down to choose, Enter to go, Esc to stay\r\n",
        candidates.len()
    ));
    for (index, candidate) in candidates.iter().enumerate() {
        // §39.1: the focused item must be legible without colour, so the cursor is a character.
        let cursor = if index == chosen { '>' } else { ' ' };
        screen.push_str(&format!("{cursor} {}\r\n", candidate.row()));
    }
    // Back to the first line of the drawing, so the next frame paints over this one.
    screen.push_str(&format!("\x1b[{}A\r", candidates.len() + 1));
    let _ = out.write_all(screen.as_bytes());
    let _ = out.flush();
}

/// Wipes the picker off the screen once it has answered.
fn clear_picker(out: &mut std::io::Stdout, _rows: usize) {
    use std::io::Write;
    let _ = out.write_all(b"\r\x1b[J");
    let _ = out.flush();
}

fn map_mode() -> String {
    setting_text("spatial.map.mode").unwrap_or_else(|| "auto".to_owned())
}

/// Reads a `spatial.*` setting the session recorded before the first spatial command ran (§47).
fn setting_text(key: &str) -> Option<String> {
    crate::spatial::session::configured_text(key)
}

fn setting_flag(key: &str) -> bool {
    crate::spatial::session::configured_flag(key)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "a test states its preconditions directly (AGENTS.md section 16)"
    )]

    use super::*;

    /// v0.4 §34: a slow provider may not cost the user the key that gets them out. Without the
    /// race this never returns, which is what the timeout turns into a failure rather than a hang.
    #[tokio::test]
    async fn should_leave_the_view_when_the_closing_key_arrives_while_work_is_still_running() {
        let keymap = Keymap::default_bindings();
        let mut pressed = false;
        let answered = tokio::time::timeout(
            Duration::from_secs(2),
            while_answering(
                std::future::pending::<u8>(),
                || {
                    if pressed {
                        return None;
                    }
                    pressed = true;
                    Some(Key::Esc)
                },
                &keymap,
                &mut VecDeque::new(),
            ),
        )
        .await;
        assert_eq!(
            answered,
            Ok(Awaited::Left),
            "Esc must close a view whose observation never finishes"
        );
    }

    #[tokio::test]
    async fn should_answer_with_the_work_when_no_key_interrupts_it() {
        let keymap = Keymap::default_bindings();
        assert_eq!(
            while_answering(async { 7_u8 }, || None, &keymap, &mut VecDeque::new()).await,
            Awaited::Done(7)
        );
    }

    #[tokio::test]
    async fn should_not_let_leaving_overtake_a_key_the_user_pressed_first() {
        // `Enter`, `Backspace`, `Esc` typed without pausing: the map goes back and *then* closes.
        // Closing first would drop the movement the user asked for (v0.4 §43.4).
        let keymap = Keymap::default_bindings();
        let mut typed = [Key::Backspace, Key::Esc].into_iter();
        let mut waiting = VecDeque::new();
        let work = async {
            tokio::time::sleep(Duration::from_millis(120)).await;
            1_u8
        };
        assert_eq!(
            while_answering(work, || typed.next(), &keymap, &mut waiting).await,
            Awaited::Done(1)
        );
        assert_eq!(
            waiting.into_iter().collect::<Vec<_>>(),
            vec![Key::Backspace, Key::Esc]
        );
    }

    #[tokio::test]
    async fn should_keep_a_key_that_does_not_close_the_view_for_the_loop_to_answer() {
        // Typing `Enter` and then `Backspace` without pausing means both. The observation the
        // first one started must not swallow the second: it waits, in the order it was typed.
        let keymap = Keymap::default_bindings();
        let mut typed = [Key::Enter, Key::Backspace].into_iter();
        let mut waiting = VecDeque::new();
        let work = async {
            tokio::time::sleep(Duration::from_millis(120)).await;
            3_u8
        };
        assert_eq!(
            while_answering(work, || typed.next(), &keymap, &mut waiting).await,
            Awaited::Done(3)
        );
        assert_eq!(
            waiting.into_iter().collect::<Vec<_>>(),
            vec![Key::Enter, Key::Backspace],
            "a key pressed during an observation is answered after it, not lost"
        );
    }
}
