//! Walking through what was run.

use crate::entry::Entry;

/// Which way a recall moves through history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Towards what was run longer ago — what Up and Ctrl-P do.
    Older,
    /// Back towards the line being typed.
    Newer,
}

/// A position in history, and the filter a recall is anchored to.
///
/// The cursor starts past the newest entry, on the line the user is typing. Stepping newer from
/// the newest entry returns there rather than wrapping, because a recall that wraps around to the
/// oldest command is a recall nobody asked for.
#[derive(Debug, Clone)]
pub struct Cursor<'history> {
    entries: &'history [Entry],
    position: usize,
    prefix: String,
}

impl<'history> Cursor<'history> {
    pub(crate) fn new(entries: &'history [Entry]) -> Self {
        Self {
            entries,
            position: entries.len(),
            prefix: String::new(),
        }
    }

    /// Anchors the recall to entries starting with `prefix` (what Up does on a non-empty line).
    #[must_use]
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self.position = self.entries.len();
        self
    }

    /// Moves one matching entry in `direction` and returns its text, or `None` at the end.
    ///
    /// At the oldest matching entry, a further step older stays put and returns `None`, so a
    /// user leaning on the key does not silently lose their place.
    pub fn step(&mut self, direction: Direction) -> Option<&'history str> {
        match direction {
            Direction::Older => {
                let mut index = self.position;
                while index > 0 {
                    index -= 1;
                    if self.matches(index) {
                        self.position = index;
                        return self.entries.get(index).map(Entry::command_text);
                    }
                }
                None
            }
            Direction::Newer => {
                let mut index = self.position + 1;
                while index < self.entries.len() {
                    if self.matches(index) {
                        self.position = index;
                        return self.entries.get(index).map(Entry::command_text);
                    }
                    index += 1;
                }
                self.position = self.entries.len();
                None
            }
        }
    }

    /// The entry the cursor is on, if it is on one rather than on the live line.
    #[must_use]
    pub fn current(&self) -> Option<&'history Entry> {
        self.entries.get(self.position)
    }

    fn matches(&self, index: usize) -> bool {
        self.entries
            .get(index)
            .is_some_and(|entry| entry.command_text().starts_with(&self.prefix))
    }
}
