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
    adapter: Option<Arc<AdapterTrace>>,
}

/// How an adapted record came to be: the external executable, the invocations, the adapter
/// and the decoder that produced it (spec v0.3 §1.8, ADR-0057).
///
/// Everything a person needs to answer "which program produced this, run how, read by what,
/// and how faithfully" — the ten questions of spec v0.3 §1.8. Exactness is recorded per field,
/// but only for fields that are not exact, so the common case costs nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdapterTrace {
    adapter: Arc<str>,
    adapter_version: Arc<str>,
    executable: Arc<std::path::Path>,
    executable_version: Option<Arc<str>>,
    user_invocation: Arc<str>,
    actual_invocation: Arc<str>,
    decoder: Arc<str>,
    stability: Arc<str>,
    exactness: std::collections::BTreeMap<String, String>,
    limits: Vec<String>,
}

impl AdapterTrace {
    /// Starts a trace for `adapter` at `adapter_version`, run as `executable`.
    #[must_use]
    pub fn new(adapter: &str, adapter_version: &str, executable: &std::path::Path) -> Self {
        Self {
            adapter: adapter.into(),
            adapter_version: adapter_version.into(),
            executable: Arc::from(executable),
            executable_version: None,
            user_invocation: "".into(),
            actual_invocation: "".into(),
            decoder: "".into(),
            stability: "stable".into(),
            exactness: std::collections::BTreeMap::new(),
            limits: Vec::new(),
        }
    }

    /// Records the executable's version, when it was detected.
    #[must_use]
    pub fn executable_version_of(mut self, version: Option<&str>) -> Self {
        self.executable_version = version.map(Into::into);
        self
    }

    /// Records the invocation as the user typed it and the one that actually ran.
    #[must_use]
    pub fn invocations(mut self, user: &str, actual: &str) -> Self {
        self.user_invocation = user.into();
        self.actual_invocation = actual.into();
        self
    }

    /// Records the decoder and whether its format is stable or version-constrained.
    #[must_use]
    pub fn decoded_by(mut self, decoder: &str, stability: &str) -> Self {
        self.decoder = decoder.into();
        self.stability = stability.into();
        self
    }

    /// Records that `field` is not exact — `normalized` or `inferred`.
    #[must_use]
    pub fn field_exactness(mut self, field: &str, exactness: &str) -> Self {
        self.exactness
            .insert(field.to_owned(), exactness.to_owned());
        self
    }

    /// Records what the adapter could not provide.
    #[must_use]
    pub fn with_limits(mut self, limits: Vec<String>) -> Self {
        self.limits = limits;
        self
    }

    /// The adapter's full id.
    #[must_use]
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// The adapter package's version.
    #[must_use]
    pub fn adapter_version(&self) -> &str {
        &self.adapter_version
    }

    /// The executable that ran.
    #[must_use]
    pub fn executable(&self) -> &std::path::Path {
        &self.executable
    }

    /// The executable's version, when it was detected.
    #[must_use]
    pub fn executable_version(&self) -> Option<&str> {
        self.executable_version.as_deref()
    }

    /// The invocation as the user typed it.
    #[must_use]
    pub fn user_invocation(&self) -> &str {
        &self.user_invocation
    }

    /// The invocation that actually ran.
    #[must_use]
    pub fn actual_invocation(&self) -> &str {
        &self.actual_invocation
    }

    /// The decoder that read the output.
    #[must_use]
    pub fn decoder(&self) -> &str {
        &self.decoder
    }

    /// `stable` for a documented machine format, `version-constrained` for a human-output parser.
    #[must_use]
    pub fn stability(&self) -> &str {
        &self.stability
    }

    /// The fields that are not exact, and how they are not.
    #[must_use]
    pub fn exactness(&self) -> &std::collections::BTreeMap<String, String> {
        &self.exactness
    }

    /// What the adapter could not provide.
    #[must_use]
    pub fn limits(&self) -> &[String] {
        &self.limits
    }
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
            adapter: None,
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

    /// Records that the value was adapted from an external command's output (spec v0.3 §1.8).
    #[must_use]
    pub fn adapted_by(mut self, trace: AdapterTrace) -> Self {
        self.adapter = Some(Arc::new(trace));
        self
    }

    /// The adapter trace, for a value that came from an external command.
    #[must_use]
    pub fn adapter(&self) -> Option<&AdapterTrace> {
        self.adapter.as_deref()
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
            let _ = writeln!(out, "{key:<19}{value}");
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
        if let Some(adapter) = &self.adapter {
            line("adapter", &adapter.adapter);
            line("adapter_version", &adapter.adapter_version);
            line("executable", &adapter.executable.display().to_string());
            line(
                "executable_version",
                adapter.executable_version.as_deref().unwrap_or("null"),
            );
            line("user_invocation", &adapter.user_invocation);
            line("actual_invocation", &adapter.actual_invocation);
            line("decoder", &adapter.decoder);
            line("stability", &adapter.stability);
            if !adapter.exactness.is_empty() {
                let listed: Vec<String> = adapter
                    .exactness
                    .iter()
                    .map(|(field, how)| format!("{field}={how}"))
                    .collect();
                line("exactness", &listed.join(", "));
            }
            for limit in &adapter.limits {
                line("limit", limit);
            }
        }
        out
    }
}
