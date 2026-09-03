//! How much presentation a destination can receive.
//!
//! Spec §4.6 describes progressive enhancement rather than a mode switch:
//!
//! ```text
//! TTY:      rich table + selection + drill-down
//! pipe:     stream of structured values
//! redirect: deterministic serialization or explicit text rendering
//! script:   no hidden terminal interaction
//! ```
//!
//! An operation MUST NOT behave differently *semantically* because a table happens to be
//! interactive. This type decides only how much decoration a destination gets.

/// What the destination of this output can usefully receive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Presentation {
    /// A capable terminal: colour, and interaction where a command offers it.
    Terminal,
    /// A terminal the user asked to keep plain, or one that cannot do better.
    Plain,
    /// Another process. Structure, no cursor control.
    Pipe,
    /// A file. Deterministic bytes.
    Redirect,
    /// A script or a non-interactive invocation: never a hidden terminal interaction (§17.4).
    Script,
}

impl Presentation {
    /// Chooses a presentation from whether the destination is a terminal and the environment.
    ///
    /// The environment is passed in rather than read, so the choice is testable and so a
    /// non-interactive invocation can decide for itself.
    ///
    /// ```
    /// use ono_render::Presentation;
    /// assert_eq!(Presentation::choose(false, &[]), Presentation::Pipe);
    /// assert_eq!(Presentation::choose(true, &[("NO_COLOR", "1")]), Presentation::Plain);
    /// ```
    #[must_use]
    pub fn choose(is_terminal: bool, environment: &[(&str, &str)]) -> Self {
        if !is_terminal {
            return Presentation::Pipe;
        }
        let lookup = |name: &str| {
            environment
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| *value)
        };
        // NO_COLOR is honoured whenever it is present, whatever its value, as the convention
        // specifies. A user who asks for plain output is not overruled by a capable terminal.
        if lookup("NO_COLOR").is_some() {
            return Presentation::Plain;
        }
        match lookup("TERM") {
            Some("dumb") | Some("") => Presentation::Plain,
            _ => Presentation::Terminal,
        }
    }

    /// Whether this destination should receive colour.
    #[must_use]
    pub const fn allows_color(self) -> bool {
        matches!(self, Presentation::Terminal)
    }

    /// Whether a person is reading this and cannot be shown colour (spec §44, ADR-0558).
    ///
    /// A `Plain` destination is a terminal the user asked to keep plain or one that cannot do
    /// better: someone is reading it, and every meaning a theme would have carried in a hue has
    /// to arrive some other way. That is what a token's marker is for.
    ///
    /// A pipe, a redirect and a script are not people. There the structure is the answer, the
    /// bytes must not depend on which theme happens to be configured, and a mark in the middle
    /// of them would be noise a reader downstream has to strip.
    ///
    /// ```
    /// use ono_render::Presentation;
    /// assert!(Presentation::Plain.marks());
    /// assert!(!Presentation::Pipe.marks());
    /// assert!(!Presentation::Terminal.marks());
    /// ```
    #[must_use]
    pub const fn marks(self) -> bool {
        matches!(self, Presentation::Plain)
    }

    /// Whether a command may offer an interactive selection here (spec §13.5).
    #[must_use]
    pub const fn allows_interaction(self) -> bool {
        matches!(self, Presentation::Terminal)
    }

    /// Whether output here must be byte-for-byte reproducible (spec §4.6, §50).
    #[must_use]
    pub const fn must_be_deterministic(self) -> bool {
        !matches!(self, Presentation::Terminal)
    }
}
