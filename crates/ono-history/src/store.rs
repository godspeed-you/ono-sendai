//! Reading, recording and persisting history.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::cursor::Cursor;
use crate::entry::{Entry, Outcome};
use crate::id::IdSource;
use crate::policy::Policy;

/// Why history could not be read or written.
///
/// History failing must never take the shell down with it — the caller reports the problem and
/// carries on with an in-memory history — but it must never fail silently either, because a user
/// who thinks their history is being kept and finds it empty next week has lost real work.
#[derive(Debug)]
pub struct HistoryError {
    path: PathBuf,
    action: &'static str,
    cause: std::io::Error,
}

impl HistoryError {
    /// The file that could not be used.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl fmt::Display for HistoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot {} history at {}: {}",
            self.action,
            self.path.display(),
            self.cause
        )
    }
}

impl std::error::Error for HistoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}

/// What the shell remembers, and the file it remembers it in.
#[derive(Debug)]
pub struct History {
    path: PathBuf,
    policy: Policy,
    entries: Vec<Entry>,
    ids: IdSource,
    /// How many entries the file already holds, so an ordinary flush appends one line.
    persisted: usize,
    /// Set when trimming dropped an entry the file still holds, forcing a rewrite.
    stale: bool,
}

impl History {
    /// Opens the history at `path`, creating its directory if the shell has never run before.
    ///
    /// Lines that cannot be parsed are skipped: a torn write costs one entry, not the file.
    ///
    /// # Errors
    ///
    /// Returns a [`HistoryError`] when the path exists but cannot be used as a history file —
    /// it is a directory, or its directory cannot be created or read.
    pub fn open(path: &Path, policy: Policy) -> Result<Self, HistoryError> {
        let fail = |action: &'static str, cause: std::io::Error| HistoryError {
            path: path.to_path_buf(),
            action,
            cause,
        };

        if path.is_dir() {
            return Err(fail(
                "use",
                std::io::Error::new(
                    std::io::ErrorKind::IsADirectory,
                    "the history path is a directory",
                ),
            ));
        }
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|cause| fail("create the directory for", cause))?;
        }

        let mut entries = Vec::new();
        match File::open(path) {
            Ok(file) => {
                for line in BufReader::new(file).lines().map_while(Result::ok) {
                    if line.trim().is_empty() {
                        continue;
                    }
                    if let Ok(entry) = serde_json::from_str::<Entry>(&line) {
                        entries.push(entry);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(fail("read", error)),
        }

        let limit = policy.entry_limit();
        let stale = entries.len() > limit;
        if stale {
            entries.drain(..entries.len() - limit);
        }

        Ok(Self {
            path: path.to_path_buf(),
            policy,
            persisted: entries.len(),
            entries,
            ids: IdSource::new(),
            stale,
        })
    }

    /// Records `command`, applying the policy: hidden commands are not stored at all, secrets are
    /// replaced before the text is kept, and a collapsed repeat is not stored twice.
    ///
    /// Returns the entry's identity, or `None` when the policy declined to record it.
    pub fn record(&mut self, command: &str, cwd: &Path, outcome: Outcome) -> Option<String> {
        if !self.policy.should_record(command) {
            return None;
        }
        let text = self.policy.redact(command);
        let previous = self.entries.last().map(Entry::command_text);
        if self.policy.collapses(previous, &text) {
            return None;
        }

        let id = self.ids.next_id();
        self.entries.push(Entry::new(
            id.clone(),
            self.ids.session().to_owned(),
            text,
            cwd.to_path_buf(),
            Some(outcome),
        ));

        let limit = self.policy.entry_limit();
        if self.entries.len() > limit {
            let excess = self.entries.len() - limit;
            self.entries.drain(..excess);
            self.persisted = self.persisted.saturating_sub(excess);
            self.stale = true;
        }
        Some(id)
    }

    /// Writes everything not yet on disk.
    ///
    /// An ordinary flush appends the new lines. A flush after trimming rewrites the file through
    /// a temporary and a rename, so a reader never sees a half-written history.
    ///
    /// # Errors
    ///
    /// Returns a [`HistoryError`] if the file cannot be written.
    pub fn flush(&mut self) -> Result<(), HistoryError> {
        let fail = |action: &'static str, cause: std::io::Error| HistoryError {
            path: self.path.clone(),
            action,
            cause,
        };

        if self.stale {
            let temporary = self.path.with_extension("jsonl.new");
            let mut file = File::create(&temporary).map_err(|cause| fail("write", cause))?;
            for entry in &self.entries {
                write_entry(&mut file, entry).map_err(|cause| fail("write", cause))?;
            }
            file.sync_all().map_err(|cause| fail("write", cause))?;
            drop(file);
            std::fs::rename(&temporary, &self.path).map_err(|cause| fail("replace", cause))?;
            self.persisted = self.entries.len();
            self.stale = false;
            return Ok(());
        }

        if self.persisted >= self.entries.len() {
            return Ok(());
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|cause| fail("append to", cause))?;
        for entry in &self.entries[self.persisted..] {
            write_entry(&mut file, entry).map_err(|cause| fail("append to", cause))?;
        }
        file.flush().map_err(|cause| fail("append to", cause))?;
        self.persisted = self.entries.len();
        Ok(())
    }

    /// Everything remembered, oldest first.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// A cursor positioned on the line being typed.
    #[must_use]
    pub fn cursor(&self) -> Cursor<'_> {
        Cursor::new(&self.entries)
    }

    /// The index of the newest entry containing `needle` strictly before `before`.
    ///
    /// `None` for `before` searches from the newest entry. This is what Ctrl-R repeats against.
    #[must_use]
    pub fn search_before(&self, needle: &str, before: Option<usize>) -> Option<usize> {
        let upper = before.unwrap_or(self.entries.len()).min(self.entries.len());
        self.entries[..upper]
            .iter()
            .rposition(|entry| entry.command_text().contains(needle))
    }

    /// The policy in force.
    #[must_use]
    pub fn policy(&self) -> &Policy {
        &self.policy
    }
}

fn write_entry(file: &mut File, entry: &Entry) -> std::io::Result<()> {
    let line = serde_json::to_string(entry)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")
}
