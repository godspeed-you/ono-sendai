//! What the shell will and will not remember.

use regex::Regex;

/// The default number of entries kept. Large enough that a year of work stays recallable, small
/// enough that reading it costs nothing at startup (spec §34).
const DEFAULT_MAX_ENTRIES: usize = 20_000;

/// What appears in place of a redacted value.
const REDACTED: &str = "<redacted>";

/// The rules the shell applies before a command becomes an entry.
#[derive(Debug, Clone)]
pub struct Policy {
    max_entries: usize,
    collapse_repeats: bool,
    hide_leading_space: bool,
    redactions: Vec<Regex>,
}

impl Policy {
    /// Keeps at most `max_entries`, discarding the oldest.
    #[must_use]
    pub fn max_entries(mut self, max_entries: usize) -> Self {
        self.max_entries = max_entries.max(1);
        self
    }

    /// Collapses a command repeated immediately after itself.
    #[must_use]
    pub fn collapse_repeats(mut self, collapse: bool) -> Self {
        self.collapse_repeats = collapse;
        self
    }

    /// Whether a command starting with a space is remembered at all.
    ///
    /// On by default: it is the convention every shell user already knows for "do not remember
    /// this", and it is the cheapest secret-aware policy there is (spec §17.5).
    #[must_use]
    pub fn hide_leading_space(mut self, hide: bool) -> Self {
        self.hide_leading_space = hide;
        self
    }

    /// Replaces the last capture group of each pattern before the command is stored.
    ///
    /// Patterns that do not compile are dropped rather than failing the shell's startup: a
    /// mistyped pattern must not cost the user their history, and a redaction that silently did
    /// not apply is reported through `get config --problems` (ADR-0010).
    #[must_use]
    pub fn redacting<I, S>(mut self, patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.redactions = patterns
            .into_iter()
            .filter_map(|pattern| Regex::new(pattern.as_ref()).ok())
            .collect();
        self
    }

    /// The entry limit.
    #[must_use]
    pub fn entry_limit(&self) -> usize {
        self.max_entries
    }

    /// Whether `command` should be remembered at all.
    #[must_use]
    pub fn should_record(&self, command: &str) -> bool {
        if command.trim().is_empty() {
            return false;
        }
        !(self.hide_leading_space && command.starts_with(' '))
    }

    /// Whether a repeat of `previous` should be collapsed.
    #[must_use]
    pub fn collapses(&self, previous: Option<&str>, command: &str) -> bool {
        self.collapse_repeats && previous == Some(command)
    }

    /// The text that may be stored, with every configured secret replaced.
    ///
    /// The last capture group of a pattern is what gets replaced, so a pattern can match its
    /// context (`--password=`) while redacting only the value.
    #[must_use]
    pub fn redact(&self, command: &str) -> String {
        let mut text = command.to_owned();
        for pattern in &self.redactions {
            let mut result = String::with_capacity(text.len());
            let mut last_end = 0;
            for captures in pattern.captures_iter(&text) {
                let Some(whole) = captures.get(0) else {
                    continue;
                };
                let secret = (1..captures.len())
                    .rev()
                    .find_map(|index| captures.get(index))
                    .unwrap_or(whole);
                result.push_str(&text[last_end..secret.start()]);
                result.push_str(REDACTED);
                last_end = secret.end();
            }
            if last_end == 0 {
                continue;
            }
            result.push_str(&text[last_end..]);
            text = result;
        }
        text
    }
}

/// The secrets a shell redacts before it has been configured to redact anything.
///
/// Spec §17.5 requires that secrets not reach history, and an empty default list would mean the
/// mechanism worked and the product did not: nobody configures redaction before the first time it
/// would have mattered. These are the spellings a secret actually arrives in on a command line —
/// an option with a value, or an assignment — and each replaces only the value, so the command
/// stays readable and only the part that must not be kept is gone.
///
/// The list is deliberately short. A pattern that fires on ordinary text would teach people to
/// turn redaction off, which is worse than not having it.
const DEFAULT_REDACTIONS: &[&str] = &[
    // `--password=hunter2`, `--api-key hunter2`, `-p=hunter2`
    r"(?i)--?(password|passwd|pass|token|secret|api[-_]?key|access[-_]?key|auth|credential)[=\s]+(\S+)",
    // `PASSWORD=hunter2`, `AWS_SECRET_ACCESS_KEY=…` — an assignment anywhere in the line.
    r"(?i)\b([A-Z0-9_]*(PASSWORD|TOKEN|SECRET|API_?KEY|CREDENTIAL)[A-Z0-9_]*)=(\S+)",
];

impl Default for Policy {
    fn default() -> Self {
        Self {
            max_entries: DEFAULT_MAX_ENTRIES,
            collapse_repeats: false,
            hide_leading_space: true,
            redactions: DEFAULT_REDACTIONS
                .iter()
                .filter_map(|pattern| Regex::new(pattern).ok())
                .collect(),
        }
    }
}
