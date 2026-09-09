# ADR-0664: `find event` is planned here and evaluated by the pipeline

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §20.1, §20.2, §20.3, §20.4, §28.1, §32.3, §39, §43.1
- Decided by: agent (autonomous)

## Context

§20.3 adds `find event <predicate>` and constrains it in one sentence: it "reuses the existing
`find` verb and Ono expression semantics rather than inventing a new search language". The
expression language is `ono-parser` and `ono-command`. `ono-temporal-query` depends on neither and
must not: §39 puts it below the command layer, and a query planner that could evaluate an Ono
expression would be a second evaluator to keep in step with the first.

§32.3 nevertheless budgets 150 ms p95 for "find event indexed predicate", which is a promise that
*something* narrows the question before the ledger answers it.

## Decision

**This crate plans the search; the pipeline evaluates the predicate.**

`SearchHints` is what a caller can read off a parsed predicate without evaluating it: the scope,
the subjects it named, the kinds it fixed, the window it implied, a ceiling and an order. `plan`
turns those into one `EventQuery` with every restriction pushed down, and `find_events` runs it.
Whatever the hints could not express is applied by the same `where` a pipeline already applies to
every other stream, over the bounded set the planner produced (§28.1).

The hints are deliberately not a predicate type. There is no expression AST here, no comparison
operators, no field-path language — those exist once, in the parser, and this crate would only be
able to disagree with them.

A search with no ceiling still takes `DEFAULT_LIMIT`, because §43.1 has no unbounded answers and
§32.3 has no unbounded budgets.

## Consequences

- `find event 'kind == "action.failed"'` reaches the ledger as a kind restriction and comes back
  bounded; `find event 'subject.name contains "nginx"'` reaches it as a window and is filtered by
  the pipeline. Both are one command to the user and neither invents a syntax.
- Agent H's CLI owns the extraction from the parsed expression into `SearchHints`, and the quality
  of the extraction is a performance property rather than a correctness one: a hint the extractor
  misses costs time, and a hint it invents would be a defect, so the extractor may only narrow.
- §20.4's completion is in the same module and uses the same relevance judgement as the timeline,
  so a candidate list and a default timeline agree about what matters at this place.
- Encoded by `crates/ono-temporal-query/tests/search.rs`.

## Alternatives considered

- **Embedding a small predicate language here.** It is the thing §20.3 names and refuses, and it
  would drift from the real one within an increment.
- **Depending on `ono-command` from `ono-temporal-query`.** It inverts §39's layering and makes the
  query crate untestable without the command registry.
