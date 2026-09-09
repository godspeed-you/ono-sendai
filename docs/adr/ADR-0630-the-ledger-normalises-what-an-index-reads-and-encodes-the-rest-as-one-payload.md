# ADR-0630: The ledger normalises what an index reads and encodes the rest as one payload

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §31.3, §31.4, §31.8, §32.3, §39.4; ADR-0620, ADR-0621
- Decided by: agent (autonomous)

## Context

§31.3 lists ten logical sets and then says the physical schema "MAY normalize differently". §31.4
says typed payloads should be a versioned binary encoding "while indexed scalar metadata remains
relational". §32.3 budgets six queries. Those three sentences together decide the schema, and the
only real question is where the line between column and payload falls.

A `TemporalEvent` has seventeen fields. Six of them are what a query filters, orders or joins on:
the instant a human navigates by, the scope, the kind, the subject, the source and the identity.
The other eleven — the subtype, the related subjects, the before and after values, the field
changes, the evidence list, the causal parents, the provider body, the provenance — are read whole
or not at all. Putting the second group in columns would buy nothing and cost a migration for every
shape a provider invents; putting the first group in a payload would make §32.3's budgets
unreachable, because answering "the last fifteen minutes here" would mean decoding everything.

## Decision

### 1. A column is a thing a query looks at; everything else is one CBOR payload per row

`events` carries `event_id`, `kind`, `scope_path`, `subject`, `presentation_nanos`, `source_nanos`,
`observed_nanos`, `ingested_nanos`, `source_sequence`, `monotonic_nanos`, `uncertainty_nanos`,
`domain_host`, `domain_boot`, `source` and one `body` blob. The three timestamps stay three columns
because §3.3 forbids collapsing them and because a query on one of them is a query on one of them.
`evidence`, `actions` and `checkpoints` follow the same split. `causal_links` and
`coverage_intervals` are entirely relational, because every field of both is something a query
filters on.

### 2. The ten logical sets are present, plus four join tables that exist for retention

`events`, `evidence`, `causal_links`, `coverage_intervals`, `checkpoints`, `checkpoint_objects`,
`checkpoint_relations`, `source_sequences`, `actions` and `metadata` are tables under those names,
and `LedgerStore::logical_sets` answers with them so §31.3 is a test rather than a claim.

Beside them are `event_subjects`, `event_evidence`, `causal_link_evidence` and
`evidence_derivation`. They exist for one reason: §31.8 requires retention to remove orphaned
evidence "without leaving invalid references", and finding an orphan by decoding every surviving
payload would be unbounded work over a bounded deletion. As join tables it is a row count. The same
tables make §11.2's subject filter and §32.3's causal lookup index scans.

### 3. A scope is a prefix-comparable path, so "one place" is a range scan

`scope_path` renders a scope and its ancestors outermost first as `kind␞id␞boot␟`, repeated, with
the terminator after every level. `SpatialScope::contains(other)` is then exactly "this path is a
prefix of that one", and a scope filter is `scope_path >= :prefix AND scope_path < :upper`, which
`events_by_place (scope_path, presentation_nanos)` answers directly. The trailing separator is what
makes it exact: without it `host␞web0␟` would be a prefix of `host␞web01␟`.

### 4. The indexes are the six queries §32.3 budgets, and the gate checks the plans

| §32.3 query | index |
|---|---|
| timeline, one place, 15 minutes | `events_by_place (scope_path, presentation_nanos)` |
| changes, 1 hour | `events_by_time (presentation_nanos)`, `events_by_kind (kind, presentation_nanos)` |
| nearest checkpoint before an instant | `checkpoints_by_place (scope_path, captured_nanos)` |
| event by id | the `events` primary key, as a prefix range for §11.6's shortened reference |
| causal links by effect | `causal_links_by_effect (effect)` |
| retention's oldest-first sweep | `events_by_time (presentation_nanos)` |

`LedgerStore::explain_*_plan` returns SQLite's own plan for each, and `tests/schema.rs` asserts that
none of them is a table scan. An index that stops being used becomes a red test rather than a slow
release.

### 5. The payload carries its own schema id and version

Every blob is `["ono.temporal-ledger-payload", 1, body]`. A blob naming a version this Ono does not
know fails its row with a diagnostic rather than being guessed at, which is what §31.4's "MUST carry
a schema/version identifier" is for.

### 6. Records inside a payload are read back against a registry

A `RecordValue` is stored as its schema id, its provenance, its declared fields by name and its
provider extensions. On read the schema id is resolved through the registry the store was opened
with, which defaults to `ono_value::builtin_schemas()` and can be widened with
`StoreOptions::with_schemas` for contributed schemas. A field the schema no longer declares survives
as an extension rather than being dropped; a schema the registry does not know at all fails the row.

`Provenance`'s adapter trace is deliberately not stored: it names an external executable and the
arguments it was run with, which is the material §30.4 refuses to bulk-record.

## Consequences

Adding a queryable dimension is a migration; adding a field a query never looks at is not. That is
the right way round, because providers invent fields and the shell invents queries far more slowly.

A payload is opaque to SQL, so nothing outside this crate can filter on what is inside one. That is
§39.4 working as intended: the layer above holds `LedgerRead`.

Retention's orphan sweep is a set operation over four small tables rather than a decode of the whole
ledger, which is what makes §31.8's bounded work bounded.

Tests: `tests/schema.rs` (the ten sets and the six plans), `tests/persistence.rs` (every `Value`
kind round-trips, and an unresolved subject stays unresolved), `tests/query.rs`, `tests/scale.rs`.

## Alternatives considered

**One table per §31.3 name and nothing else.** Rejected: without the join tables, retention's orphan
sweep decodes every surviving payload, which §31.8 forbids.

**Everything relational, no payload.** Rejected: a provider body is an arbitrary `Value`, so it
would need a generic key/value table, and reading one event would become a join with an unbounded
row count.

**Everything in one payload, with only an instant indexed.** Rejected: §32.3's place, kind and
subject filters would each become a full decode.

**Store the schema definition beside each record rather than resolving it.** Rejected as speculative
generality: every schema in play ships with the shell or is loaded by the host that opens the store,
and `with_schemas` covers the second case with no per-row cost.
