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
        format!("{} at Profile {}", self.benchmark, self.profile)
    }
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

    /// What `measured` says about this baseline, at `tolerance`.
    #[must_use]
    pub fn compare(&self, measured: &Measurement, tolerance: Tolerance) -> Comparison {
        if measured.environment != self.environment {
            return Comparison::ForeignEnvironment {
                baseline: self.environment.clone(),
                measured: measured.environment.clone(),
            };
        }
        let Some(baseline) = self.record(&measured.benchmark, &measured.profile) else {
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

    if problems.is_empty() {
        Ok(Measurement {
            benchmark,
            profile,
            commit,
            environment,
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
