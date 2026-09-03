//! Benchmark records and the regression baseline (v0.4.1 §32.3, §32.4, Appendix F.4).
//!
//! §32.3 lists six metrics and closes with the sentence this module exists for:
//!
//! > A single total runtime number is insufficient for streaming operations.
//!
//! A streaming operation that answers in 200 ms and finishes in 40 s is a different product from
//! one that is blank for 39 s and then dumps everything, and a total runtime cannot tell them
//! apart. That confusion is §0.5.7's, and issue #22 lived inside it for a release cycle: the
//! reported symptom was "30 s, zero bytes", which is a *time to first value* of never and a time
//! to completion nobody measured.
//!
//! §32.4 supplies the second half — a figure means nothing without the machine it was measured on:
//!
//! > Performance results MUST be stored in a machine-readable baseline file tied to the reference
//! > environment. CI MAY use percentage thresholds rather than exact wall-clock values on shared
//! > runners, but release qualification MUST run on a named reference environment with stable
//! > absolute targets.
//!
//! So a record names its environment, a comparison across two environments is *uncomparable*
//! rather than a pass, and the tolerance is a parameter rather than a constant: percentage for a
//! shared runner, absolute for release qualification.
//!
//! The baseline lives at `docs/spec/hardening/performance_baseline.json`; the profiles its records
//! name are `docs/spec/hardening/performance_profiles.yaml` (ADR-0488). Decisions: ADR-0489.

use serde_json::Value as Json;

use crate::scan::Problem;

/// Where the baseline lives, relative to the repository root.
pub const BASELINE: &str = "docs/spec/hardening/performance_baseline.json";

/// The six metrics of v0.4.1 §32.3, in the order the specification lists them.
///
/// Appendix F.4's example record spells four of them and leaves "values per second" and
/// "allocated/estimated bytes" implicit; F.4 permits different field names — *"Field names MAY
/// differ, but the information content is required"* — and the information content is what this
/// list is. Every one of them is required to be *present*; three of them may be `null`, because
/// §32.3 qualifies them with "where practical" and "where available" and v0.4.1 §2.6 keeps an
/// unknown unknown rather than turning it into a zero.
pub const REQUIRED_METRICS: [Metric; 6] = [
    Metric {
        field: "time_to_first_ms",
        spec: "time to first value",
        direction: Direction::Lower,
        may_be_unknown: false,
    },
    Metric {
        field: "time_to_complete_ms",
        spec: "time to completion",
        direction: Direction::Lower,
        may_be_unknown: false,
    },
    Metric {
        field: "peak_rss_bytes",
        spec: "peak or sampled RSS where practical",
        direction: Direction::Lower,
        may_be_unknown: true,
    },
    Metric {
        field: "values_per_second",
        spec: "values per second",
        direction: Direction::Higher,
        may_be_unknown: false,
    },
    Metric {
        field: "estimated_bytes",
        spec: "allocated/estimated bytes where available",
        direction: Direction::Lower,
        may_be_unknown: true,
    },
    Metric {
        field: "cancel_ms",
        spec: "cancellation latency",
        direction: Direction::Lower,
        may_be_unknown: true,
    },
];

/// Which way a metric gets worse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// A latency or a size: more is worse.
    Lower,
    /// A throughput: less is worse.
    Higher,
}

/// One of §32.3's six metrics, and what a change in it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metric {
    /// The field name in a record.
    pub field: &'static str,
    /// How §32.3 words it.
    pub spec: &'static str,
    /// Which way it gets worse.
    pub direction: Direction,
    /// Whether §32.3 permits it to be unknown on a host that cannot measure it.
    pub may_be_unknown: bool,
}

/// One benchmark result (Appendix F.4).
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    /// What was measured — `spatial.map_live`, `spatial.selector_miss`.
    pub benchmark: String,
    /// Which reference cardinality profile it ran at (§32.2): `S`, `M`, `L`.
    pub profile: String,
    /// The commit the figure belongs to.
    pub commit: String,
    /// The named reference environment it was measured on (§32.4, §37.2).
    pub environment: String,
    /// Whether the figure is cold, warm or a cache hit (§37.3).
    pub temperature: Temperature,
    /// How many iterations produced it (§37.4).
    pub iterations: u32,
    /// Which build profile produced the binary, so a debug figure cannot pass as a release one.
    pub build: String,
    /// How many values the run produced.
    pub values: f64,
    /// The p95 of the run's own distribution (§37.4).
    pub p95_ms: f64,
    /// §32.3's six, by field name, `None` where the host could not measure one.
    metrics: Vec<(&'static str, Option<f64>)>,
}

impl Measurement {
    /// §32.3's six metrics, in the order the specification lists them.
    #[must_use]
    pub fn metrics(&self) -> &[(&'static str, Option<f64>)] {
        &self.metrics
    }

    /// One metric by field name, `None` where the record left it unknown.
    #[must_use]
    pub fn metric(&self, field: &str) -> Option<f64> {
        self.metrics
            .iter()
            .find(|(name, _)| *name == field)
            .and_then(|(_, value)| *value)
    }

    /// How a record names itself in a message.
    fn label(&self) -> String {
        format!(
            "{} at Profile {} ({})",
            self.benchmark,
            self.profile,
            self.temperature.as_str()
        )
    }

    /// The record as Appendix F.4's document.
    #[must_use]
    pub fn to_json(&self) -> Json {
        let mut row = serde_json::Map::new();
        row.insert("benchmark".to_owned(), Json::from(self.benchmark.clone()));
        row.insert("profile".to_owned(), Json::from(self.profile.clone()));
        row.insert("commit".to_owned(), Json::from(self.commit.clone()));
        row.insert(
            "environment".to_owned(),
            Json::from(self.environment.clone()),
        );
        row.insert(
            "temperature".to_owned(),
            Json::from(self.temperature.as_str()),
        );
        row.insert("iterations".to_owned(), Json::from(self.iterations));
        row.insert("build".to_owned(), Json::from(self.build.clone()));
        row.insert("values".to_owned(), rounded(self.values));
        row.insert("p95_ms".to_owned(), rounded(self.p95_ms));
        for (field, value) in &self.metrics {
            row.insert((*field).to_owned(), value.map_or(Json::Null, rounded));
        }
        Json::Object(row)
    }
}

/// A figure at three decimal places, so a record does not carry noise it cannot justify.
fn rounded(value: f64) -> Json {
    serde_json::Number::from_f64(round3(value)).map_or(Json::Null, Json::Number)
}

/// A figure at three decimal places.
///
/// Applied where a measurement is *made*, not only where it is written, so the record a runner
/// hands back and the record the baseline holds are the same record. A microsecond of a
/// millisecond is below the noise floor of anything measured through a process boundary; keeping
/// it would make two equal runs compare unequal.
fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// The checked-in baseline of §32.4.
#[derive(Debug, Clone, PartialEq)]
pub struct Baseline {
    /// The reference environment every record in it was measured on (§32.4, §37.2).
    pub environment: String,
    /// The records, in file order.
    pub measurements: Vec<Measurement>,
}

/// How far a result may move from its baseline before it is a regression.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tolerance {
    /// A percentage of the baseline figure. §32.4 permits this on a shared runner.
    Percent(f64),
    /// The baseline figure itself. §32.4 requires this for release qualification.
    Absolute,
}

impl Tolerance {
    /// A percentage tolerance.
    #[must_use]
    pub fn percent(percent: f64) -> Self {
        Self::Percent(percent)
    }

    /// The worst figure `baseline` may become before it is a regression.
    fn allowed(self, baseline: f64, direction: Direction) -> f64 {
        match (self, direction) {
            (Self::Absolute, _) => baseline,
            (Self::Percent(percent), Direction::Lower) => baseline * (1.0 + percent / 100.0),
            (Self::Percent(percent), Direction::Higher) => baseline * (1.0 - percent / 100.0),
        }
    }
}

/// One metric that moved the wrong way.
#[derive(Debug, Clone, PartialEq)]
pub struct Regression {
    /// The benchmark and profile it belongs to.
    pub benchmark: String,
    /// The field that moved.
    pub metric: &'static str,
    /// What the baseline holds.
    pub baseline: f64,
    /// What was measured.
    pub measured: f64,
    /// The worst figure the tolerance permitted.
    pub allowed: f64,
}

/// What comparing a result against the baseline says.
///
/// The three answers that are not "held" are all distinct on purpose: §32.4's rule is about
/// *tying* a figure to an environment, and a comparison that cannot honour the tie has to say so
/// rather than fall through to a pass. §65.10's defect is a skip that reaches the summary as a
/// pass; this is the same defect wearing a benchmark's clothes.
#[derive(Debug, Clone, PartialEq)]
pub enum Comparison {
    /// Every metric is inside the tolerance.
    Held,
    /// At least one metric moved the wrong way past the tolerance.
    Regressed(Vec<Regression>),
    /// The baseline holds no record for this benchmark at this profile.
    Unmeasured,
    /// The result ran too few iterations to qualify a release (§37.4).
    Underpowered {
        /// How many iterations it ran.
        iterations: u32,
        /// How many §37.4 requires.
        required: u32,
    },
    /// The result was measured somewhere the baseline does not describe.
    ForeignEnvironment {
        /// The environment the baseline names.
        baseline: String,
        /// The environment the result names.
        measured: String,
    },
}

impl Baseline {
    /// Parses a baseline document, refusing any record that is not a §32.3 benchmark result.
    ///
    /// # Errors
    ///
    /// Returns every problem in the document rather than the first one, because a record missing
    /// three metrics should be fixed once.
    pub fn parse(text: &str) -> Result<Self, Vec<Problem>> {
        let document: Json = match serde_json::from_str(text) {
            Ok(document) => document,
            Err(error) => {
                return Err(vec![Problem::new(
                    BASELINE,
                    format!("the baseline is not valid JSON: {error}"),
                )]);
            }
        };

        let mut problems = Vec::new();
        let environment = document
            .get("environment")
            .and_then(Json::as_str)
            .unwrap_or_default()
            .to_owned();
        if environment.is_empty() {
            problems.push(Problem::new(
                BASELINE,
                "v0.4.1 §32.4 ties a baseline to a named reference environment, and this document \
                 names none. §37.2 says what naming one means: CPU model and core count, RAM, \
                 kernel version, image, toolchain and release build flags",
            ));
        }

        let rows = document
            .get("measurements")
            .and_then(Json::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let mut measurements = Vec::new();
        for row in rows {
            match measurement(row) {
                Ok(measurement) => measurements.push(measurement),
                Err(found) => problems.extend(found),
            }
        }

        if problems.is_empty() {
            Ok(Self {
                environment,
                measurements,
            })
        } else {
            Err(problems)
        }
    }

    /// The record for one benchmark at one profile, if the baseline holds it.
    #[must_use]
    pub fn record(&self, benchmark: &str, profile: &str) -> Option<&Measurement> {
        self.measurements
            .iter()
            .find(|record| record.benchmark == benchmark && record.profile == profile)
    }

    /// The record for one benchmark at one profile and one temperature.
    ///
    /// §37.3: *"A warm-cache number MUST not be advertised as cold performance."* Temperature is
    /// therefore part of the key rather than a field beside it, so a warm figure simply has no
    /// cold baseline to be held against.
    #[must_use]
    pub fn record_at(
        &self,
        benchmark: &str,
        profile: &str,
        temperature: Temperature,
    ) -> Option<&Measurement> {
        self.measurements.iter().find(|record| {
            record.benchmark == benchmark
                && record.profile == profile
                && record.temperature == temperature
        })
    }

    /// What `measured` says about this baseline, at `tolerance`.
    #[must_use]
    pub fn compare(&self, measured: &Measurement, tolerance: Tolerance) -> Comparison {
        if measured.environment != self.environment {
            return Comparison::ForeignEnvironment {
                baseline: self.environment.clone(),
                measured: measured.environment.clone(),
            };
        }
        if tolerance == Tolerance::Absolute && measured.iterations < MIN_ITERATIONS {
            // §37.4: "Single-run best-case timings MUST NOT define release success." Absolute
            // tolerance is release qualification, and a record below the floor cannot supply it.
            return Comparison::Underpowered {
                iterations: measured.iterations,
                required: MIN_ITERATIONS,
            };
        }
        let Some(baseline) =
            self.record_at(&measured.benchmark, &measured.profile, measured.temperature)
        else {
            return Comparison::Unmeasured;
        };

        let mut regressions = Vec::new();
        for metric in REQUIRED_METRICS {
            // A metric the baseline could not measure is not a metric a result can regress from.
            let (Some(before), Some(after)) =
                (baseline.metric(metric.field), measured.metric(metric.field))
            else {
                continue;
            };
            let allowed = tolerance.allowed(before, metric.direction);
            let worse = match metric.direction {
                Direction::Lower => after > allowed,
                Direction::Higher => after < allowed,
            };
            if worse {
                regressions.push(Regression {
                    benchmark: measured.label(),
                    metric: metric.field,
                    baseline: before,
                    measured: after,
                    allowed,
                });
            }
        }

        if regressions.is_empty() {
            Comparison::Held
        } else {
            Comparison::Regressed(regressions)
        }
    }
}

/// One record, or every reason it is not one.
fn measurement(row: &Json) -> Result<Measurement, Vec<Problem>> {
    let mut problems = Vec::new();
    let benchmark = required_string(row, "benchmark", &mut problems);
    let profile = required_string(row, "profile", &mut problems);
    let label = if benchmark.is_empty() {
        "a benchmark record".to_owned()
    } else {
        format!("`{benchmark}` at Profile {profile}")
    };

    let commit = required_string(row, "commit", &mut problems);
    let environment = required_string(row, "environment", &mut problems);
    if environment.is_empty() {
        problems.push(Problem::new(
            BASELINE,
            format!(
                "{label} names no reference environment; v0.4.1 §32.4 stores results tied to one, \
                 so a figure without it is a figure about an unknown machine"
            ),
        ));
    }

    let mut metrics = Vec::new();
    for metric in REQUIRED_METRICS {
        match row.get(metric.field) {
            None => problems.push(Problem::new(
                BASELINE,
                format!(
                    "{label} does not record `{}` — v0.4.1 §32.3's \"{}\". \"A single total \
                     runtime number is insufficient for streaming operations\", so every one of \
                     the six is required{}",
                    metric.field,
                    metric.spec,
                    if metric.may_be_unknown {
                        ", and this one may be `null` where the host cannot measure it"
                    } else {
                        ""
                    }
                ),
            )),
            Some(Json::Null) if metric.may_be_unknown => metrics.push((metric.field, None)),
            Some(Json::Null) => problems.push(Problem::new(
                BASELINE,
                format!(
                    "{label} records `{}` as unknown, and v0.4.1 §32.3 requires \"{}\" of every \
                     benchmark without qualification",
                    metric.field, metric.spec
                ),
            )),
            Some(value) => match value.as_f64() {
                Some(number) => metrics.push((metric.field, Some(number))),
                None => problems.push(Problem::new(
                    BASELINE,
                    format!(
                        "{label} records `{}` as {value}, not a number",
                        metric.field
                    ),
                )),
            },
        }
    }

    let values = required_number(row, "values", &label, &mut problems);
    let p95_ms = required_number(row, "p95_ms", &label, &mut problems);
    let build = required_string(row, "build", &mut problems);
    let iterations = u32::try_from(row.get("iterations").and_then(Json::as_u64).unwrap_or_else(
        || {
            problems.push(Problem::new(
                BASELINE,
                format!(
                    "{label} does not state how many iterations produced it; v0.4.1 §37.4 wants \
                     at least {MIN_ITERATIONS} for a short benchmark, and \"single-run best-case \
                     timings MUST NOT define release success\""
                ),
            ));
            0
        },
    ))
    .unwrap_or(0);
    let temperature = match row.get("temperature").and_then(Json::as_str) {
        Some(name) => Temperature::from_name(name).unwrap_or_else(|| {
            problems.push(Problem::new(
                BASELINE,
                format!(
                    "{label} declares the temperature `{name}`; v0.4.1 §37.3 distinguishes cold, \
                     warm and cache_hit"
                ),
            ));
            Temperature::Cold
        }),
        None => {
            problems.push(Problem::new(
                BASELINE,
                format!(
                    "{label} does not say whether it is a cold or a warm figure; v0.4.1 §37.3: \
                     \"A warm-cache number MUST not be advertised as cold performance\""
                ),
            ));
            Temperature::Cold
        }
    };

    if problems.is_empty() {
        Ok(Measurement {
            benchmark,
            profile,
            commit,
            environment,
            temperature,
            iterations,
            build,
            values,
            p95_ms,
            metrics,
        })
    } else {
        Err(problems)
    }
}

/// A string field, or a problem naming it.
fn required_string(row: &Json, field: &str, problems: &mut Vec<Problem>) -> String {
    match row.get(field).and_then(Json::as_str) {
        Some(text) => text.to_owned(),
        None => {
            problems.push(Problem::new(
                BASELINE,
                format!("a benchmark record does not name its `{field}` (Appendix F.4)"),
            ));
            String::new()
        }
    }
}

/// A numeric field, or a problem naming it.
fn required_number(row: &Json, field: &str, label: &str, problems: &mut Vec<Problem>) -> f64 {
    match row.get(field).and_then(Json::as_f64) {
        Some(number) => number,
        None => {
            problems.push(Problem::new(
                BASELINE,
                format!("{label} does not record `{field}` (Appendix F.4)"),
            ));
            0.0
        }
    }
}

// ------------------------------------------------------------------------------------------
// §37: the benchmark command, the reference environment, warm and cold.
// ------------------------------------------------------------------------------------------

/// Where the reference environment is declared, relative to the repository root.
pub const ENVIRONMENT: &str = "docs/spec/hardening/performance_environment.yaml";

/// The iteration floor of v0.4.1 §37.4.
///
/// > Performance acceptance SHOULD use at least 20 iterations for short benchmarks and report
/// > median plus p95. Single-run best-case timings MUST NOT define release success.
///
/// The second sentence is a MUST, so it is enforced rather than recommended: a record below this
/// floor cannot qualify a release, whatever its figures say.
pub const MIN_ITERATIONS: u32 = 20;

/// §37.2's six facts about the machine absolute performance gates are measured on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceEnvironment {
    /// How a record names it.
    pub id: String,
    /// Every declared field, so a missing one can be reported by name.
    fields: std::collections::BTreeMap<String, String>,
}

impl ReferenceEnvironment {
    /// Whether `field` is stated and non-empty.
    #[must_use]
    pub fn states(&self, field: &str) -> bool {
        self.fields
            .get(field)
            .is_some_and(|value| !value.trim().is_empty())
    }

    /// What `field` says, if it is stated.
    #[must_use]
    pub fn field(&self, field: &str) -> Option<&str> {
        self.fields.get(field).map(String::as_str)
    }
}

/// The reference environment of §37.2, as the registry declares it.
///
/// # Errors
///
/// Returns the reason the registry could not be read, so a caller can report it rather than
/// silently measuring against an unnamed machine.
pub fn reference_environment(root: &std::path::Path) -> Result<ReferenceEnvironment, String> {
    let path = root.join(ENVIRONMENT);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text)
        .map_err(|error| format!("{} is not valid YAML: {error}", path.display()))?;
    let row = document
        .get("environment")
        .ok_or_else(|| format!("{} declares no `environment`", path.display()))?;
    let mapping = row
        .as_mapping()
        .ok_or_else(|| format!("{}'s `environment` is not a mapping", path.display()))?;

    let mut fields = std::collections::BTreeMap::new();
    for (key, value) in mapping {
        let Some(name) = key.as_str() else { continue };
        let stated = match value {
            serde_yaml_ng::Value::String(text) => text.trim().to_owned(),
            serde_yaml_ng::Value::Number(number) => number.to_string(),
            _ => continue,
        };
        fields.insert(name.to_owned(), stated);
    }
    let id = fields
        .get("id")
        .cloned()
        .ok_or_else(|| format!("{} declares no environment `id`", path.display()))?;
    Ok(ReferenceEnvironment { id, fields })
}

/// Whether a run measured a cold process, a warm one, or a cache hit (v0.4.1 §37.3).
///
/// > Benchmarks MUST distinguish: cold startup / uncached query; warm process with provider
/// > initialized; cache-hit behavior where caches are part of product semantics. A warm-cache
/// > number MUST not be advertised as cold performance.
///
/// The last sentence is why this is part of a record's identity rather than a note on it: a warm
/// figure has no cold baseline to be compared against, and the comparison says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Temperature {
    /// A fresh process answering a query nothing has answered before.
    Cold,
    /// A process that is already running and whose providers are initialised, answering a query
    /// nothing has answered before.
    Warm,
    /// The same query answered again, from a cache that is part of the product's semantics.
    CacheHit,
}

impl Temperature {
    /// The word a record uses.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Warm => "warm",
            Self::CacheHit => "cache_hit",
        }
    }

    /// The temperature that word names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "cold" => Some(Self::Cold),
            "warm" => Some(Self::Warm),
            "cache_hit" => Some(Self::CacheHit),
            _ => None,
        }
    }
}

/// One declared benchmark: what to run, at what cardinality, at what temperature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Benchmark {
    /// What it measures — `spatial.map_first_frame`, `spatial.selector_miss`.
    pub id: &'static str,
    /// The reference cardinality profile the host is held at (§32.2, ADR-0488).
    pub profile: &'static str,
    /// Cold, warm or cache hit (§37.3).
    pub temperature: Temperature,
    /// What runs before the measured script, in the same process, for a warm measurement.
    pub warmup: Option<&'static str>,
    /// The script the figure is about.
    pub script: &'static str,
}

/// The marker a warm benchmark prints between its warm-up and the script being measured.
///
/// A warm figure has to be measured from the moment the process is warm, and the only place that
/// moment is observable from outside is the output stream. So the warm-up ends by printing this,
/// and the clock starts on the byte that carries it.
const MARK: &str = "ONO-PERF-MARK";

/// The benchmarks `cargo xtask perf` runs (§37.1).
///
/// Every one of them runs the real binary against a real host held at a declared profile: §32.2's
/// rule is that "provider/planner code exercised by the benchmark MUST match production logic",
/// and the shortest way to obey it is to measure the product rather than a harness around it.
pub const BENCHMARKS: &[Benchmark] = &[
    Benchmark {
        id: "shell.cold_start",
        profile: "S",
        temperature: Temperature::Cold,
        warmup: None,
        script: "echo ready",
    },
    Benchmark {
        id: "spatial.look",
        profile: "S",
        temperature: Temperature::Cold,
        warmup: None,
        script: "look --json",
    },
    Benchmark {
        id: "spatial.look",
        profile: "S",
        temperature: Temperature::CacheHit,
        warmup: Some("look --json | count"),
        script: "look --json",
    },
    Benchmark {
        id: "spatial.map_first_frame",
        profile: "S",
        temperature: Temperature::Cold,
        warmup: None,
        script: "map --live --json | take 1 | to json",
    },
    Benchmark {
        id: "spatial.selector_miss",
        profile: "S",
        temperature: Temperature::Cold,
        warmup: None,
        script: "enter no-such-place-1a2b3c",
    },
    Benchmark {
        id: "process.enumeration",
        profile: "S",
        temperature: Temperature::Cold,
        warmup: None,
        script: "get process | to json",
    },
    Benchmark {
        id: "service.enumeration",
        profile: "S",
        temperature: Temperature::Cold,
        warmup: None,
        script: "get service | count",
    },
    // Profile M, and at the place the cardinality actually lives: a thousand processes are in
    // COMPUTE, and a benchmark that measures the root measures the geography rather than the
    // system (§32.1).
    Benchmark {
        id: "spatial.query",
        profile: "M",
        temperature: Temperature::Cold,
        warmup: None,
        script: "enter compute; look --json",
    },
    Benchmark {
        id: "spatial.query",
        profile: "M",
        temperature: Temperature::CacheHit,
        warmup: Some("enter compute; look --json"),
        script: "look --json",
    },
    Benchmark {
        id: "spatial.query",
        profile: "M",
        temperature: Temperature::Warm,
        warmup: Some("get host"),
        script: "enter compute; look --json",
    },
    Benchmark {
        id: "spatial.map_first_frame",
        profile: "M",
        temperature: Temperature::Cold,
        warmup: None,
        script: "enter compute; map --live --json | take 1 | to json",
    },
    Benchmark {
        id: "spatial.selector_miss",
        profile: "M",
        temperature: Temperature::Cold,
        warmup: None,
        script: "enter no-such-place-1a2b3c",
    },
    // The other half of §36.1's sentence: a selector that resolves *through* the sweep. The
    // Profile M fixture places a thousand `sleep` children, so this one always has an answer, and
    // the difference between the two rows is how complete the sweep had to be.
    Benchmark {
        id: "spatial.selector_hit_by_sweep",
        profile: "M",
        temperature: Temperature::Cold,
        warmup: None,
        script: "enter sleep",
    },
    // Profile L on the axis this repository can build: a hundred thousand listening sockets, in
    // NETWORK where they are. The process axis at Profile L is the container's
    // (`docs/spec/hardening/performance_profiles.yaml`, ADR-0488).
    Benchmark {
        id: "spatial.map_first_frame",
        profile: "L",
        temperature: Temperature::Cold,
        warmup: None,
        script: "enter network; map --live --json | take 1 | to json",
    },
];

/// v0.4.1 §33.2's reference targets, as data.
///
/// > On the release reference environment:
/// >
/// > ```text
/// > basic cached look/near first result            < 50 ms p95
/// > spatial query Profile M first result           < 150 ms p95
/// > map live Profile M initial visible frame       < 500 ms p95
/// > map live Profile L initial progress/summary    < 1.5 s p95
/// > ```
///
/// Each row names the record it is a target for, so a target with no measurement behind it is a
/// missing measurement rather than a silent pass. "Basic cached" is the warm row of §37.3: a
/// cached `look` is one whose providers have already answered, which is exactly what a warm
/// measurement is.
pub const TARGETS: &[Target] = &[
    Target {
        spec: "basic cached look/near first result",
        benchmark: "spatial.look",
        profile: "S",
        temperature: Temperature::CacheHit,
        budget_ms: 50.0,
    },
    Target {
        spec: "spatial query Profile M first result",
        benchmark: "spatial.query",
        profile: "M",
        temperature: Temperature::Cold,
        budget_ms: 150.0,
    },
    Target {
        spec: "map live Profile M initial visible frame",
        benchmark: "spatial.map_first_frame",
        profile: "M",
        temperature: Temperature::Cold,
        budget_ms: 500.0,
    },
    Target {
        spec: "map live Profile L initial progress/summary",
        benchmark: "spatial.map_first_frame",
        profile: "L",
        temperature: Temperature::Cold,
        budget_ms: 1_500.0,
    },
];

/// v0.4.1 §33.3's hard interactive budget.
///
/// > A supported interactive operation MUST NOT spend 30 seconds producing neither output nor
/// > progress on the reference Profile M/L fixtures.
///
/// It is the floor underneath §33.2's four targets rather than one of them, and §61.3 makes it a
/// watchdog: *"A watchdog acceptance test MUST fail any interactive spatial command that produces
/// neither first result nor progress/refusal within the declared hard interactive budget."*
pub const HARD_INTERACTIVE_BUDGET_MS: f64 = 30_000.0;

/// A §33.2 target and what the baseline records against it.
#[derive(Debug, Clone, PartialEq)]
pub enum TargetVerdict {
    /// The recorded p95 is inside the budget.
    Held {
        /// What was measured.
        p95_ms: f64,
    },
    /// The recorded p95 is outside the budget.
    Missed {
        /// What was measured.
        p95_ms: f64,
        /// By what factor it exceeds the budget.
        factor: f64,
    },
    /// The baseline holds no record for the row, so nothing is known about it.
    Unmeasured,
}

/// What each §33.2 row says on the evidence of `baseline`.
///
/// Answering `Unmeasured` rather than passing is the same rule `Baseline::compare` follows: a
/// target nobody measured is not a target that holds (§65.10).
#[must_use]
pub fn verdicts(baseline: &Baseline) -> Vec<(&'static Target, TargetVerdict)> {
    TARGETS
        .iter()
        .map(|target| {
            let verdict = baseline
                .record_at(target.benchmark, target.profile, target.temperature)
                .map_or(TargetVerdict::Unmeasured, |record| {
                    if record.p95_ms <= target.budget_ms {
                        TargetVerdict::Held {
                            p95_ms: record.p95_ms,
                        }
                    } else {
                        TargetVerdict::Missed {
                            p95_ms: record.p95_ms,
                            factor: record.p95_ms / target.budget_ms,
                        }
                    }
                });
            (target, verdict)
        })
        .collect()
}

/// One row of §33.2's reference targets table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Target {
    /// How §33.2 words the row.
    pub spec: &'static str,
    /// The benchmark whose p95 answers for it.
    pub benchmark: &'static str,
    /// The profile it is stated at.
    pub profile: &'static str,
    /// The temperature it is stated at (§37.3).
    pub temperature: Temperature,
    /// The p95 the first result must stay inside.
    pub budget_ms: f64,
}

impl Benchmark {
    /// A benchmark that measures the harness rather than the product.
    ///
    /// The runner has to be exercised by a test, and the declared set builds populations and
    /// takes minutes. This one starts the binary, prints one value and exits, so what it proves
    /// is that a record comes out complete — which is the runner's contract.
    #[must_use]
    pub fn probe() -> Self {
        Self {
            id: "harness.probe",
            profile: "S",
            temperature: Temperature::Cold,
            warmup: None,
            script: "echo ready",
        }
    }

    /// The script the runner actually gives the shell.
    fn full_script(&self) -> String {
        match self.warmup {
            Some(warmup) => format!("{warmup} | count; echo {MARK}; {}", self.script),
            None => self.script.to_owned(),
        }
    }
}

/// One run of one benchmark, measured from outside the process.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Sample {
    /// Milliseconds from spawn (or from the warm-up marker) to the first byte of a value.
    to_first_ms: f64,
    /// Milliseconds from spawn to exit.
    to_complete_ms: f64,
    /// Peak resident set the kernel reported while it ran, when it could be sampled.
    peak_rss_bytes: Option<u64>,
    /// How many values it produced.
    values: f64,
    /// How many bytes of output it produced.
    bytes: u64,
}

/// Runs the declared benchmarks against a built binary (§37.1).
#[derive(Debug, Clone)]
pub struct Runner {
    binary: std::path::PathBuf,
    environment: String,
    commit: String,
    iterations: u32,
    build: String,
}

impl Runner {
    /// A runner that measures `binary` and files its records under `environment`.
    #[must_use]
    pub fn new(
        binary: impl Into<std::path::PathBuf>,
        environment: impl Into<String>,
        commit: impl Into<String>,
    ) -> Self {
        Self {
            binary: binary.into(),
            environment: environment.into(),
            commit: commit.into(),
            iterations: MIN_ITERATIONS,
            build: "release".to_owned(),
        }
    }

    /// How many iterations each benchmark runs (§37.4).
    #[must_use]
    pub fn iterations(mut self, iterations: u32) -> Self {
        self.iterations = iterations;
        self
    }

    /// Which build profile produced the binary, recorded so a debug figure cannot be read as a
    /// release one (§37.2's release build flags).
    #[must_use]
    pub fn build(mut self, build: impl Into<String>) -> Self {
        self.build = build.into();
        self
    }

    /// Measures one benchmark and returns its §32.3 record.
    ///
    /// # Panics
    ///
    /// Panics if the binary cannot be spawned at all, which means there is nothing to measure.
    #[must_use]
    pub fn run(&self, benchmark: &Benchmark) -> Measurement {
        let script = benchmark.full_script();
        let measured = benchmark.warmup.is_some();
        let samples: Vec<Sample> = (0..self.iterations)
            .map(|_| self.sample(&script, measured))
            .collect();

        let firsts: Vec<f64> = samples.iter().map(|sample| sample.to_first_ms).collect();
        let completes: Vec<f64> = samples.iter().map(|sample| sample.to_complete_ms).collect();
        let values = median(
            &samples
                .iter()
                .map(|sample| sample.values)
                .collect::<Vec<_>>(),
        );
        let complete_ms = median(&completes);
        let per_second = if complete_ms > 0.0 {
            values * 1000.0 / complete_ms
        } else {
            0.0
        };

        Measurement {
            benchmark: benchmark.id.to_owned(),
            profile: benchmark.profile.to_owned(),
            commit: self.commit.clone(),
            environment: self.environment.clone(),
            temperature: benchmark.temperature,
            iterations: self.iterations,
            build: self.build.clone(),
            values: round3(values),
            p95_ms: round3(percentile(&firsts, 95.0)),
            metrics: vec![
                ("time_to_first_ms", Some(round3(median(&firsts)))),
                ("time_to_complete_ms", Some(round3(complete_ms))),
                (
                    "peak_rss_bytes",
                    samples
                        .iter()
                        .filter_map(|sample| sample.peak_rss_bytes)
                        .max()
                        .map(|bytes| bytes as f64),
                ),
                ("values_per_second", Some(round3(per_second))),
                (
                    "estimated_bytes",
                    Some(round3(median(
                        &samples
                            .iter()
                            .map(|sample| sample.bytes as f64)
                            .collect::<Vec<_>>(),
                    ))),
                ),
                ("cancel_ms", self.cancellation(&script).map(round3)),
            ],
        }
    }

    /// One iteration.
    #[allow(
        clippy::expect_used,
        reason = "a benchmark whose binary will not start or whose pipe was not piped has \
                  nothing to measure, and reporting a zero for it would be worse (v0.4.1 §2.6)"
    )]
    fn sample(&self, script: &str, from_marker: bool) -> Sample {
        use std::io::Read;
        use std::time::Instant;

        let started = Instant::now();
        let mut child = std::process::Command::new(&self.binary)
            .args(["-c", script])
            .env("NO_COLOR", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the ono binary must be built before a benchmark runs it");
        let pid = child.id();

        // Read stdout on a worker that timestamps the first byte that is not the warm-up's, so
        // "time to first value" is what a user would have waited for rather than what the whole
        // pipeline took.
        let mut stdout = child.stdout.take().expect("stdout was piped");
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            let mut buffer = [0u8; 8192];
            let mut first: Option<Instant> = None;
            let mut clock: Option<Instant> = None;
            loop {
                match stdout.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        let chunk = String::from_utf8_lossy(&buffer[..count]);
                        let at = Instant::now();
                        if from_marker {
                            if clock.is_none() && (text.contains(MARK) || chunk.contains(MARK)) {
                                clock = Some(at);
                            } else if clock.is_some() && first.is_none() && !chunk.trim().is_empty()
                            {
                                first = Some(at);
                            }
                        } else if first.is_none() && !chunk.trim().is_empty() {
                            first = Some(at);
                        }
                        text.push_str(&chunk);
                    }
                }
            }
            (text, clock, first)
        });

        let mut peak = None;
        loop {
            if let Some(rss) = peak_rss(pid) {
                peak = Some(peak.map_or(rss, |seen: u64| seen.max(rss)));
            }
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(2)),
            }
        }
        let finished = Instant::now();
        let _ = child.wait();
        let (text, clock, first) = reader.join().unwrap_or_default();

        let zero = clock.unwrap_or(started);
        Sample {
            to_first_ms: first.map_or_else(
                || finished.saturating_duration_since(zero).as_secs_f64() * 1000.0,
                |at| at.saturating_duration_since(zero).as_secs_f64() * 1000.0,
            ),
            to_complete_ms: finished.saturating_duration_since(started).as_secs_f64() * 1000.0,
            peak_rss_bytes: peak,
            values: count_values(&text, from_marker),
            bytes: text.len() as u64,
        }
    }

    /// The p95 of the milliseconds from an interrupt to the process being gone (§32.3's
    /// cancellation latency), over as many samples as the benchmark's iterations.
    ///
    /// The p95 rather than the median, because §23.3 states its target as one:
    ///
    /// > p95 < 100 ms, p99 < 250 ms … measured from the cancellation signal to the cessation of
    /// > additional captured-value growth.
    ///
    /// ADR-0459 measured that behaviour deterministically and deliberately asserted no figure,
    /// naming issues #83 and #84 as what would make one meaningful. They exist, so the figure is
    /// taken here, under §37.4's rule and on §37.2's named environment. A p99 would want about a
    /// hundred samples rather than twenty; that is the sample count and nothing else.
    ///
    /// A benchmark that finishes before it can be interrupted has no cancellation latency to
    /// report, and `None` says so rather than reporting a zero nobody measured (§2.6).
    fn cancellation(&self, script: &str) -> Option<f64> {
        let samples: Vec<f64> = (0..self.iterations)
            .filter_map(|_| self.cancellation_sample(script))
            .collect();
        (!samples.is_empty()).then(|| percentile(&samples, 95.0))
    }

    /// One interrupt, and how long the process took to be gone.
    fn cancellation_sample(&self, script: &str) -> Option<f64> {
        use std::time::Instant;

        let mut child = std::process::Command::new(&self.binary)
            .args(["-c", script])
            .env("NO_COLOR", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        std::thread::sleep(std::time::Duration::from_millis(20));
        if matches!(child.try_wait(), Ok(Some(_))) {
            let _ = child.wait();
            return None;
        }

        let signalled = Instant::now();
        let _ = std::process::Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status();
        let deadline = signalled + std::time::Duration::from_secs(10);
        loop {
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => break,
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(1)),
            }
        }
        let stopped = Instant::now();
        let _ = child.wait();
        Some(stopped.saturating_duration_since(signalled).as_secs_f64() * 1000.0)
    }
}

/// The peak resident set of a live process, from `/proc`.
fn peak_rss(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .and_then(|value| value.split_whitespace().next()?.parse::<u64>().ok())
        .map(|kib| kib * 1024)
}

/// How many values a run produced.
///
/// A `to json` stage prints the stream as one array, so its length is the count; anything else
/// prints a line per value. Neither is a guess about the implementation — both are what the
/// output says.
fn count_values(text: &str, after_marker: bool) -> f64 {
    let body = if after_marker {
        text.split_once(MARK).map_or(text, |(_, rest)| rest)
    } else {
        text
    };
    let trimmed = body.trim();
    if let Ok(serde_json::Value::Array(values)) = serde_json::from_str::<Json>(trimmed) {
        return values.len() as f64;
    }
    trimmed
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count() as f64
}

/// The median of a sample, or zero for an empty one.
fn median(samples: &[f64]) -> f64 {
    percentile(samples, 50.0)
}

/// The `percent`th percentile of a sample, by nearest rank.
fn percentile(samples: &[f64], percent: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let rank = ((percent / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// Writes a baseline document (§32.4, Appendix F.4).
///
/// # Errors
///
/// Returns the reason the file could not be written.
pub fn write_baseline(
    path: &std::path::Path,
    environment: impl Into<String>,
    measurements: &[Measurement],
) -> Result<(), String> {
    let environment = environment.into();
    let document = serde_json::json!({
        "version": 1,
        "note": NOTE,
        "environment": environment,
        // The reference environment is a virtualised slice of a shared machine (§37.2's `notes`),
        // so what else it was doing is part of what a figure means. Recorded rather than assumed
        // away, because §32.4's absolute targets are read off this file by a later run.
        "load_average": load_average(),
        "measurements": measurements.iter().map(Measurement::to_json).collect::<Vec<_>>(),
    });
    let text = serde_json::to_string_pretty(&document)
        .map_err(|error| format!("cannot render the baseline: {error}"))?;
    std::fs::write(path, format!("{text}\n"))
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// The machine's one-minute load average, or `null` where it cannot be read.
fn load_average() -> Json {
    std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|text| text.split_whitespace().next()?.parse::<f64>().ok())
        .map_or(Json::Null, rounded)
}

/// What the baseline file says about itself.
const NOTE: &str = "The regression baseline of v0.4.1 §32.4. Every record carries the six \
                    metrics of §32.3 in the shape of Appendix F.4, names the reference \
                    environment of §37.2 it was measured on, and states whether it is a cold or \
                    a warm figure (§37.3). Written by `cargo xtask perf --write-baseline`; a \
                    figure edited by hand is a figure nobody measured. `cargo xtask spec-check` \
                    refuses a record that is not a complete §32.3 result.";

// ------------------------------------------------------------------------------------------
// §36.2's completion budget, measured directly (issue #21).
// ------------------------------------------------------------------------------------------

/// The benchmark id §36.2's first-completion budget is recorded under.
pub const COMPLETION_BENCHMARK: &str = "completion.first_candidate";

/// How long one cold provider-backed completion takes, in milliseconds, and how many candidates
/// it offered.
///
/// This is the *thing* v0.4.1 §36.2 budgets, called directly: `ono_cli::complete::ProviderValues`
/// in the seam the line editor installs it in, asked the question `get user <TAB>` asks. ADR-0252
/// accepted a thousand-iteration proxy over `ono_command::complete` with **no value completer at
/// all**, which measures registry lookups and touches no provider; issue #21 has been open on
/// that ever since.
///
/// It is deliberately one sample per process. The completer caches what a provider said for five
/// seconds, so the second call in a process is a cache hit and a different measurement (§37.3);
/// twenty *cold* samples are twenty processes, which is what the runner arranges.
#[must_use]
pub fn sample_completion() -> (f64, usize) {
    use ono_command::ValueCompleter as _;

    let Ok(registry) = ono_command::CommandRegistry::embedded() else {
        return (0.0, 0);
    };
    let Some(command) = registry
        .commands()
        .iter()
        .find(|command| command.id() == "ono.user.get")
    else {
        return (0.0, 0);
    };
    let Some(parameter) = command.selectors().first() else {
        return (0.0, 0);
    };
    let environment: Vec<(String, String)> = std::env::vars().collect();
    let completer = ono_cli::complete::ProviderValues::new(environment);

    let started = std::time::Instant::now();
    let offered = completer.complete(command, parameter, "");
    let elapsed = started.elapsed().as_secs_f64() * 1000.0;
    (elapsed, offered.len())
}

impl Runner {
    /// Measures §36.2's first completion, one cold sample per process.
    ///
    /// # Panics
    ///
    /// Panics if this executable cannot be located, which means nothing can be re-run.
    #[allow(
        clippy::expect_used,
        reason = "a benchmark that cannot find the executable it re-runs has nothing to measure"
    )]
    #[must_use]
    pub fn run_completion(&self) -> Measurement {
        let me = std::env::current_exe().expect("the running xtask must have a path");
        let mut latencies = Vec::new();
        let mut candidates = Vec::new();
        for _ in 0..self.iterations {
            let Ok(output) = std::process::Command::new(&me)
                .args(["perf", "--sample-completion"])
                .output()
            else {
                continue;
            };
            let text = String::from_utf8_lossy(&output.stdout);
            let mut parts = text.split_whitespace();
            let (Some(ms), Some(offered)) = (parts.next(), parts.next()) else {
                continue;
            };
            if let (Ok(ms), Ok(offered)) = (ms.parse::<f64>(), offered.parse::<f64>()) {
                latencies.push(ms);
                candidates.push(offered);
            }
        }

        let complete_ms = median(&latencies);
        let values = median(&candidates);
        Measurement {
            benchmark: COMPLETION_BENCHMARK.to_owned(),
            profile: "S".to_owned(),
            commit: self.commit.clone(),
            environment: self.environment.clone(),
            temperature: Temperature::Cold,
            iterations: u32::try_from(latencies.len()).unwrap_or(0),
            build: self.build.clone(),
            values: round3(values),
            p95_ms: round3(percentile(&latencies, 95.0)),
            metrics: vec![
                ("time_to_first_ms", Some(round3(complete_ms))),
                ("time_to_complete_ms", Some(round3(complete_ms))),
                ("peak_rss_bytes", None),
                (
                    "values_per_second",
                    Some(round3(if complete_ms > 0.0 {
                        values * 1000.0 / complete_ms
                    } else {
                        0.0
                    })),
                ),
                (
                    "estimated_bytes",
                    Some(round3(values * AVERAGE_CANDIDATE_BYTES)),
                ),
                // A completion is not cancellable: it answers at its own budget, which is what
                // §36.2 asks of it instead (§2.6 keeps the unknown unknown).
                ("cancel_ms", None),
            ],
        }
    }
}

/// What one candidate is estimated to weigh, for §32.3's byte metric.
///
/// An account name and the doc string a candidate carries. Approximate on purpose — §21.2 makes
/// value-size estimation deterministic and approximate, and a benchmark that serialized every
/// candidate to count its bytes would be measuring the counter.
const AVERAGE_CANDIDATE_BYTES: f64 = 64.0;

/// The three performance registries, validated where the gate can see them (v0.4.1 §52.3).
///
/// §52.3: *"`scripts/gate.sh` MUST validate every machine-readable contract for schema
/// correctness **and cross-reference integrity**."* The baseline was already parsed on every gate
/// run (ADR-0489); the environment it names and the profiles its records name were checked only
/// by the crates that consume them, which leaves the cross-references between the three
/// unverified by anything.
///
/// The three questions, in the order a reader asks them: does the baseline parse, does the
/// machine it names exist with §37.2's seven facts stated, and is every profile a record or a
/// declared benchmark names a profile Appendix F declares?
#[must_use]
pub fn check_registries(root: &std::path::Path) -> Vec<Problem> {
    let mut problems = Vec::new();

    let declared = match reference_environment(root) {
        Ok(environment) => Some(environment),
        Err(detail) => {
            // The registry arrives with the benchmark command; a missing one is not an error
            // (AGENTS.md §14), but an unreadable one is.
            if root.join(ENVIRONMENT).exists() {
                problems.push(Problem::new(ENVIRONMENT, detail));
            }
            None
        }
    };
    if let Some(environment) = &declared {
        for field in [
            "cpu_model",
            "cpu_cores",
            "ram_bytes",
            "kernel",
            "distribution",
            "rust_toolchain",
            "release_build_flags",
        ] {
            if !environment.states(field) {
                problems.push(Problem::new(
                    ENVIRONMENT,
                    format!(
                        "states no `{field}`; v0.4.1 §37.2 requires the reference environment to \
                         name it, because §32.4 puts release qualification on a named machine \
                         with absolute targets"
                    ),
                ));
            }
        }
    }

    let profiles = declared_profiles(root);
    let baseline = match std::fs::read_to_string(root.join(BASELINE)) {
        // The baseline arrives with the benchmark command (AGENTS.md §14).
        Err(_) => None,
        Ok(text) => match Baseline::parse(&text) {
            Ok(baseline) => Some(baseline),
            Err(found) => {
                problems.extend(found);
                None
            }
        },
    };

    if let Some(baseline) = &baseline {
        if let Some(environment) = &declared
            && baseline.environment != environment.id
        {
            problems.push(Problem::new(
                BASELINE,
                format!(
                    "is tied to the environment `{}`, and \
                     `docs/spec/hardening/performance_environment.yaml` names `{}` (§32.4)",
                    baseline.environment, environment.id
                ),
            ));
        }
        for record in &baseline.measurements {
            if !profiles.is_empty() && !profiles.contains(&record.profile) {
                problems.push(Problem::new(
                    BASELINE,
                    format!(
                        "records `{}` at profile `{}`, which \
                         `docs/spec/hardening/performance_profiles.yaml` does not declare \
                         (Appendix F)",
                        record.benchmark, record.profile
                    ),
                ));
            }
        }
    }

    if !profiles.is_empty() {
        for benchmark in BENCHMARKS {
            if !profiles.contains(benchmark.profile) {
                problems.push(Problem::new(
                    "docs/spec/hardening/performance_profiles.yaml",
                    format!(
                        "declares no profile `{}`, which the benchmark `{}` is measured at \
                         (v0.4.1 §32.2, Appendix F)",
                        benchmark.profile, benchmark.id
                    ),
                ));
            }
        }
    }
    problems
}

/// Every topology profile `docs/spec/hardening/performance_profiles.yaml` declares.
fn declared_profiles(root: &std::path::Path) -> std::collections::BTreeSet<String> {
    let Ok(text) =
        std::fs::read_to_string(root.join("docs/spec/hardening/performance_profiles.yaml"))
    else {
        return std::collections::BTreeSet::new();
    };
    let Ok(document) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text) else {
        return std::collections::BTreeSet::new();
    };
    document
        .get("profiles")
        .and_then(serde_yaml_ng::Value::as_sequence)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.get("id").and_then(serde_yaml_ng::Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
