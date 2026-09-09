# ADR-0661: The default timeline is a place and a window rather than a firehose

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §11.2, §11.3, §11.7, §11.8, §18.4, §32.3, §33, §43.1
- Decided by: agent (autonomous)

## Context

§11.3 gives `timeline` three different default scopes in three sentences — the current place and
its directly relevant events, high-significance events and current-session actions at the root,
and the full visible scope under `--all` — without saying what "directly relevant" or
"high-significance" mean. §11.8 adds a fourth variable: in historical context the window centres
on the coordinate rather than ending at now. §32.3 then budgets 100 ms p95 for fifteen minutes of
it, which is a statement about how the query is shaped rather than about how fast the filter is.

## Decision

**A `Horizon` is what a timeline is scoped to, and it answers both questions the planner asks: the
subjects to push into the ledger query, and whether an event belongs in the answer.**

1. **Four horizons.** `at_place` (a place and its directly relevant objects), `at_root`,
   `selecting` (a selector named subjects), and `everything` (`--all`). `Horizon::subjects()` is
   what the planner pushes down; it is empty for the root and for `--all`, where the window and
   the significance rule bound the answer instead.
2. **"Directly relevant" is the caller's neighbourhood.** `ono-temporal-query` reaches for no
   spatial index (§39.3), so the caller — which already computed the neighbourhood to draw `look`
   — hands the neighbour identities in. An event touching the place or a neighbour, as subject or
   as a related end, is in scope.
3. **"High-significance" is §18.4's own top four.** Node lifecycle, relation lifecycle, service and
   container state, landmark change. Operator actions from this session are in beside them,
   because §11.3 names them separately. A field moving by a byte is out, which is the firehose
   §11.3 refuses.
4. **The window.** Explicit `--since`/`--until` win. Otherwise historical context centres the
   window on the coordinate ±15 minutes (§11.8); the present takes the 30 minutes ending at `now`,
   which is `temporal.timeline.default_window` (§33). `now` is a parameter (§39.2).
5. **The query carries the ceiling plus one.** `plan` asks the ledger for `limit + 1` events, so
   `timeline` distinguishes a window that ended from an answer that was cut without a second
   query, and `truncated` on `ono.temporal-timeline/1` is answered honestly.
6. **Gaps come from coverage, never from the events.** The gaps in a timeline are
   `CoverageSummary::compose`'s gaps over the window. Nothing looks at where the events are, so
   §11.7's "a gap MUST not be hidden simply because events exist on both sides" holds by
   construction rather than by a rule somebody remembered to write.

## Consequences

- The whole default-scope judgement is one function, `relevance::is_default_scope`, testable
  against two events built by hand with no provider and no place index.
- The planner pushes the scope, the subjects, the kinds and the window into one `EventQuery`, so a
  ledger with an index answers a bounded question. A million-event ledger is never materialised.
- The relevance filter still runs over what came back, because the ledger's `subjects` restriction
  cannot express "high-significance at the root". That set is bounded by the window and the limit.
- `--all` widens the subjects and nothing else. Retention and permission remain the ledger's to
  apply, which is where they can be applied honestly.
- Encoded by `crates/ono-temporal-query/tests/timeline.rs` and `tests/relevance.rs`.

## Alternatives considered

- **Reading the spatial neighbourhood in this crate.** It would need `ono-spatial-query` and a live
  index, which §39.3 forbids and which would make a timeline untestable without a machine.
- **Filtering a materialised event set.** Simpler to write and it misses §32.3 by the width of the
  ledger.
- **Making the root scope "everything, ranked".** It reads well and it is the firehose §11.3 names.
