//! The provider event merge of spec v0.4 §25.1: "`map --live` MUST subscribe to available
//! provider events and/or explicit polling sources."
//!
//! The events are the ones the v0.2 watch runtime already emits (v0.2 §18.2, ADR-0024): a stream
//! begins with `snapshot` events carrying the current state, then reports `added`, `changed` and
//! `removed`, and every event says through `source` whether it was seen by `subscription` or by
//! `poll`. §2.16 forbids the spatial layer from becoming a second source of system truth, so this
//! merge reads those events and invents nothing beside them.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "a test states its preconditions directly (AGENTS.md section 16)"
)]

use std::sync::Arc;

use jiff::Timestamp;
use ono_spatial_events::{EventKind, EventMerge, Freshness};
use ono_value::{Provenance, RecordValue, SchemaId, Value, builtin_schemas};

/// One `ono.socket-event/1` exactly as the v0.2 watch runtime writes it (ADR-0024).
fn watch_event(kind: &str, source: &str, inode: i128) -> Value {
    watch_event_at(kind, source, inode, Timestamp::UNIX_EPOCH)
}

/// The same envelope, observed at a stated instant.
fn watch_event_at(kind: &str, source: &str, inode: i128, at: Timestamp) -> Value {
    let schema = builtin_schemas()
        .get(&SchemaId::new("ono.socket-event", 1))
        .expect("the workspace carries the socket event contract");
    let socket = builtin_schemas()
        .get(&SchemaId::new("ono.socket", 1))
        .expect("the workspace carries the socket contract");
    let object = RecordValue::builder(
        socket,
        Provenance::local("linux.sock-diag", SchemaId::new("ono.socket", 1)),
    )
    .set("inode", Value::Int(inode))
    .expect("`inode` is a declared field")
    .build();
    let event = RecordValue::builder(
        schema,
        Provenance::local("ono.runtime", SchemaId::new("ono.socket-event", 1)),
    )
    .set("kind", Value::string(kind))
    .expect("`kind` is a declared field")
    .set("at", Value::Timestamp(at))
    .expect("`at` is a declared field")
    .set("socket", Value::Record(Arc::new(object)))
    .expect("`socket` is a declared field")
    .set("source", Value::string(source))
    .expect("`source` is a declared field")
    .build();
    Value::Record(Arc::new(event))
}

#[test]
fn should_read_the_watch_runtimes_own_event_envelope_rather_than_a_second_vocabulary() {
    // §2.16: the spatial layer composes provider data. The events it merges are the ones v0.2
    // §18.2 already defines, field for field, so nothing has to be translated twice.
    let mut merge = EventMerge::new();

    let observed = merge
        .absorb(&watch_event("added", "poll", 4711))
        .expect("an `ono.socket-event/1` record is an event this merge understands");

    assert_eq!(observed.kind(), EventKind::Added);
    assert_eq!(
        observed
            .object()
            .and_then(|record| record.get("inode").cloned()),
        Some(Value::Int(4711)),
        "the object the event carries is the provider's own record"
    );
}

#[test]
fn should_ignore_a_value_that_is_not_an_event_rather_than_inventing_a_change_for_it() {
    // §24.3: "No fake change summary may be generated when no event source or comparison snapshot
    // exists." A plain object arriving where an event was expected is not a change.
    let mut merge = EventMerge::new();

    assert!(
        merge.absorb(&Value::string("not an event")).is_none(),
        "spec §24.3: a value that is not an event announces nothing"
    );
    assert!(
        merge.absorb(&Value::Null).is_none(),
        "spec §24.3: neither does an absent one"
    );
}

#[test]
fn should_report_the_view_as_polled_when_every_event_says_the_runtime_polled_for_it() {
    // §25.3 and v0.2 §18.2: polling is explicit. A view fed by a polling runtime is `polled`, and
    // calling it `event_driven` would promise a liveness nothing delivers (§2.12).
    let mut merge = EventMerge::new();
    merge.absorb(&watch_event("snapshot", "poll", 1));
    merge.absorb(&watch_event("added", "poll", 2));

    assert_eq!(merge.freshness(), Freshness::Polled);
    assert_eq!(merge.freshness().as_str(), "polled");
}

#[test]
fn should_report_the_view_as_event_driven_when_the_provider_subscribed_for_every_source() {
    // §25.3's first word, and the reason the merge reads `source` at all: the day a provider
    // grows a real subscription, the view says so without another change here.
    let mut merge = EventMerge::new();
    merge.absorb(&watch_event("snapshot", "subscription", 1));
    merge.absorb(&watch_event("added", "subscription", 2));

    assert_eq!(merge.freshness(), Freshness::EventDriven);
}

#[test]
fn should_report_the_weaker_freshness_when_one_source_polls_and_another_subscribes() {
    // §25.3 describes one view, and a view is only as live as its slowest source. Claiming the
    // stronger word would describe the half of the picture that happens to be fastest.
    let mut merge = EventMerge::new();
    merge.absorb(&watch_event("snapshot", "subscription", 1));
    merge.absorb(&watch_event("snapshot", "poll", 2));

    assert_eq!(
        merge.freshness(),
        Freshness::Polled,
        "spec §25.3: a view with one polled source is a polled view"
    );
}

#[test]
fn should_report_the_view_as_cached_until_the_first_event_has_arrived() {
    // §25.3: `cached` is the honest word for a view showing what was read before anything is
    // watching it. §2.17 makes the difference between "nothing has happened" and "nothing has
    // been observed" one the user must be able to see.
    let merge = EventMerge::new();

    assert_eq!(merge.freshness(), Freshness::Cached);
    assert!(
        !merge.is_settled(),
        "spec §18.2/ADR-0024: a stream is settled once its opening snapshot has arrived"
    );
}

#[test]
fn should_settle_once_the_opening_snapshot_of_the_stream_has_arrived() {
    // ADR-0024: "a subscription always begins with the current state as `snapshot` events". Until
    // they have all arrived, an `added` cannot be told from a state the view simply had not seen
    // yet — which is the difference between §43.6's real change and a startup artefact.
    let mut merge = EventMerge::new();
    merge.absorb(&watch_event("snapshot", "poll", 1));

    assert!(merge.is_settled());
}

#[test]
fn should_name_a_removal_as_a_removal_so_the_place_can_be_tombstoned() {
    // §10.3: a removed object may remain as a tombstone. The merge is where the removal is seen;
    // §25.1 lists "node appearance/removal" among what a live map reflects.
    let mut merge = EventMerge::new();
    merge.absorb(&watch_event("snapshot", "poll", 1));

    let observed = merge
        .absorb(&watch_event("removed", "poll", 1))
        .expect("a removal is an event");

    assert_eq!(observed.kind(), EventKind::Removed);
    assert!(
        observed.object().is_some(),
        "spec §10.3: a tombstone needs the object as it last was, so the removal carries it"
    );
}

/// The same envelope as [`watch_event`], with the observation time left out.
///
/// v0.5 §3.3 keeps source time, observed time and ingestion time apart and forbids collapsing
/// them, so a stream that states no time must produce an event that states no time.
fn watch_event_without_time(kind: &str, source: &str) -> Value {
    let schema = builtin_schemas()
        .get(&SchemaId::new("ono.socket-event", 1))
        .expect("the workspace carries the socket event contract");
    let event = RecordValue::builder(
        schema,
        Provenance::local("ono.runtime", SchemaId::new("ono.socket-event", 1)),
    )
    .set("kind", Value::string(kind))
    .expect("`kind` is a declared field")
    .set("source", Value::string(source))
    .expect("`source` is a declared field")
    .build();
    Value::Record(Arc::new(event))
}

/// The `changed` envelope of v0.2 §18.2: the names of the fields whose values moved.
fn changed_event(fields: Option<Vec<&str>>) -> Value {
    let schema = builtin_schemas()
        .get(&SchemaId::new("ono.socket-event", 1))
        .expect("the workspace carries the socket event contract");
    let mut event = RecordValue::builder(
        schema,
        Provenance::local("ono.runtime", SchemaId::new("ono.socket-event", 1)),
    )
    .set("kind", Value::string("changed"))
    .expect("`kind` is a declared field")
    .set("at", Value::Timestamp(Timestamp::UNIX_EPOCH))
    .expect("`at` is a declared field")
    .set("source", Value::string("poll"))
    .expect("`source` is a declared field");
    if let Some(fields) = fields {
        event = event
            .set(
                "changed",
                Value::list(fields.into_iter().map(Value::string).collect::<Vec<_>>()),
            )
            .expect("`changed` is a declared field");
    }
    Value::Record(Arc::new(event.build()))
}

#[test]
fn should_carry_the_observation_time_when_the_envelope_states_one() {
    // v0.5 §3.3: an event's `observed_at` is when the observing component detected it. The watch
    // envelope has carried it since v0.2 §18.2; throwing it away leaves a change nothing
    // downstream can place in time without inventing a timestamp.
    let mut merge = EventMerge::new();
    let at = Timestamp::from_second(1_756_000_000).expect("a representable instant");

    let observed = merge
        .absorb(&watch_event_at("added", "poll", 4711, at))
        .expect("an `ono.socket-event/1` record is an event this merge understands");

    assert_eq!(
        observed.at(),
        Some(at),
        "spec v0.5 §3.3: the instant the provider saw it is the event's own, not a later one"
    );
}

#[test]
fn should_report_no_observation_time_when_the_envelope_states_none() {
    // v0.5 §3.3: an absent observation time is a fact. Substituting one would make the ledger
    // claim knowledge no source supplied.
    let mut merge = EventMerge::new();

    let observed = merge
        .absorb(&watch_event_without_time("added", "poll"))
        .expect("an envelope without `at` is still an event");

    assert_eq!(
        observed.at(),
        None,
        "spec v0.5 §3.3: an event with no stated time says so rather than borrowing one"
    );
}

#[test]
fn should_carry_the_changed_field_names_the_envelope_lists() {
    // v0.5 §6.2: `object.changed` carries typed field changes where known, and v0.2 §18.2's
    // `changed` is exactly that list of names.
    let mut merge = EventMerge::new();

    let observed = merge
        .absorb(&changed_event(Some(vec!["state", "queue"])))
        .expect("a `changed` envelope is an event");

    assert_eq!(
        observed.changed_fields(),
        ["state", "queue"],
        "spec v0.5 §6.2: the fields whose values moved travel with the event"
    );
}

#[test]
fn should_report_no_changed_fields_when_the_envelope_lists_none() {
    // v0.2 §18.2: `changed` is null for every kind but `changed`. An empty list is the honest
    // answer, and §35.3 forbids fabricating names for it.
    let mut merge = EventMerge::new();

    let observed = merge
        .absorb(&changed_event(None))
        .expect("an envelope without `changed` is still an event");

    assert!(
        observed.changed_fields().is_empty(),
        "spec v0.2 §18.2: no field list is an empty field list, got {:?}",
        observed.changed_fields()
    );
}
