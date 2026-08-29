//! The full-screen map view: a viewport, a cursor and the semantics of §23.3's keys.
//!
//! Everything here is decided without a terminal. The view is handed the same
//! `ono.spatial-map/1` record the textual map draws — already ranked, bounded and clustered by
//! `ono-spatial-query` — and adds three things the text projection has no need for: which lines
//! are visible, which node the cursor is on, and what a key press means. It selects nothing,
//! ranks nothing and invents nothing (§45.4, §49.5); a view that re-decided any of that would
//! disagree with `map` about what the system looks like.
//!
//! Two rules from the specification shape the whole module:
//!
//! - **§23.4: moving focus MUST NOT change the shell's current place.** So [`MapView::apply`]
//!   answers with an [`Effect`], and only `Enter`, `Follow`, `Back`, `Up` and `Home` produce an
//!   effect the shell acts on. Every other key changes the view and nothing else — the same
//!   distinction §8.3 draws between expanding a cluster and entering a child.
//! - **§23.3: "Key bindings MUST be configurable. Semantic actions are normative; exact
//!   single-key choices MAY be remapped."** So [`Action`] is the normative half and [`Keymap`]
//!   the configurable one, and [`Keymap::default_bindings`] is exactly the table §23.3 prints.
//!
//! §39.1 forbids colour from being the only carrier of six distinctions, one of which is the
//! focused item. The cursor is therefore a character in the left margin, present in every
//! rendering including a monochrome one.

use ono_value::RecordValue;

use crate::map::{Charset, MapLine, map_lines};

/// A key press, described without reference to any terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A printable character.
    Char(char),
    /// A character with Control held.
    Ctrl(char),
    /// Return.
    Enter,
    /// Escape.
    Esc,
    /// Tab.
    Tab,
    /// Shift-Tab, which terminals report as a key of its own.
    BackTab,
    /// Backspace.
    Backspace,
    /// Cursor up.
    Up,
    /// Cursor down.
    Down,
    /// Cursor left.
    Left,
    /// Cursor right.
    Right,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
}

impl Key {
    /// The key a configuration file names, in the spelling [`Keymap::describe`] writes back.
    ///
    /// # Errors
    ///
    /// The word itself, when it names no key.
    pub fn parse(word: &str) -> Result<Self, String> {
        let word = word.trim();
        if let Some(rest) = word
            .strip_prefix("Ctrl-")
            .or_else(|| word.strip_prefix("ctrl-"))
            && let Some(character) = rest.chars().next()
            && rest.chars().count() == 1
        {
            return Ok(Key::Ctrl(character.to_ascii_lowercase()));
        }
        let named = match word.to_ascii_lowercase().as_str() {
            "enter" | "return" => Some(Key::Enter),
            "esc" | "escape" => Some(Key::Esc),
            "tab" => Some(Key::Tab),
            "shift-tab" | "backtab" => Some(Key::BackTab),
            "backspace" => Some(Key::Backspace),
            "up" => Some(Key::Up),
            "down" => Some(Key::Down),
            "left" => Some(Key::Left),
            "right" => Some(Key::Right),
            "home" => Some(Key::Home),
            "end" => Some(Key::End),
            "pageup" => Some(Key::PageUp),
            "pagedown" => Some(Key::PageDown),
            "space" => Some(Key::Char(' ')),
            _ => None,
        };
        if let Some(key) = named {
            return Ok(key);
        }
        let mut characters = word.chars();
        match (characters.next(), characters.next()) {
            (Some(character), None) => Ok(Key::Char(character)),
            _ => Err(word.to_owned()),
        }
    }

    /// How the key is written in the help line and in a configuration file.
    #[must_use]
    pub fn describe(self) -> String {
        match self {
            Key::Char(' ') => "Space".to_owned(),
            Key::Char(character) => character.to_string(),
            Key::Ctrl(character) => format!("Ctrl-{character}"),
            Key::Enter => "Enter".to_owned(),
            Key::Esc => "Esc".to_owned(),
            Key::Tab => "Tab".to_owned(),
            Key::BackTab => "Shift-Tab".to_owned(),
            Key::Backspace => "Backspace".to_owned(),
            Key::Up => "Up".to_owned(),
            Key::Down => "Down".to_owned(),
            Key::Left => "Left".to_owned(),
            Key::Right => "Right".to_owned(),
            Key::Home => "Home".to_owned(),
            Key::End => "End".to_owned(),
            Key::PageUp => "PageUp".to_owned(),
            Key::PageDown => "PageDown".to_owned(),
        }
    }
}

/// The semantic actions §23.3 makes normative. The keys that reach them are configurable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    /// Move focus to the next visible node.
    FocusNext,
    /// Move focus to the previous visible node.
    FocusPrevious,
    /// Move focus a screen forward.
    FocusPageDown,
    /// Move focus a screen back.
    FocusPageUp,
    /// Move focus to the first visible node — the place the map is centred on.
    FocusFirst,
    /// Move focus to the last visible node.
    FocusLast,
    /// Enter the focused node. The one key that moves the shell (§23.4).
    Enter,
    /// Follow the focused relationship edge (§6.4).
    Follow,
    /// Back along the navigation history (§6.6).
    Back,
    /// Up the canonical hierarchy (§6.6).
    Up,
    /// Home, to the root place (§6.6).
    Home,
    /// Search the drawn map.
    Search,
    /// One zoom level closer (§8.1).
    ZoomIn,
    /// One zoom level further out (§8.1).
    ZoomOut,
    /// Cycle through the zoom levels.
    ZoomCycle,
    /// Ask the providers again (§33.2).
    Refresh,
    /// Turn the live subscription on or off (§25.1).
    ToggleLive,
    /// Show everything known about the focused object (§6.1's `inspect`).
    Inspect,
    /// Pin or unpin the focused place (§20.4).
    Pin,
    /// Close the view, preserving the current place (§23.3).
    Close,
    /// Show the key table (§23.3's `?`).
    Help,
}

impl Action {
    /// Every action, in the order §23.3's table lists them.
    pub const ALL: [Action; 21] = [
        Action::FocusNext,
        Action::FocusPrevious,
        Action::FocusPageDown,
        Action::FocusPageUp,
        Action::FocusFirst,
        Action::FocusLast,
        Action::Enter,
        Action::Follow,
        Action::Back,
        Action::Up,
        Action::Home,
        Action::Search,
        Action::ZoomIn,
        Action::ZoomOut,
        Action::ZoomCycle,
        Action::Refresh,
        Action::ToggleLive,
        Action::Inspect,
        Action::Pin,
        Action::Close,
        Action::Help,
    ];

    /// The name a configuration file uses for the action.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Action::FocusNext => "focus-next",
            Action::FocusPrevious => "focus-previous",
            Action::FocusPageDown => "focus-page-down",
            Action::FocusPageUp => "focus-page-up",
            Action::FocusFirst => "focus-first",
            Action::FocusLast => "focus-last",
            Action::Enter => "enter",
            Action::Follow => "follow",
            Action::Back => "back",
            Action::Up => "up",
            Action::Home => "home",
            Action::Search => "search",
            Action::ZoomIn => "zoom-in",
            Action::ZoomOut => "zoom-out",
            Action::ZoomCycle => "zoom",
            Action::Refresh => "refresh",
            Action::ToggleLive => "live",
            Action::Inspect => "inspect",
            Action::Pin => "pin",
            Action::Close => "close",
            Action::Help => "help",
        }
    }

    /// The action a configuration file names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Action::ALL
            .into_iter()
            .find(|action| action.name() == name.trim())
    }

    /// How the action reads in the help table.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Action::FocusNext => "next node",
            Action::FocusPrevious => "previous node",
            Action::FocusPageDown => "a screen forward",
            Action::FocusPageUp => "a screen back",
            Action::FocusFirst => "the place itself",
            Action::FocusLast => "the last node",
            Action::Enter => "enter the focused node",
            Action::Follow => "follow the focused relation",
            Action::Back => "back along the trail",
            Action::Up => "up the hierarchy",
            Action::Home => "home to SYSTEM",
            Action::Search => "search the drawn map",
            Action::ZoomIn => "zoom in",
            Action::ZoomOut => "zoom out",
            Action::ZoomCycle => "cycle the zoom level",
            Action::Refresh => "ask the providers again",
            Action::ToggleLive => "live updates on or off",
            Action::Inspect => "inspect the focused object",
            Action::Pin => "pin or unpin",
            Action::Close => "close, keeping the place",
            Action::Help => "this table",
        }
    }
}

/// Which key means which action (§23.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keymap {
    bindings: Vec<(Key, Action)>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::default_bindings()
    }
}

impl Keymap {
    /// The table §23.3 prints, key for key.
    #[must_use]
    pub fn default_bindings() -> Self {
        let bindings = vec![
            (Key::Down, Action::FocusNext),
            (Key::Char('j'), Action::FocusNext),
            (Key::Right, Action::FocusNext),
            (Key::Char('l'), Action::FocusNext),
            (Key::Tab, Action::FocusNext),
            (Key::Up, Action::FocusPrevious),
            (Key::Char('k'), Action::FocusPrevious),
            (Key::Left, Action::FocusPrevious),
            (Key::BackTab, Action::FocusPrevious),
            (Key::PageDown, Action::FocusPageDown),
            (Key::PageUp, Action::FocusPageUp),
            (Key::Home, Action::FocusFirst),
            (Key::End, Action::FocusLast),
            (Key::Enter, Action::Enter),
            (Key::Char('f'), Action::Follow),
            (Key::Char('b'), Action::Back),
            (Key::Backspace, Action::Back),
            (Key::Char('u'), Action::Up),
            (Key::Char('h'), Action::Home),
            (Key::Char('/'), Action::Search),
            (Key::Char('+'), Action::ZoomIn),
            (Key::Char('-'), Action::ZoomOut),
            (Key::Char('z'), Action::ZoomCycle),
            (Key::Char('r'), Action::Refresh),
            (Key::Char('w'), Action::ToggleLive),
            (Key::Char('i'), Action::Inspect),
            (Key::Char('p'), Action::Pin),
            (Key::Esc, Action::Close),
            (Key::Ctrl('c'), Action::Close),
            (Key::Char('?'), Action::Help),
        ];
        Self { bindings }
    }

    /// Binds `key` to `action`, replacing whatever that key meant before.
    pub fn bind(&mut self, key: Key, action: Action) {
        self.bindings.retain(|(bound, _)| *bound != key);
        self.bindings.push((key, action));
    }

    /// What `key` means here.
    #[must_use]
    pub fn action(&self, key: Key) -> Option<Action> {
        self.bindings
            .iter()
            .find(|(bound, _)| *bound == key)
            .map(|(_, action)| *action)
    }

    /// The keys bound to `action`, in the order they were bound.
    #[must_use]
    pub fn keys_for(&self, action: Action) -> Vec<Key> {
        self.bindings
            .iter()
            .filter(|(_, bound)| *bound == action)
            .map(|(key, _)| *key)
            .collect()
    }

    /// Applies the overrides a user configured: `close=q, enter=Enter, home=g`.
    ///
    /// Each entry names a normative action and the key that is to reach it; the key stops
    /// meaning whatever it meant before, and the action keeps every other key it had, so a
    /// partial configuration never leaves an action unreachable.
    ///
    /// # Errors
    ///
    /// The offending word, when an entry names no action or no key.
    pub fn apply_overrides(&mut self, spec: &str) -> Result<(), String> {
        for entry in spec.split(',').map(str::trim).filter(|e| !e.is_empty()) {
            let (name, keys) = entry
                .split_once('=')
                .ok_or_else(|| format!("`{entry}` is not `<action>=<key>`"))?;
            let action = Action::from_name(name)
                .ok_or_else(|| format!("`{}` is not a map action", name.trim()))?;
            for word in keys.split_whitespace() {
                let key = Key::parse(word).map_err(|word| format!("`{word}` is not a key"))?;
                self.bind(key, action);
            }
        }
        Ok(())
    }

    /// The table the `?` overlay prints, one line per action.
    #[must_use]
    pub fn describe(&self) -> Vec<String> {
        Action::ALL
            .into_iter()
            .map(|action| {
                let keys: Vec<String> = self
                    .keys_for(action)
                    .into_iter()
                    .map(Key::describe)
                    .collect();
                format!("  {:<20} {}", keys.join(" / "), action.describe())
            })
            .collect()
    }
}

/// What the shell must do because of a key press. Everything else the view did itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// The view changed, or nothing happened. Redraw and read the next key.
    Stay,
    /// Enter this node — the only key that moves the current place (§23.4).
    Enter(String),
    /// Follow this relationship edge: its relation, and the place at the far end (§6.4).
    Follow {
        /// The relation the drawn edge carries.
        relation: String,
        /// The place the edge points at.
        node: String,
    },
    /// Back along the trail (§6.6).
    Back,
    /// Up the canonical hierarchy (§6.6).
    Up,
    /// Home (§6.6).
    Home,
    /// Ask the providers again and redraw (§33.2).
    Refresh,
    /// Turn the live subscription on or off (§25.1).
    ToggleLive,
    /// Redraw at this zoom level (§8.1).
    Zoom(u8),
    /// Show everything known about this node (§6.1).
    Inspect(String),
    /// Pin or unpin this node (§20.4).
    Pin(String),
    /// Leave the view, keeping the current place (§23.3).
    Close,
}

/// What the view is showing instead of the map, if anything.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Overlay {
    None,
    Help,
    Detail(Vec<String>),
}

/// The full-screen projection of one `SpatialMap`: a viewport, a cursor and a footer.
#[derive(Debug, Clone)]
pub struct MapView {
    heading: String,
    place: String,
    lines: Vec<MapLine>,
    focus: Option<usize>,
    top: usize,
    width: usize,
    height: usize,
    zoom: u8,
    live: bool,
    freshness: String,
    searching: Option<String>,
    query: Option<String>,
    overlay: Overlay,
    status: Option<String>,
    keymap: Keymap,
}

/// How many rows the header and the footer take from the body.
const CHROME: usize = 2;

impl MapView {
    /// A view of `map`, drawn into a terminal `width` by `height`.
    #[must_use]
    pub fn new(
        map: &RecordValue,
        width: usize,
        height: usize,
        charset: Charset,
        keymap: Keymap,
    ) -> Self {
        let mut view = Self {
            heading: String::new(),
            place: String::new(),
            lines: Vec::new(),
            focus: None,
            top: 0,
            width: width.max(20),
            height: height.max(4),
            zoom: 0,
            live: false,
            freshness: String::new(),
            searching: None,
            query: None,
            overlay: Overlay::None,
            status: None,
            keymap,
        };
        view.absorb(map, charset);
        view.focus = view.first_focusable();
        view.scroll_into_view();
        view
    }

    /// Replaces the drawing with a new projection of the same or a new place.
    ///
    /// The cursor stays on the node it was on where that node is still drawn, which is what
    /// §43.4 means by "terminal resize preserves current place and focus where possible" and
    /// what a refresh of a live map needs in order not to jump under the user's hand.
    pub fn redraw(&mut self, map: &RecordValue, charset: Charset) {
        let held = self.focused_node().map(str::to_owned);
        self.absorb(map, charset);
        self.focus = held
            .and_then(|node| {
                self.lines
                    .iter()
                    .position(|line| line.node() == Some(node.as_str()))
            })
            .or_else(|| self.first_focusable());
        self.scroll_into_view();
    }

    /// Draws the same map into a terminal of a new size (§43.4, §39.3).
    pub fn resize(&mut self, map: &RecordValue, width: usize, height: usize, charset: Charset) {
        self.width = width.max(20);
        self.height = height.max(4);
        self.redraw(map, charset);
    }

    fn absorb(&mut self, map: &RecordValue, charset: Charset) {
        // The cursor lives in the left margin, so the drawing is asked for two columns less than
        // the terminal has (§39.1: the focused item is legible without colour).
        let mut lines = map_lines(map, self.width.saturating_sub(2).max(20), charset);
        // The text map opens with its own heading; here that line is the header row, so it is
        // taken out of the body rather than drawn twice.
        self.heading = if lines.first().is_some_and(|line| line.node().is_none()) {
            lines.remove(0).text().trim().to_owned()
        } else {
            String::new()
        };
        self.lines = lines;
        self.zoom = match map.get("zoom_level") {
            Some(ono_value::Value::Int(level)) => u8::try_from(*level).unwrap_or(0),
            _ => self.zoom,
        };
        self.freshness = match map.get("freshness") {
            Some(ono_value::Value::String(text)) => text.to_string(),
            _ => String::new(),
        };
    }

    /// The node the cursor is on, if the map drew any.
    #[must_use]
    pub fn focused_node(&self) -> Option<&str> {
        self.focus
            .and_then(|at| self.lines.get(at))
            .and_then(MapLine::node)
    }

    /// The relation the cursor is on, where the cursor is on a relationship edge.
    #[must_use]
    pub fn focused_relation(&self) -> Option<&str> {
        self.focus
            .and_then(|at| self.lines.get(at))
            .and_then(MapLine::relation)
    }

    /// Whether the view is subscribed to change (§25.1).
    #[must_use]
    pub const fn is_live(&self) -> bool {
        self.live
    }

    /// Records the place path the shell is standing at, as §21.2 spells it.
    ///
    /// The map's own heading names the *label* of the place it drew; this names where that is —
    /// `local/compute` — which is what turns a drawing into an orientation (§21.1, §23.1). The
    /// shell supplies it because a `SpatialMap` carries no rendered path and §22 keeps it that
    /// way.
    pub fn set_place(&mut self, place: impl Into<String>) {
        self.place = place.into();
    }

    /// The heading line of the drawn map, which the header row shows.
    #[must_use]
    pub fn heading(&self) -> &str {
        &self.heading
    }

    /// Records that the subscription was turned on or off, and how it is being kept current.
    ///
    /// The word is §25.3's vocabulary — `event_driven`, `polled`, `cached`, `stale`, `partial` —
    /// because a live view "MUST expose whether updates are" one of those, and a view that said
    /// `live` without saying how would be claiming more than it knows.
    pub fn set_live(&mut self, live: bool, freshness: &str) {
        self.live = live;
        freshness.clone_into(&mut self.freshness);
    }

    /// Says something in the footer until the next key press.
    pub fn say(&mut self, message: impl Into<String>) {
        self.status = Some(message.into());
    }

    /// Shows `lines` over the map until the next Esc — the answer to `i` (§6.1).
    pub fn show_detail(&mut self, lines: Vec<String>) {
        self.overlay = Overlay::Detail(lines);
    }

    /// What a key press means here, and what is left for the shell to do.
    pub fn apply(&mut self, key: Key) -> Effect {
        self.status = None;

        // A search is a line editor, so while it is open the keys are text and not commands.
        if let Some(query) = self.searching.as_mut() {
            match key {
                Key::Char(character) => {
                    query.push(character);
                    return Effect::Stay;
                }
                Key::Backspace => {
                    query.pop();
                    return Effect::Stay;
                }
                Key::Enter => {
                    let query = self.searching.take().unwrap_or_default();
                    self.seek(&query);
                    return Effect::Stay;
                }
                Key::Esc => {
                    self.searching = None;
                    return Effect::Stay;
                }
                // §43.4 makes Ctrl-C the way out of a live view, and a half-typed search must
                // not be the one place it stops working.
                Key::Ctrl('c') => return Effect::Close,
                _ => return Effect::Stay,
            }
        }

        if self.overlay != Overlay::None {
            // Any key closes an overlay; the one that closes the view still closes the view, so
            // Esc out of help does not need a second press to leave.
            let closing = self.keymap.action(key) == Some(Action::Close);
            self.overlay = Overlay::None;
            if closing && key == Key::Ctrl('c') {
                return Effect::Close;
            }
            return Effect::Stay;
        }

        let Some(action) = self.keymap.action(key) else {
            return Effect::Stay;
        };
        match action {
            Action::FocusNext => {
                self.move_focus(1);
                Effect::Stay
            }
            Action::FocusPrevious => {
                self.move_focus(-1);
                Effect::Stay
            }
            Action::FocusPageDown => {
                self.move_focus(self.body_height().try_into().unwrap_or(1));
                Effect::Stay
            }
            Action::FocusPageUp => {
                let page: isize = self.body_height().try_into().unwrap_or(1);
                self.move_focus(-page);
                Effect::Stay
            }
            Action::FocusFirst => {
                self.focus = self.first_focusable();
                self.scroll_into_view();
                Effect::Stay
            }
            Action::FocusLast => {
                self.focus = self
                    .lines
                    .iter()
                    .rposition(|line| line.node().is_some())
                    .or(self.focus);
                self.scroll_into_view();
                Effect::Stay
            }
            Action::Enter => self
                .focused_node()
                .map(|node| Effect::Enter(node.to_owned()))
                .unwrap_or(Effect::Stay),
            Action::Follow => match (self.focused_relation(), self.focused_node()) {
                (Some(relation), Some(node)) => Effect::Follow {
                    relation: relation.to_owned(),
                    node: node.to_owned(),
                },
                _ => {
                    self.say("no relation is focused — move to a line under `relations`");
                    Effect::Stay
                }
            },
            Action::Back => Effect::Back,
            Action::Up => Effect::Up,
            Action::Home => Effect::Home,
            Action::Search => {
                self.searching = Some(String::new());
                Effect::Stay
            }
            Action::ZoomIn => Effect::Zoom(self.zoom.saturating_sub(1)),
            Action::ZoomOut => Effect::Zoom(self.zoom.saturating_add(1).min(MAX_ZOOM)),
            Action::ZoomCycle => Effect::Zoom(if self.zoom >= MAX_ZOOM {
                0
            } else {
                self.zoom + 1
            }),
            Action::Refresh => Effect::Refresh,
            Action::ToggleLive => Effect::ToggleLive,
            Action::Inspect => self
                .focused_node()
                .map(|node| Effect::Inspect(node.to_owned()))
                .unwrap_or(Effect::Stay),
            Action::Pin => self
                .focused_node()
                .map(|node| Effect::Pin(node.to_owned()))
                .unwrap_or(Effect::Stay),
            Action::Close => Effect::Close,
            Action::Help => {
                self.overlay = Overlay::Help;
                Effect::Stay
            }
        }
    }

    /// The screen, exactly `height` lines of at most `width` columns.
    #[must_use]
    pub fn frame(&self) -> Vec<String> {
        let mut frame = Vec::with_capacity(self.height);
        frame.push(self.header());

        let body = self.body_height();
        match &self.overlay {
            Overlay::Help => {
                let mut help = vec!["  keys — every one of them configurable".to_owned()];
                help.extend(self.keymap.describe());
                for index in 0..body {
                    frame.push(clip(help.get(index).map_or("", String::as_str), self.width));
                }
            }
            Overlay::Detail(lines) => {
                for index in 0..body {
                    frame.push(clip(
                        lines.get(index).map_or("", String::as_str),
                        self.width,
                    ));
                }
            }
            Overlay::None => {
                for index in 0..body {
                    let at = self.top + index;
                    let Some(line) = self.lines.get(at) else {
                        frame.push(String::new());
                        continue;
                    };
                    let cursor = if Some(at) == self.focus { '>' } else { ' ' };
                    frame.push(clip(&format!("{cursor} {}", line.text()), self.width));
                }
            }
        }

        frame.push(self.footer());
        frame
    }

    fn header(&self) -> String {
        // Where you are first, then what is drawn: §21.1's components in the order the prompt
        // itself puts them, so the view and the prompt read as one sentence.
        let mut line = String::new();
        if !self.place.is_empty() {
            line.push_str(&self.place);
            line.push_str("  ");
        }
        line.push_str(&self.heading);
        if self.live {
            line.push_str("  live");
            if !self.freshness.is_empty() {
                // §25.3: a live view MUST say whether its updates are event driven, polled,
                // cached, stale or partial. This is where it says it.
                line.push_str(&format!("  {}", self.freshness.replace('_', " ")));
            }
        }
        clip(&format!(" {line}"), self.width)
    }

    fn footer(&self) -> String {
        if let Some(query) = &self.searching {
            return clip(&format!(" /{query}"), self.width);
        }
        if let Some(status) = &self.status {
            return clip(&format!(" {status}"), self.width);
        }
        if let Some(query) = &self.query {
            return clip(
                &format!(" /{query}   {}  help", self.key_hint(Action::Help)),
                self.width,
            );
        }
        let hint = format!(
            " {} enter  {} back  {} up  {} close  {} help",
            self.key_hint(Action::Enter),
            self.key_hint(Action::Back),
            self.key_hint(Action::Up),
            self.key_hint(Action::Close),
            self.key_hint(Action::Help),
        );
        clip(&hint, self.width)
    }

    fn key_hint(&self, action: Action) -> String {
        self.keymap
            .keys_for(action)
            .first()
            .map_or_else(|| "—".to_owned(), |key| key.describe())
    }

    const fn body_height(&self) -> usize {
        self.height.saturating_sub(CHROME)
    }

    fn first_focusable(&self) -> Option<usize> {
        self.lines.iter().position(|line| line.node().is_some())
    }

    /// Moves the cursor `step` focusable lines on, clamped at either end.
    fn move_focus(&mut self, step: isize) {
        let focusable: Vec<usize> = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.node().is_some())
            .map(|(at, _)| at)
            .collect();
        if focusable.is_empty() {
            return;
        }
        let current = self
            .focus
            .and_then(|at| focusable.iter().position(|line| *line == at))
            .unwrap_or(0);
        let moved = isize::try_from(current).unwrap_or(0).saturating_add(step);
        let last = isize::try_from(focusable.len().saturating_sub(1)).unwrap_or(0);
        let clamped = usize::try_from(moved.clamp(0, last)).unwrap_or(0);
        self.focus = focusable.get(clamped).copied();
        self.scroll_into_view();
    }

    /// Moves the cursor to the next line whose text contains `query` (§23.3's `/`).
    fn seek(&mut self, query: &str) {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            self.query = None;
            return;
        }
        let start = self.focus.map_or(0, |at| at + 1);
        let found = self
            .lines
            .iter()
            .enumerate()
            .skip(start)
            .chain(self.lines.iter().enumerate().take(start))
            .find(|(_, line)| line.node().is_some() && line.text().to_lowercase().contains(&needle))
            .map(|(at, _)| at);
        match found {
            Some(at) => {
                self.focus = Some(at);
                self.query = Some(needle);
                self.scroll_into_view();
            }
            None => {
                self.query = None;
                self.say(format!("nothing drawn here matches `{needle}`"));
            }
        }
    }

    fn scroll_into_view(&mut self) {
        let body = self.body_height().max(1);
        let Some(focus) = self.focus else {
            self.top = 0;
            return;
        };
        if focus < self.top {
            self.top = focus;
        } else if focus >= self.top + body {
            self.top = focus + 1 - body;
        }
        let last_top = self.lines.len().saturating_sub(body);
        self.top = self.top.min(last_top);
    }
}

/// The deepest zoom level §8.1 defines, mirrored here so the view can clamp without asking the
/// query crate, which it does not depend on.
const MAX_ZOOM: u8 = 4;

/// Cuts a line to `width` whole characters, so nothing is drawn past the right edge (§39.3).
fn clip(line: &str, width: usize) -> String {
    if line.chars().count() <= width {
        return line.trim_end().to_owned();
    }
    line.chars()
        .take(width)
        .collect::<String>()
        .trim_end()
        .to_owned()
}
