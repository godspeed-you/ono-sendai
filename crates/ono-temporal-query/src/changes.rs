//! The changes engine (spec v0.5 §13): what differs between two points in time, as typed
//! `ono.temporal-change/1` values whose unknown sides stay unknown.
//!
//! §13.1 fixes the question — `changes [selector] --since <t> [--until <t>]`, with `--until`
//! omitted meaning the active temporal coordinate — and §13.2 fixes the answer: a stream of
//! canonical values in five classes, never a rendered diff.
//!
//! §13.4 is the rule the engine exists for. "If one side lacks enough evidence, the field MUST be
//! reported as unknown rather than fabricated." A field whose earlier side nothing observed comes
//! back with `before: None`, [`ChangeCertainty::Unknown`], and the composed coverage that
//! explains why — never a zero, never an empty string, never "changed to nothing".
//!
//! One engine answers three questions. §13.5 requires `look`'s recent-change section to be backed
//! by this one rather than by a second ad-hoc snapshot comparison, and §18.7's return-to-now
//! summary is [`summarise`] over the same values. Both call [`changes`].

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_core::{SpatialId, SpatialScope};
use ono_temporal_core::{
    ChangeCertainty, ChangeClass, CoverageQuery, CoverageSummary, EventKind, EventQuery,
    FieldChange, LedgerRead, QueryOrder, SpatialRef, TemporalEvent, TimeRange, presentation_order,
    value,
};
use ono_value::{
    ErrorValue, MapValue, Provenance, RecordBuilder, RecordValue, SchemaId, Value, builtin_schemas,
};

/// How many changes an answer carries before the caller has to narrow the question.
///
/// §32.3 budgets 150 ms p95 for an hour of changes, which is a promise about a bounded answer:
/// an unbounded one would break both that and §43.1.
pub const DEFAULT_LIMIT: usize = 500;

/// How many events one comparison may read (issue #150, ADR-1067).
///
/// A change is a fold over every event its subject had in the window — the earliest `before` and
/// the latest `after` of each field (§13.4) — so a comparison over part of a window is not a
/// smaller answer but a wrong one. The bound is therefore on the read, and a window that holds
/// more is refused rather than compared in part. Fifty thousand is an hour of the whole host at
/// §49's fixture rate (11.6 events a second) with room to spare. A window at the bound decoded
/// whole costs about 130 MB of resident memory (ADR-1068's measurement at 100k events) — bounded,
/// not small; a window over it is refused at index cost, before any event is decoded.
pub const MAX_WINDOW_EVENTS: usize = 50_000;

/// What a caller asks `changes` for (§13.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangesRequest {
    /// The boundary the comparison covers.
    pub scope: SpatialScope,
    /// The objects a selector named. Empty compares everything in the scope.
    pub subjects: Vec<SpatialId>,
    /// `--since`, which §13.1 makes required.
    pub since: Timestamp,
    /// `--until`. `None` means the active temporal coordinate (§13.1).
    pub until: Option<Timestamp>,
    /// How many changes to return. One more is returned where there are more, so a caller can
    /// say that the limit cut the answer rather than let it read as complete.
    pub limit: usize,
    /// How many events the comparison may read before it refuses ([`MAX_WINDOW_EVENTS`]).
    pub max_events: usize,
}

impl ChangesRequest {
    /// A request over `scope` from `since` to the active temporal coordinate.
    #[must_use]
    pub fn new(scope: SpatialScope, since: Timestamp) -> Self {
        Self {
            scope,
            subjects: Vec::new(),
            since,
            until: None,
            limit: DEFAULT_LIMIT,
            max_events: MAX_WINDOW_EVENTS,
        }
    }

    /// The same request restricted to `subjects`.
    #[must_use]
    pub fn about(mut self, subjects: Vec<SpatialId>) -> Self {
        self.subjects = subjects;
        self
    }
}

/// The edge a relation change is about (§6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationChange {
    /// The near end, as the event named it.
    pub from: SpatialRef,
    /// The far end.
    pub to: SpatialRef,
    /// The relation type, as v0.4's registry names it.
    pub relation: Arc<str>,
    /// How well the edge is known, where the source said (v0.4 §11.5).
    pub confidence: Option<Arc<str>>,
}

/// What became different about one subject between two instants (`ono.temporal-change/1`).
#[derive(Debug, Clone, PartialEq)]
pub struct TemporalChange {
    /// A stable identity for this change within the answer.
    pub change_id: Arc<str>,
    /// §13.2's class.
    pub class: ChangeClass,
    /// The object the change is about.
    pub subject: SpatialRef,
    /// The instant the comparison starts at — `--since`.
    pub from_time: Timestamp,
    /// The instant it ends at — `--until`, or the active coordinate.
    pub to_time: Timestamp,
    /// The typed field changes (§6.2). Empty for `added` and `removed`.
    pub field_changes: Vec<FieldChange>,
    /// The edge that changed, for a relation class (§6.4).
    pub relation: Option<RelationChange>,
    /// The composed coverage over the window (§8.5), which is what makes §13.4's unknown honest.
    pub coverage: CoverageSummary,
    /// Where the answer came from (v0.2 §25.2).
    pub provenance: Provenance,
}

impl TemporalChange {
    /// The `ono.temporal-change/1` record a pipeline consumes (§28.1).
    ///
    /// # Errors
    ///
    /// Returns `ono.provider_schema_violation` where the contract is not in this build.
    pub fn to_record(&self) -> Result<RecordValue, ErrorValue> {
        let schema_id = SchemaId::new("ono.temporal-change", 1);
        let schema = builtin_schemas().get(&schema_id).ok_or_else(|| {
            ErrorValue::new(
                ono_core::ErrorCode::ProviderSchemaViolation,
                "the `ono.temporal-change/1` contract is not in this build".to_owned(),
            )
        })?;
        let changes: Vec<Value> = self.field_changes.iter().map(value::field_change).collect();
        let builder = RecordValue::builder(schema, self.provenance.clone());
        let builder = put(builder, "change_id", Value::string(&self.change_id));
        let builder = put(builder, "kind", Value::string(self.class.as_str()));
        let builder = put(builder, "subject", value::spatial_ref(&self.subject));
        let builder = put(builder, "from_time", Value::Timestamp(self.from_time));
        let builder = put(builder, "to_time", Value::Timestamp(self.to_time));
        let builder = put(builder, "field_changes", Value::list(changes));
        let builder = put(
            builder,
            "relation",
            self.relation.as_ref().map_or(Value::Null, relation_value),
        );
        let builder = put(
            builder,
            "coverage",
            value::coverage_summary(&self.coverage)?,
        );
        let builder = put(builder, "provenance", provenance_value(&self.provenance));
        Ok(builder.build())
    }
}

/// The changes `request` asks `ledger` for, with `coordinate` standing in for an omitted
/// `--until` (§13.1).
///
/// At most `request.limit + 1` changes come back: one over the limit says the limit cut the
/// answer, which the caller reports rather than presenting the first `limit` as all of them.
///
/// # Errors
///
/// Returns whatever §34 refusal the ledger raises, and `resource.item_limit` where the window
/// holds more than `request.max_events` events — the ledger is asked for one more than that and
/// no further, so the bound is on the read and not only on the answer (issue #150).
pub fn changes(
    ledger: &dyn LedgerRead,
    request: &ChangesRequest,
    coordinate: Timestamp,
) -> Result<Vec<TemporalChange>, ErrorValue> {
    let until = request.until.unwrap_or(coordinate).max(request.since);
    let window = TimeRange::between(request.since, until);
    let query = EventQuery {
        scope: Some(request.scope.clone()),
        subjects: request.subjects.clone(),
        kinds: Vec::new(),
        range: window,
        limit: Some(request.max_events.saturating_add(1)),
        order: QueryOrder::Ascending,
    };
    // Decided at index cost before a body is decoded: a refused window costs a count of at most
    // `max_events + 1` index entries, not that many decoded events (review finding PRF-5).
    if ledger.count_events(&query)? > request.max_events {
        return Err(window_too_large(request, until));
    }
    let mut events = ledger.events(&query)?;
    if events.len() > request.max_events {
        return Err(window_too_large(request, until));
    }
    presentation_order(&mut events);

    let intervals = ledger.coverage(&CoverageQuery {
        scope: Some(request.scope.clone()),
        capabilities: Vec::new(),
        range: window,
    })?;
    let coverage = CoverageSummary::compose(&intervals, window);
    let provenance = Provenance::local("ono.temporal", SchemaId::new("ono.temporal-change", 1));

    let mut answer: Vec<TemporalChange> = Vec::new();
    for event in &events {
        if let Some(change) = relation_change(event, request, until, &coverage, &provenance) {
            answer.push(change);
        }
    }
    for group in group_by_subject(&events) {
        if let Some(change) = object_change(&group, request, until, &coverage, &provenance) {
            answer.push(change);
        }
    }
    answer.sort_by(|a, b| {
        a.from_time
            .cmp(&b.from_time)
            .then_with(|| a.class.cmp(&b.class))
            .then_with(|| a.subject.label().cmp(b.subject.label()))
            .then_with(|| a.change_id.cmp(&b.change_id))
    });
    answer.truncate(request.limit.saturating_add(1));
    Ok(answer)
}

/// §43.1's refusal for a window that holds more events than one comparison may read.
fn window_too_large(request: &ChangesRequest, until: Timestamp) -> ErrorValue {
    let bound = i128::try_from(request.max_events).unwrap_or(i128::MAX);
    ErrorValue::new(
        ono_core::ErrorCode::ResourceItemLimit,
        format!(
            "the window from {} to {until} holds more than {} events, and `changes` compares a \
             window whole or not at all",
            request.since, request.max_events
        ),
    )
    .with_help(
        "a change folds every event of its subject in the window, so a part of the window would \
         be a wrong answer rather than a shorter one; narrow `--since`/`--until` or name a target \
         (issue #150)",
    )
    .with_metadata("stage", Value::string("changes"))
    // The keys every other `resource.item_limit` refusal carries (`budget::Exceeded`), so a reader
    // of one reads them all the same way (review finding COR-T6).
    .with_metadata("ceiling", Value::string("values"))
    .with_metadata("limit", Value::Int(bound))
    .with_metadata("consumed", Value::Int(bound.saturating_add(1)))
    // No setting raises this bound; the key is there, and says so (§35.3: unknown is null).
    .with_metadata("setting", Value::Null)
    .with_metadata("since", Value::Timestamp(request.since))
    .with_metadata("until", Value::Timestamp(until))
}

/// One field transition worth naming in a return-to-now summary (§18.7).
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeHighlight {
    /// What a person calls the object.
    pub subject: Arc<str>,
    /// The field that moved.
    pub field: Arc<str>,
    /// What it was, where anything observed it.
    pub before: Option<Value>,
    /// What it became.
    pub after: Option<Value>,
}

/// What accumulated while the session was in the past (§18.7).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ChangeSummary {
    /// How many objects appeared.
    pub added: usize,
    /// How many went away.
    pub removed: usize,
    /// How many changed a field.
    pub changed: usize,
    /// How many relationships came into being.
    pub relations_added: usize,
    /// How many ended.
    pub relations_removed: usize,
    /// The net count per object type, sorted by type name — §18.7's `+3 processes`.
    pub net_by_type: Vec<(Arc<str>, i64)>,
    /// The field transitions a reader would want named — §18.7's `nginx.service active -> failed`.
    pub highlights: Vec<ChangeHighlight>,
}

/// Summarises `changes` the way §18.7's return to now does.
///
/// It is a reading of the same values `changes` produced, so the summary and the list can never
/// disagree: §13.5 asks for one change implementation, and this is a projection of it rather than
/// a second one.
#[must_use]
pub fn summarise(changes: &[TemporalChange]) -> ChangeSummary {
    let mut summary = ChangeSummary::default();
    let mut net: Vec<(Arc<str>, i64)> = Vec::new();
    for change in changes {
        match change.class {
            ChangeClass::Added => summary.added += 1,
            ChangeClass::Removed => summary.removed += 1,
            ChangeClass::Changed => summary.changed += 1,
            ChangeClass::RelationAdded => summary.relations_added += 1,
            ChangeClass::RelationRemoved => summary.relations_removed += 1,
        }
        let step = match change.class {
            ChangeClass::Added => 1,
            ChangeClass::Removed => -1,
            _ => 0,
        };
        if step != 0
            && let SpatialRef::Resolved { object_type, .. } = &change.subject
        {
            let name: Arc<str> = Arc::from(object_type.as_str());
            match net.iter_mut().find(|(held, _)| *held == name) {
                Some((_, count)) => *count += step,
                None => net.push((name, step)),
            }
        }
        if change.class == ChangeClass::Changed {
            for field in &change.field_changes {
                summary.highlights.push(ChangeHighlight {
                    subject: Arc::from(change.subject.label()),
                    field: Arc::clone(&field.field),
                    before: field.before.clone(),
                    after: field.after.clone(),
                });
            }
        }
    }
    net.sort_by(|a, b| a.0.cmp(&b.0));
    summary.net_by_type = net;
    summary
}

/// The events of one subject, in presentation order, borrowed from the window that was read.
struct SubjectRun<'a> {
    subject: &'a SpatialRef,
    events: Vec<&'a TemporalEvent>,
}

/// Splits `events` by the subject they are about, keeping each run in presentation order and
/// the runs in the order their subjects first appear.
///
/// One map lookup per event, and no event copied: a window of fifty thousand events over as many
/// subjects is grouped in time linear in the window (review finding PRF-5).
fn group_by_subject(events: &[TemporalEvent]) -> Vec<SubjectRun<'_>> {
    group_runs(events, |event| {
        if matches!(
            event.kind,
            EventKind::RelationAdded | EventKind::RelationRemoved
        ) {
            return None;
        }
        event.subject.as_ref()
    })
    .into_iter()
    .map(|(subject, events)| SubjectRun { subject, events })
    .collect()
}

/// Splits `items` into runs by `key`, each run in the order of `items` and the runs in the order
/// their keys first appear; an item with no key is left out. One map lookup per item, so the work
/// is linear in the items whatever the number of keys.
fn group_runs<'a, T, K: std::hash::Hash + Eq>(
    items: &'a [T],
    key: impl Fn(&'a T) -> Option<K>,
) -> Vec<(K, Vec<&'a T>)> {
    let mut runs: Vec<(K, Vec<&'a T>)> = Vec::new();
    let mut index: std::collections::HashMap<K, usize> = std::collections::HashMap::new();
    for item in items {
        let Some(found) = key(item) else {
            continue;
        };
        match index.entry(found) {
            std::collections::hash_map::Entry::Occupied(at) => {
                if let Some((_, run)) = runs.get_mut(*at.get()) {
                    run.push(item);
                }
            }
            std::collections::hash_map::Entry::Vacant(slot) => {
                let at = runs.len();
                // The map keeps the key it was asked by; the run keeps the item's own.
                if let Some(own) = key(item) {
                    runs.push((own, vec![item]));
                    slot.insert(at);
                }
            }
        }
    }
    runs
}

/// The change one subject's run of events amounts to over the window (§13.2).
///
/// The comparison is between two instants rather than a replay: the class comes from the last
/// lifecycle event in the window, and a field's two sides come from the first and last change to
/// it. A subject that appeared and went away inside the window reads as `removed`, because that
/// is what a reader standing at `--until` sees.
fn object_change(
    run: &SubjectRun<'_>,
    request: &ChangesRequest,
    until: Timestamp,
    coverage: &CoverageSummary,
    provenance: &Provenance,
) -> Option<TemporalChange> {
    let lifecycle = run
        .events
        .iter()
        .rev()
        .find(|event| {
            matches!(
                event.kind,
                EventKind::ObjectAppeared | EventKind::ObjectDisappeared
            )
        })
        .map(|event| event.kind);

    let class = match lifecycle {
        Some(EventKind::ObjectAppeared) => ChangeClass::Added,
        Some(EventKind::ObjectDisappeared) => ChangeClass::Removed,
        _ => ChangeClass::Changed,
    };

    let field_changes = if class == ChangeClass::Changed {
        fold_fields(&run.events)
    } else {
        Vec::new()
    };
    if class == ChangeClass::Changed && field_changes.is_empty() {
        return None;
    }

    Some(build(
        class,
        run.subject.clone(),
        request.since,
        until,
        field_changes,
        None,
        coverage,
        provenance,
    ))
}

/// The two sides of every field the run touched (§13.1, §13.4).
///
/// The earlier side is the first in-window observation's own `before`. Where the source gave
/// none, nothing observed that side, so the field comes back unknown with a null before — §13.4
/// forbids fabricating one, and §6.2 forbids reading the null as anything but unknown.
fn fold_fields(events: &[&TemporalEvent]) -> Vec<FieldChange> {
    let mut folded: Vec<FieldChange> = Vec::new();
    for event in events {
        for change in &event.changed_fields {
            match folded.iter_mut().find(|held| held.field == change.field) {
                Some(held) => {
                    held.after = change.after.clone();
                    held.certainty = certainty_of(held.before.as_ref(), held.certainty);
                }
                None => folded.push(FieldChange {
                    field: Arc::clone(&change.field),
                    before: change.before.clone(),
                    after: change.after.clone(),
                    certainty: certainty_of(change.before.as_ref(), change.certainty),
                }),
            }
        }
    }
    folded
}

/// How well the folded change is known, given whether anything observed the earlier side.
fn certainty_of(before: Option<&Value>, reported: ChangeCertainty) -> ChangeCertainty {
    if before.is_none() {
        ChangeCertainty::Unknown
    } else {
        reported
    }
}

/// The change a relation event amounts to (§6.4, §13.2).
fn relation_change(
    event: &TemporalEvent,
    request: &ChangesRequest,
    until: Timestamp,
    coverage: &CoverageSummary,
    provenance: &Provenance,
) -> Option<TemporalChange> {
    let class = match event.kind {
        EventKind::RelationAdded => ChangeClass::RelationAdded,
        EventKind::RelationRemoved => ChangeClass::RelationRemoved,
        _ => return None,
    };
    let from = event.subject.clone()?;
    let to = event.related.first().cloned()?;
    let payload = event.payload.as_ref().and_then(|value| value.as_map().ok());
    let relation: Arc<str> = payload
        .and_then(|map| map.get("relation"))
        .and_then(|value| value.as_str().ok())
        .map_or_else(|| Arc::from("related"), Arc::from);
    let confidence = payload
        .and_then(|map| map.get("confidence"))
        .and_then(|value| value.as_str().ok())
        .map(Arc::from);

    Some(build(
        class,
        from.clone(),
        request.since,
        until,
        Vec::new(),
        Some(RelationChange {
            from,
            to,
            relation,
            confidence,
        }),
        coverage,
        provenance,
    ))
}

/// Assembles a change and mints its identity.
#[allow(
    clippy::too_many_arguments,
    reason = "every part is a field of the value being built, and hiding them behind a struct \
              would only move the same list one line up"
)]
fn build(
    class: ChangeClass,
    subject: SpatialRef,
    from_time: Timestamp,
    to_time: Timestamp,
    field_changes: Vec<FieldChange>,
    relation: Option<RelationChange>,
    coverage: &CoverageSummary,
    provenance: &Provenance,
) -> TemporalChange {
    let change_id = identity(
        class,
        &subject,
        from_time,
        to_time,
        &field_changes,
        &relation,
    );
    TemporalChange {
        change_id,
        class,
        subject,
        from_time,
        to_time,
        field_changes,
        relation,
        coverage: coverage.clone(),
        provenance: provenance.clone(),
    }
}

/// The stable identity of one computed change.
///
/// A change is an answer over a window rather than a row in the ledger, so its identity is a
/// digest of the question and the answer: the same comparison run twice produces the same
/// reference, and two different comparisons produce two.
fn identity(
    class: ChangeClass,
    subject: &SpatialRef,
    from_time: Timestamp,
    to_time: Timestamp,
    field_changes: &[FieldChange],
    relation: &Option<RelationChange>,
) -> Arc<str> {
    let mut token = String::new();
    token.push_str(class.as_str());
    token.push('\u{1}');
    token.push_str(
        subject
            .spatial_id()
            .map_or_else(|| subject.label().to_owned(), |id| id.as_str().to_owned())
            .as_str(),
    );
    token.push('\u{1}');
    token.push_str(&from_time.as_nanosecond().to_string());
    token.push('\u{1}');
    token.push_str(&to_time.as_nanosecond().to_string());
    for change in field_changes {
        token.push('\u{1}');
        token.push_str(&change.field);
        token.push('\u{2}');
        token.push_str(&render(change.before.as_ref()));
        token.push('\u{2}');
        token.push_str(&render(change.after.as_ref()));
    }
    if let Some(edge) = relation {
        token.push('\u{1}');
        token.push_str(&edge.relation);
        token.push('\u{2}');
        token.push_str(edge.to.label());
    }
    Arc::from(format!("c{:016x}", fnv1a(&token)))
}

/// A value as identity text; an unobserved side renders as nothing, which is not an empty value.
fn render(value: Option<&Value>) -> String {
    value.map_or_else(String::new, |value| format!("{value:?}"))
}

/// FNV-1a over `text`.
///
/// A change identity names an answer inside one session's output rather than a record anybody
/// stores, so a short non-cryptographic digest is the right size for it. The ledger's own
/// identities are SHA-256 in `ono-temporal-core`, and nothing here is one of those.
fn fnv1a(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

/// One relation change as the sub-record `ono.temporal-change/1` carries (§6.4).
fn relation_value(relation: &RelationChange) -> Value {
    let mut map = MapValue::new();
    map.insert("from".into(), value::spatial_ref(&relation.from));
    map.insert("to".into(), value::spatial_ref(&relation.to));
    map.insert("relation".into(), Value::string(&relation.relation));
    map.insert(
        "confidence".into(),
        relation
            .confidence
            .as_deref()
            .map_or(Value::Null, Value::string),
    );
    Value::Map(Arc::new(map))
}

/// Sets a field the schema declares; an undeclared name is a bug here rather than a caller's.
fn put(builder: RecordBuilder, name: &str, value: Value) -> RecordBuilder {
    let fallback = builder.clone();
    builder.set(name, value).unwrap_or(fallback)
}

/// Provenance as the nested record every schema of §35 declares (v0.2 §25.2).
fn provenance_value(provenance: &Provenance) -> Value {
    let mut map = MapValue::new();
    map.insert("provider".into(), Value::string(provenance.provider()));
    map.insert(
        "observed".into(),
        provenance.observed().map_or(Value::Null, Value::Timestamp),
    );
    map.insert(
        "source".into(),
        provenance.source().map_or(Value::Null, Value::string),
    );
    map.insert("link".into(), Value::string(&provenance.link().to_string()));
    map.insert(
        "schema".into(),
        Value::string(&provenance.schema().to_string()),
    );
    Value::Map(Arc::new(map))
}

#[cfg(test)]
mod grouping {
    use std::cell::Cell;

    use super::group_runs;

    /// A key that counts every comparison made of it.
    struct Counted<'a> {
        id: u32,
        comparisons: &'a Cell<usize>,
    }

    impl PartialEq for Counted<'_> {
        fn eq(&self, other: &Self) -> bool {
            self.comparisons.set(self.comparisons.get() + 1);
            self.id == other.id
        }
    }

    impl Eq for Counted<'_> {}

    impl std::hash::Hash for Counted<'_> {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.id.hash(state);
        }
    }

    #[test]
    fn should_group_ten_thousand_items_over_five_thousand_keys_in_work_linear_in_the_items() {
        // Review findings PRF-5, R2C-5: grouping searched every run for every event, which is
        // twelve and a half million comparisons for ten thousand events over five thousand
        // subjects. The work is a bound the test can fail on: at most a few comparisons per item.
        let comparisons = Cell::new(0);
        let items: Vec<u32> = (0..10_000).map(|index| index % 5_000).collect();
        let runs = group_runs(&items, |item| {
            Some(Counted {
                id: *item,
                comparisons: &comparisons,
            })
        });
        assert_eq!(runs.len(), 5_000, "one run per key");
        assert!(
            runs.iter().all(|(_, run)| run.len() == 2),
            "every item lands in its key's run"
        );
        assert_eq!(
            runs.first().map(|(key, _)| key.id),
            Some(0),
            "runs in the order their keys first appear"
        );
        assert!(
            comparisons.get() <= 2 * items.len(),
            "grouping {} items over {} keys made {} comparisons; it must be linear in the items",
            items.len(),
            runs.len(),
            comparisons.get()
        );
    }
}
