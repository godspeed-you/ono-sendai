//! The deterministic temporal harness: one fixed origin, one way to advance it, and the
//! declaration of §49's fixture ledger (spec v0.5 §39.2, §49, work packages TEST-002 and TEST-003).
//!
//! v0.5 §39.2 makes time a parameter rather than an ambient fact: *"pure functions over supplied
//! time"*, so no temporal crate reads a clock and every one of them takes the instant as an
//! argument. A test therefore needs an instant to hand in, and the moment two suites choose two
//! different origins their fixtures stop being comparable — a timeline written against
//! `2026-08-31` cannot be replayed against a checkpoint written against `2026-01-01`.
//!
//! So there is one origin for the whole workspace and it is the one the KUANG/11 test host
//! already pins: [`VIRTUAL_NOW`], `2026-08-26T12:00:00Z`, spec §31.73's virtual time. Agreeing
//! with the shipped constant costs nothing and choosing a second one would cost every future
//! cross-crate fixture.
//!
//! ```
//! use ono_testkit::temporal::Clock;
//!
//! let mut clock = Clock::fixed();
//! let started = clock.now();
//! let later = clock.advance_minutes(15);
//! assert_eq!(later.as_second() - started.as_second(), 15 * 60);
//! assert_eq!(Clock::fixed().now(), started, "the origin is fixed, so two runs agree");
//! ```
//!
//! The second half of the module is the declaration of the fixture ledger v0.5 §49 requires —
//! a million events, a hundred thousand lifetimes, half a million relation changes and ten
//! thousand action records. The numbers live in
//! `docs/contracts/hardening/performance_profiles.yaml` beside Appendix F's topology profiles,
//! and [`declared_temporal_profiles`] reads them back, so the registry and the fixture cannot
//! drift apart in either direction (ADR-0746).

use jiff::{Span, Timestamp};

/// The fixed instant every deterministic temporal fixture is anchored to (spec §31.73).
///
/// The same string `ono_kuang_testhost::VIRTUAL_NOW` carries. A second origin beside it would
/// make two suites' fixtures incomparable for no gain.
pub const VIRTUAL_NOW: &str = "2026-08-26T12:00:00Z";

/// A clock that is a value rather than a reading (spec v0.5 §39.2).
///
/// Every temporal crate takes the instant as a parameter, so a harness is a small thing done
/// once: an origin, a cursor, and a way to move the cursor. Nothing here reads the system clock,
/// which is what makes a suite built on it reproducible on a machine in another year.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    origin: Timestamp,
    now: Timestamp,
}

impl Default for Clock {
    fn default() -> Self {
        Self::fixed()
    }
}

impl Clock {
    /// A clock at [`VIRTUAL_NOW`].
    ///
    /// # Panics
    ///
    /// Panics if the constant stops being an RFC 3339 instant, which is a broken build rather
    /// than a failing test.
    #[must_use]
    pub fn fixed() -> Self {
        Self::starting_at(VIRTUAL_NOW)
    }

    /// A clock at `text`, for a fixture that needs a second, explicitly stated origin.
    ///
    /// # Panics
    ///
    /// Panics if `text` is not an RFC 3339 instant, which is a fixture that cannot be built.
    #[must_use]
    pub fn starting_at(text: &str) -> Self {
        let origin: Timestamp = text.parse().unwrap_or_else(|error| {
            panic!("`{text}` is not an instant a fixture can start at: {error}")
        });
        Self {
            origin,
            now: origin,
        }
    }

    /// The instant the clock started at, whatever it has since been advanced to.
    #[must_use]
    pub const fn origin(&self) -> Timestamp {
        self.origin
    }

    /// The instant the clock reads now.
    #[must_use]
    pub const fn now(&self) -> Timestamp {
        self.now
    }

    /// Moves the clock forward by `span` and answers with the new instant.
    ///
    /// # Panics
    ///
    /// Panics if the span moves the clock outside the representable range, which means the
    /// fixture asked for an instant no store can hold.
    pub fn advance(&mut self, span: Span) -> Timestamp {
        self.now = self
            .now
            .checked_add(span)
            .unwrap_or_else(|error| panic!("a fixture clock cannot advance by {span}: {error}"));
        self.now
    }

    /// Moves the clock forward by `seconds`.
    pub fn advance_seconds(&mut self, seconds: i64) -> Timestamp {
        self.advance(Span::new().seconds(seconds))
    }

    /// Moves the clock forward by `minutes`.
    pub fn advance_minutes(&mut self, minutes: i64) -> Timestamp {
        self.advance(Span::new().minutes(minutes))
    }

    /// Moves the clock forward by `hours`.
    pub fn advance_hours(&mut self, hours: i64) -> Timestamp {
        self.advance(Span::new().hours(hours))
    }

    /// The instant `seconds` after the origin, without moving the cursor.
    ///
    /// # Panics
    ///
    /// Panics if the offset leaves the representable range.
    #[must_use]
    pub fn at_second(&self, seconds: i64) -> Timestamp {
        self.origin
            .checked_add(Span::new().seconds(seconds))
            .unwrap_or_else(|error| panic!("second {seconds} is not an instant: {error}"))
    }

    /// `count` instants `every` apart, the first one at the cursor, without moving the cursor.
    ///
    /// This is the shape a plausible event sequence has: a source that reports on a cadence, so
    /// a window of a stated width holds a known number of events and a test can assert the count
    /// rather than discover it.
    ///
    /// # Panics
    ///
    /// Panics if the cadence leaves the representable range.
    #[must_use]
    pub fn cadence(&self, count: usize, every: Span) -> Vec<Timestamp> {
        let mut instants = Vec::with_capacity(count);
        let mut at = self.now;
        for _ in 0..count {
            instants.push(at);
            at = at
                .checked_add(every)
                .unwrap_or_else(|error| panic!("a cadence of {every} left the range: {error}"));
        }
        instants
    }
}

// ------------------------------------------------------------------------------------------
// The fixture ledger of v0.5 §49 (work package TEST-003).
// ------------------------------------------------------------------------------------------

/// The registry of Appendix F and of v0.5 §49, embedded so a fixture cannot drift from it.
const DECLARATIONS: &str =
    include_str!("../../../docs/contracts/hardening/performance_profiles.yaml");

/// The cardinality of a fixture ledger (spec v0.5 §49).
///
/// Four numbers of one ledger rather than four knobs: §49 describes *a* deterministic fixture
/// whose relation changes and action records are part of its million events and whose subjects
/// are the hundred thousand lifetimes those events are about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TemporalProfile {
    /// How a measurement record names it — `T`.
    pub name: &'static str,
    /// Events the ledger holds (§49).
    pub events: usize,
    /// Distinct object identities those events are about (§49).
    pub objects: usize,
    /// How many of the events are relation changes (§49).
    pub relation_changes: usize,
    /// Action records the ledger holds (§49).
    pub actions: usize,
    /// The seed the generator is run from, so the same fixture is the same fixture.
    pub seed: u64,
}

/// v0.5 §49's fixture ledger: the cardinality release evidence is measured against.
pub const TEMPORAL_PROFILE_T: TemporalProfile = TemporalProfile {
    name: "T",
    events: 1_000_000,
    objects: 100_000,
    relation_changes: 500_000,
    actions: 10_000,
    seed: 0x0005_0049,
};

impl TemporalProfile {
    /// The profile constant carrying `id`, when there is one.
    #[must_use]
    pub fn named(id: &str) -> Option<Self> {
        [TEMPORAL_PROFILE_T]
            .into_iter()
            .find(|profile| profile.name == id)
    }
}

/// One row of the registry's `temporal_profiles`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemporalProfileDeclaration {
    /// `T`.
    pub id: String,
    /// §49's event count.
    pub events: usize,
    /// §49's object/lifetime count.
    pub objects: usize,
    /// §49's relation-change count.
    pub relation_changes: usize,
    /// §49's action-record count.
    pub actions: usize,
    /// The generator seed, so the fixture is reproducible byte for byte.
    pub seed: u64,
    /// Where a fixture of this size can honestly be created.
    pub built_by: crate::BuiltBy,
}

impl TemporalProfileDeclaration {
    /// The constant a fixture is built from.
    ///
    /// # Panics
    ///
    /// Panics when no constant carries the declared id, which means the registry declares a
    /// fixture nothing can build.
    #[must_use]
    pub fn profile(&self) -> TemporalProfile {
        TemporalProfile::named(&self.id)
            .unwrap_or_else(|| panic!("no temporal profile constant is named `{}`", self.id))
    }
}

/// v0.5 §49's fixture ledgers as the registry declares them, in registry order.
///
/// # Panics
///
/// Panics if the registry cannot be parsed, which means no benchmark can state its cardinality.
#[must_use]
pub fn declared_temporal_profiles() -> Vec<TemporalProfileDeclaration> {
    let document: serde_yaml_ng::Value = serde_yaml_ng::from_str(DECLARATIONS)
        .expect("docs/contracts/hardening/performance_profiles.yaml must be valid YAML");
    document
        .get("temporal_profiles")
        .and_then(serde_yaml_ng::Value::as_sequence)
        .map(|rows| rows.iter().map(temporal_declaration).collect())
        .expect("performance_profiles.yaml declares `temporal_profiles` (v0.5 section 49)")
}

/// One temporal profile row.
fn temporal_declaration(row: &serde_yaml_ng::Value) -> TemporalProfileDeclaration {
    let id = field_str(row, "id");
    let built = field_str(row, "built_by");
    TemporalProfileDeclaration {
        events: field_usize(row, "events"),
        objects: field_usize(row, "objects"),
        relation_changes: field_usize(row, "relation_changes"),
        actions: field_usize(row, "actions"),
        seed: field_u64(row, "seed"),
        built_by: crate::BuiltBy::from_name(&built).unwrap_or_else(|| {
            panic!("temporal profile `{id}` declares an unknown `built_by`: {built}")
        }),
        id,
    }
}

/// A required string field.
fn field_str(row: &serde_yaml_ng::Value, field: &str) -> String {
    row.get(field)
        .and_then(serde_yaml_ng::Value::as_str)
        .unwrap_or_else(|| panic!("a temporal profile row must declare `{field}`"))
        .to_owned()
}

/// A required unsigned field.
fn field_u64(row: &serde_yaml_ng::Value, field: &str) -> u64 {
    row.get(field)
        .and_then(serde_yaml_ng::Value::as_u64)
        .unwrap_or_else(|| panic!("a temporal profile row must declare `{field}`"))
}

/// A required count field.
fn field_usize(row: &serde_yaml_ng::Value, field: &str) -> usize {
    usize::try_from(field_u64(row, field)).expect("a cardinality fits a usize")
}

#[cfg(test)]
mod tests {
    use super::{Clock, TEMPORAL_PROFILE_T, VIRTUAL_NOW, declared_temporal_profiles};

    #[test]
    fn should_read_the_same_instant_when_two_runs_build_the_same_clock() {
        assert_eq!(Clock::fixed().now(), Clock::fixed().now());
        assert_eq!(Clock::fixed().now().to_string(), VIRTUAL_NOW);
    }

    #[test]
    fn should_leave_the_origin_where_it_was_when_the_clock_is_advanced() {
        let mut clock = Clock::fixed();
        let origin = clock.origin();
        clock.advance_hours(3);
        assert_eq!(clock.origin(), origin);
        assert_eq!(clock.now(), clock.at_second(3 * 60 * 60));
    }

    #[test]
    fn should_place_one_instant_per_step_when_a_cadence_is_asked_for() {
        let clock = Clock::fixed();
        let instants = clock.cadence(900, jiff::Span::new().seconds(1));
        assert_eq!(instants.len(), 900);
        assert_eq!(instants[0], clock.now());
        assert_eq!(instants[899], clock.at_second(899));
    }

    #[test]
    fn should_declare_the_same_fixture_cardinality_as_the_registry() {
        let declared = declared_temporal_profiles();
        let row = declared
            .iter()
            .find(|row| row.id == TEMPORAL_PROFILE_T.name)
            .expect("v0.5 section 49 requires a fixture ledger profile");
        assert_eq!(
            (
                row.events,
                row.objects,
                row.relation_changes,
                row.actions,
                row.seed
            ),
            (
                TEMPORAL_PROFILE_T.events,
                TEMPORAL_PROFILE_T.objects,
                TEMPORAL_PROFILE_T.relation_changes,
                TEMPORAL_PROFILE_T.actions,
                TEMPORAL_PROFILE_T.seed,
            ),
            "the constant and its declaration are two different fixtures"
        );
        assert_eq!(
            declared.len(),
            1,
            "the registry declares a temporal profile no constant names"
        );
    }
}
