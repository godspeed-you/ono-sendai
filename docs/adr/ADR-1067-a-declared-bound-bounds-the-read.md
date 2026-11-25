# ADR-1067: A command's declared bound bounds the read, and a cut answer says so

- Status: accepted
- Date: 2026-11-24
- Spec refs: v0.5 §13.1, §13.2, §13.4, §13.5, §32.3, §43.1; v0.4.1 §21.4
- Issues: #150
- Decided by: agent (autonomous)

## Context

`changes` read its whole window from the ledger — `EventQuery { limit: None }` in
`ono_temporal_query::changes::changes` — and then cut the *answer* to `DEFAULT_LIMIT` (500)
changes, silently. On §49's million-event fixture an hour of the whole host is ~42,000 events,
all decoded into memory to produce at most 500 values, and a window of a day is the store. The
same shape had been fixed for `why` (`temporal.why.max_candidates` now reaches the ledger). Issue
#150 asks that every command's declared bound reach the query, and that an answer a bound cut
say so rather than read as complete.

`changes` cannot simply read the first *n* events of its window. A `TemporalChange` folds every
event its subject had in the window — the earliest `before` and the latest `after` of each field
(§13.4), appearance and disappearance — so a comparison over part of the window is not a shorter
answer, it is a wrong one: a subject whose later events were not read would report a stale
`after`. §13 promises an exact comparison.

## Decision

1. **A work bound on the read: `MAX_WINDOW_EVENTS` = 50,000.** `ChangesRequest` carries
   `max_events`, defaulting to it. The ledger is asked for `max_events + 1` events and no more. A
   window that holds more is refused with `resource.item_limit` (E1101, v0.4.1 §21.4), whose
   metadata names the stage (`changes`), the limit, the consumption that crossed it and the
   window, and whose help says to narrow `--since`/`--until` or name a target. 50,000 is an hour
   of the whole host at the fixture's rate (11.6 events a second) with headroom. *Corrected after
   review (PRF-5, COR-T6):* decoded, a window at the bound is not "tens of megabytes" — ADR-1068
   measured 130 MB resident at 100k events with the refusal decoding 50,001 events (1,151 ms) to
   find out. A window is now decided at index cost first: `LedgerRead::count_events` counts at
   most `max_events + 1` index entries (`SELECT COUNT(*) FROM (SELECT 1 … LIMIT ?)` in the store,
   no payload read), and only a window inside the bound is read and decoded. Grouping the window
   by subject is a map lookup per event over borrowed events (it searched every run for every
   event and copied each event into its run). The refusal carries the metadata keys every
   `resource.item_limit` refusal carries — `stage`, `ceiling` (`values`), `limit`, `consumed`,
   `setting` (null: no setting raises this bound) — plus the window.
2. **The answer bound is reported, not hidden.** `changes` returns at most `limit + 1` changes;
   the extra one says the limit cut the answer — the idiom `timeline` already uses at the ledger
   (`limit + 1` → `truncated`). The `changes` command prints the first `limit` and says on the
   diagnostic stream that more changed than it shows, as `timeline` says "the list was truncated
   by a limit". Every value printed is exact; the list is declared incomplete.

### The class, read

`limit: None` and `EventQuery::default()` in `crates/ono-temporal-query`, `crates/ono-cli/src/
temporal`, `crates/ono-temporal-reconstruct` and `crates/ono-recorder`:

| Site | What it reads | Outcome |
|---|---|---|
| `changes::changes` | the whole window | **fixed** (above) |
| `temporal::session::carry_over` | the whole ledger being replaced | fixed in #147's increment (ADR-1068) |
| `search::find_place_at` (`find place` at a historical coordinate, §14.4) | every event of the scope up to the coordinate | **left**: it is a label search with no index on labels and its answer is complete by contract; a bounded read would make it silently partial. Its work is bounded by retention (`temporal.retention.max_age`, `max_size`). Recorded as a finding. |
| `Reconstructor::reconstruct` | events from the nearest trusted checkpoint, `until: None` | **left**: replay must see every event since the checkpoint to be exact, and the checkpoint cadence (§31.9, 5 min) bounds it; reading past the coordinate (`until: None`) is wasted work but not unbounded growth with the store. Recorded as a finding. |
| `Recorder::recorded_gaps` (`Recorder::gaps`) | every `coverage.ended` marker | **left**: no command reaches it; only tests call `gaps()`. |

`timeline` (`limit + 1`), `find event` (`--limit`, default 500) and `why`
(`temporal.why.max_candidates`) already passed their bound to the ledger.

## Consequences

- A window holding more than 50,000 events is refused by `changes`, by `look`'s change section
  (§13.5) and by the full-screen timeline's return-to-now summary, all of which call the one
  engine (§13.5 forbids a second). Before, they read the whole window and answered. *Corrected
  after review (COR-T7):* `look --changes <window>` propagated the refusal and failed the whole
  `look`; its change section is now `state: unknown` with the refusal as the section's new
  optional `reason` (`ono.change-summary/1`, a nullable field added — compatible), and the rest of
  the view answers.
- `look`'s change section and the return-to-now summary receive `limit + 1` changes when the
  limit cuts, and do not yet say so; they are §13.5's summaries rather than the answer `changes`
  promises, and their rendering is spatial code outside this increment. Recorded as a finding.
- Tests: `crates/ono-temporal-query/tests/changes.rs` —
  `should_read_no_more_than_its_bound_when_the_window_holds_more_events` (a ledger double that
  counts the events it hands over observes the bound at the ledger boundary),
  `should_compare_the_whole_window_when_it_holds_no_more_than_the_bound`,
  `should_return_one_change_past_the_limit_when_the_limit_cuts_the_answer`.

## Alternatives considered

- **Read the newest `n` events and compare those.** Approximate where the contract is exact.
- **Push the fold into SQL** (first and last value per subject and field). Exact and bounded by
  subjects rather than events, but it needs a per-field index the store does not have and a second
  implementation of §13.4's fold beside the engine's; not a stabilization change.
- **A setting for the bound.** §33 lists no such setting; adding one is a contract addition for a
  figure nobody has asked to tune.
