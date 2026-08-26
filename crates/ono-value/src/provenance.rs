//! Record-level provenance, so `inspect` and `explain` can be trusted (spec §25.2).

use std::fmt::Write as _;
use std::sync::Arc;

use jiff::Timestamp;

use crate::SchemaId;

/// Where a record was observed from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Link {
    /// Observed on the machine the shell is running on.
    Local,
    /// Observed across a remote link, named as the user named it.
    Remote(Arc<str>),
}

impl std::fmt::Display for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Link::Local => f.write_str("local"),
            Link::Remote(host) => f.write_str(host),
        }
    }
}

/// Who produced a record, when, from what, and against which schema (spec §25.2).
///
/// Provenance is recorded per record rather than per field: spec §25.2 calls record-level
/// provenance a reasonable baseline, and per-field provenance would cost more than the first
/// phases can justify.
///
/// ```
/// use ono_value::{Provenance, SchemaId};
/// let provenance = Provenance::local("linux.procfs", SchemaId::new("ono.process", 1))
///     .from_source("/proc/4419/status");
/// assert!(provenance.render().contains("linux.procfs"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Provenance {
    provider: Arc<str>,
    observed: Option<Timestamp>,
    source: Option<Arc<str>>,
    link: Link,
    schema: SchemaId,
    confidence: Option<f64>,
}

impl Provenance {
    /// Provenance for a record observed on this machine.
    #[must_use]
    pub fn local(provider: &str, schema: SchemaId) -> Self {
        Self {
            provider: provider.into(),
            observed: None,
            source: None,
            link: Link::Local,
            schema,
            confidence: None,
        }
    }

    /// Provenance for a record observed across a remote link.
    #[must_use]
    pub fn remote(provider: &str, host: &str, schema: SchemaId) -> Self {
        Self {
            link: Link::Remote(host.into()),
            ..Self::local(provider, schema)
        }
    }

    /// Records when the observation was made.
    #[must_use]
    pub fn observed_at(mut self, observed: Timestamp) -> Self {
        self.observed = Some(observed);
        self
    }

    /// Records what the observation was read from, such as a list of procfs paths.
    #[must_use]
    pub fn from_source(mut self, source: &str) -> Self {
        self.source = Some(source.into());
        self
    }

    /// Records how confident the provider is in the record, on a scale from zero to one.
    ///
    /// Confidence stays unset unless a provider states it: spec §35.3 requires unknown data to be
    /// null rather than a fabricated default.
    #[must_use]
    pub fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = Some(confidence);
        self
    }

    /// The provider that produced the record, such as `linux.procfs`.
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// When the observation was made, if the provider recorded it.
    #[must_use]
    pub fn observed(&self) -> Option<Timestamp> {
        self.observed
    }

    /// What the observation was read from, if the provider recorded it.
    #[must_use]
    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    /// Whether the record was observed locally or across a link.
    #[must_use]
    pub const fn link(&self) -> &Link {
        &self.link
    }

    /// The schema the record claims to satisfy.
    #[must_use]
    pub const fn schema(&self) -> &SchemaId {
        &self.schema
    }

    /// How confident the provider is in the record, if it stated a confidence.
    #[must_use]
    pub const fn confidence(&self) -> Option<f64> {
        self.confidence
    }

    /// Renders the block spec §25.2 shows for `inspect`.
    ///
    /// Timestamps render in UTC. Turning them into the viewer's local time is a presentation
    /// decision and belongs to the renderer, not to the value (spec §13.1).
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        let mut line = |key: &str, value: &str| {
            // Writing into a String cannot fail; the result is discarded for that reason.
            let _ = writeln!(out, "{key:<13}{value}");
        };
        line("provider", &self.provider);
        line(
            "observed",
            &self
                .observed
                .map_or_else(|| "null".to_owned(), |observed| observed.to_string()),
        );
        line("source", self.source.as_deref().unwrap_or("null"));
        line("link", &self.link.to_string());
        line("schema", &self.schema.to_string());
        if let Some(confidence) = self.confidence {
            line("confidence", &confidence.to_string());
        }
        out
    }
}
