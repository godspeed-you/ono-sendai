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
    selectors: Vec<Selector>,
    options: Vec<(String, Value)>,
    limit: Option<usize>,
}

impl Query {
    /// A query for every object of `target`.
    #[must_use]
    pub fn target(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            selectors: Vec::new(),
            options: Vec::new(),
            limit: None,
        }
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
