//! What a provider is asked for.
//!
//! A query is already resolved: it carries values, not syntax. Turning `get process | where cpu >
//! 20` into one is the evaluator's job, and a plugin building one through the host API
//! (spec §31.13) does not need a parser to do it.

use ono_value::{RecordValue, Value};

/// A request for objects of one target.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    target: String,
    verb: String,
    selectors: Vec<Selector>,
    options: Vec<(String, Value)>,
    limit: Option<usize>,
    directory: Option<std::sync::Arc<std::path::Path>>,
}

impl Query {
    /// A query for every object of `target`.
    #[must_use]
    pub fn target(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            verb: "get".to_owned(),
            selectors: Vec::new(),
            options: Vec::new(),
            limit: None,
            directory: None,
        }
    }

    /// Says what is asked of the objects, in the verb the user typed.
    ///
    /// `get` — the default — and `find` ask for the objects themselves. A provider that can
    /// answer more than that (`read file` asks for a file's *content*, `tail file` for the
    /// lines appended to it) tells the two apart by this verb; one that cannot ignores it and
    /// the command's contract, which named a capability the provider does not advertise, keeps
    /// the command from ever reaching it.
    #[must_use]
    pub fn for_verb(mut self, verb: impl Into<String>) -> Self {
        self.verb = verb.into();
        self
    }

    /// The verb the query is asked in; `get` unless [`for_verb`](Self::for_verb) said otherwise.
    #[must_use]
    pub fn verb(&self) -> &str {
        &self.verb
    }

    /// Narrows the query.
    ///
    /// A provider may honour a selector by asking the system for less — which is the whole point,
    /// since `get process 4419` should read one directory rather than all of them — or ignore it
    /// and let the pipeline filter. Correctness never depends on which it chose.
    #[must_use]
    pub fn with(mut self, selector: Selector) -> Self {
        self.selectors.push(selector);
        self
    }

    /// Sets a provider option, such as `--recursive`.
    #[must_use]
    pub fn option(mut self, name: impl Into<String>, value: Value) -> Self {
        self.options.push((name.into(), value));
        self
    }

    /// Asks for at most `limit` objects.
    #[must_use]
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Says which directory a relative or omitted path in this query means.
    ///
    /// Without one it is the process's working directory, which is the foreground session's. A
    /// background job asks in the directory it was started in, which the foreground may since
    /// have left (issue #302, ADR-0957): a provider that defaults a path — a listing of "here" —
    /// defaults it to this directory.
    #[must_use]
    pub fn within(mut self, directory: std::sync::Arc<std::path::Path>) -> Self {
        self.directory = Some(directory);
        self
    }

    /// The directory a relative or omitted path means, when the query names one.
    #[must_use]
    pub fn working_directory(&self) -> Option<&std::path::Path> {
        self.directory.as_deref()
    }

    /// `path` as this query means it: joined onto its directory when it is relative and the
    /// query names one, as it stands otherwise.
    ///
    /// The join keeps the text it joins — `./x` stays `<dir>/./x` and `link/` keeps its slash —
    /// so the anchored path names the object the same words name in the foreground (ADR-0957).
    #[must_use]
    pub fn resolve_path(&self, path: &std::path::Path) -> std::path::PathBuf {
        anchor_path(self.directory.as_deref(), path)
    }

    /// The target being asked for.
    #[must_use]
    pub fn target_name(&self) -> &str {
        &self.target
    }

    /// The selectors narrowing the query.
    #[must_use]
    pub fn selectors(&self) -> &[Selector] {
        &self.selectors
    }

    /// Every option, in the order it was written.
    ///
    /// A forwarding provider — a remote link, a KUANG/11 bridge — has to carry the options it
    /// did not itself declare, and enumerating them is the only way to carry them all.
    #[must_use]
    pub fn options(&self) -> &[(String, Value)] {
        &self.options
    }

    /// An option's value, if it was given.
    #[must_use]
    pub fn option_value(&self, name: &str) -> Option<&Value> {
        self.options
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value)
    }

    /// Whether a boolean option was given and is true.
    #[must_use]
    pub fn flag(&self, name: &str) -> bool {
        matches!(self.option_value(name), Some(Value::Bool(true)))
    }

    /// The maximum number of objects wanted, if one was given.
    #[must_use]
    pub fn max(&self) -> Option<usize> {
        self.limit
    }

    /// Whether `record` satisfies every selector.
    #[must_use]
    pub fn matches(&self, record: &RecordValue) -> bool {
        self.selectors
            .iter()
            .all(|selector| selector.matches(record))
    }
}

/// A narrowing condition a provider may push down into the system it is asking.
#[derive(Debug, Clone, PartialEq)]
pub enum Selector {
    /// A field equal to a value.
    Field {
        /// The field's name.
        name: String,
        /// The value it must equal.
        value: Value,
    },
    /// A field whose text contains a substring, for name-like searches.
    Contains {
        /// The field's name.
        name: String,
        /// The text it must contain.
        text: String,
    },
    /// One specific object.
    Identity(crate::ObjectId),
}

impl Selector {
    /// A field equal to a value.
    #[must_use]
    pub fn field(name: impl Into<String>, value: Value) -> Self {
        Selector::Field {
            name: name.into(),
            value,
        }
    }

    /// A field containing text.
    #[must_use]
    pub fn contains(name: impl Into<String>, text: impl Into<String>) -> Self {
        Selector::Contains {
            name: name.into(),
            text: text.into(),
        }
    }

    /// One specific object.
    #[must_use]
    pub fn identity(id: crate::ObjectId) -> Self {
        Selector::Identity(id)
    }

    /// Whether `record` satisfies the selector.
    ///
    /// A field the record does not have never matches, and a field whose value is unknown never
    /// matches either — an unknown value is not equal to anything, which is ADR-0014's rule
    /// applied where a provider filters rather than where a pipeline does.
    #[must_use]
    pub fn matches(&self, record: &RecordValue) -> bool {
        match self {
            Selector::Field { name, value } => record
                .get(name)
                .is_some_and(|found| !matches!(found, Value::Null) && found == value),
            Selector::Contains { name, text } => record
                .get(name)
                .and_then(|found| ono_value::canonical_text(found).ok())
                .is_some_and(|found| found.contains(text.as_str())),
            Selector::Identity(id) => crate::ObjectId::of(record).as_ref() == Some(id),
        }
    }

    /// The field the selector narrows on, where it narrows on one.
    #[must_use]
    pub fn field_name(&self) -> Option<&str> {
        match self {
            Selector::Field { name, .. } | Selector::Contains { name, .. } => Some(name),
            Selector::Identity(_) => None,
        }
    }
}

/// `path` joined onto `directory` when it is relative and there is a directory, as it stands
/// otherwise. Lexical, and keeping the text it joins: nothing is canonicalized, `.` and `..` stay
/// and a trailing slash survives (ADR-0957).
#[must_use]
pub fn anchor_path(
    directory: Option<&std::path::Path>,
    path: &std::path::Path,
) -> std::path::PathBuf {
    match directory {
        Some(directory) if path.is_relative() => {
            let mut joined = directory.as_os_str().to_owned();
            if !directory.as_os_str().as_encoded_bytes().ends_with(b"/") {
                joined.push("/");
            }
            joined.push(path.as_os_str());
            std::path::PathBuf::from(joined)
        }
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
mod anchoring {
    use super::anchor_path as anchor;
    use std::path::{Path, PathBuf};

    #[test]
    fn should_keep_the_text_it_joins_when_a_relative_path_is_anchored() {
        let dir = Path::new("/job");
        assert_eq!(
            anchor(Some(dir), Path::new("link/")),
            PathBuf::from("/job/link/")
        );
        assert_eq!(
            anchor(Some(dir), Path::new("./x")),
            PathBuf::from("/job/./x")
        );
        assert_eq!(
            anchor(Some(dir), Path::new("../y")),
            PathBuf::from("/job/../y")
        );
        assert_eq!(anchor(Some(dir), Path::new("/abs")), PathBuf::from("/abs"));
        assert_eq!(anchor(None, Path::new("rel")), PathBuf::from("rel"));
        assert_eq!(
            anchor(Some(Path::new("/")), Path::new("x")),
            PathBuf::from("/x")
        );
    }
}
