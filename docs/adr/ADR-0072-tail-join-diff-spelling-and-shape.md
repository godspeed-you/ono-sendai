# ADR-0072: `tail`, `join` and `diff` — spelling, output shape, and the right-hand side

- Status: accepted
- Date: 2026-08-27
- Spec refs: §11.1, §12.1, §28.1, §33.2, §35.3, §41.4, §53; ADR-0009, ADR-0012, ADR-0028
- Decided by: agent (autonomous)

## Context

`docs/contracts/commands/data.yaml` reserved `ono.data.tail`, `ono.data.join` and `ono.data.diff`
as `experimental` with `phase: planned`: spec §53 "declines to freeze" the syntax of `join` and
`diff`, and `tail` appears only in the examples of §33.2 and §41.4 (`| tail 30`). The library
layer has carried `Join` and `Diff` in `ono-pipeline` since Phase B, but no command bound them,
so the shell answered `Ono-Sendai-E0101 … implements nothing for it`. The RED suite
`crates/ono-cli/tests/data_missing.rs` states what a user expects of the three, and this ADR
fixes the surface they get. Spec §53 is respected in the one way it can be: the surface is
delivered as `experimental`, so it may still move, but it is delivered.

## Decision

### 1. `tail N [--follow]` counts values and follows what does not end

- `tail N` on a **bounded** stream emits its last `N` values, in order, whole — a record leaves
  as a record (spec §12.1), never as lines. It holds at most `N` values while the stream runs.
  A count larger than the stream emits the whole stream; nothing is fabricated (spec §35.3).
- `tail N` on an **unbounded** stream follows it: every value is among the last `N` the moment
  it arrives, so it is passed on at once, and a `take` downstream ends the pipeline. This is
  what keeps `tail` in spec §11.1's streaming set — a transform that waited for the end of
  `watch` would never answer.
- `--follow` names that behaviour explicitly. It changes nothing on a bounded stream (the end
  arrives, the trailing window is the answer) and is accepted there so a script written for a
  live source still runs on a snapshot.
- Argument mode is `words` (ADR-0009): the count is a word, as `tail 30` is written.
- Resolution follows ADR-0028 unchanged: `tail -n 1 file` at the head of a pipeline reaches no
  object stream and is `/usr/bin/tail`; `… | from json | tail 1` is `ono.data.tail`.

### 2. `join <right> --on <key> [--kind inner|left|right|outer]`

- `<right>` is an expression producing records: a `$variable` holding a list, or a
  parenthesised pipeline `(get socket)`. A list joins its items; a single record joins itself;
  `null` joins nothing.
- `--on` is a key expression evaluated against each record of *both* sides with the record's
  fields in scope (spec §10.3), so `--on pid` reads `pid` left and right. An unknown key
  matches nothing.
- `--kind` defaults to `inner`; `left`, `right` and `outer` keep the unmatched rows of the
  named side(s) with the other side `null` — the null of spec §35.3, not an empty record.
- Output rows are `ono.join/1` records — `key`, `left`, `right` — **not** a flat merge. Merging
  would force a rule for two fields of one name (`name` on a process and on a user) before a
  use case exists to judge it by, which is precisely the freezing §53 warns against. A user who
  wants a flat row writes `select left.pid right.name`.

### 3. `diff <right> [--identity [fields]]`

- The **input** is the current state and `<right>` is what it is compared against, the way
  data.yaml's example `get service | diff @-1` reads. `<right>` is spelled as for `join`.
- Identity comes from the record's schema (spec §28.1: `ono.process/1` is `pid + started`)
  unless `--identity [pid]` overrides it. A single identity field keys by that field's value;
  several key by the list of their values; a value without a schema and without an override is
  its own identity.
- Output rows are `ono.diff/1` records: `change` (`added` | `removed` | `changed`), `key`,
  `left` (the current value, `null` for a removal) and `right` (the previous value, `null` for
  an addition). A `changed` row therefore carries both values. An unchanged object is not a
  change and produces no row; identical snapshots produce an empty stream.
- Both transforms need their input to end (`InputRequirement::Bounded`); over an unbounded
  stream they are rejected with `stream.unbounded_operation` unless windowed, as §11.1 requires.

### 4. The right-hand side is evaluated by the shell, before the stage runs

`ono-command` evaluates expressions without running pipelines (ADR-0005). So the shell runs
every parenthesised pipeline found in a native stage's arguments **before** the segment starts,
captures its values, and hands them to the stage through the `Scope` keyed by the expression's
source span. The same `Scope` now carries the session's `$variables`, which native stages could
not see before. Capturing runs the sub-pipeline through the ordinary evaluator with its output
diverted from the sink — a native sub-pipeline's values are captured; an all-external
sub-pipeline's bytes are the language family's concern (`language_missing.rs`) and are not
claimed here.

### 5. Contract change

`ono.data.tail`, `ono.data.join` and `ono.data.diff` move from `phase: planned` to `phase: B`
in `docs/contracts/commands/data.yaml`. They stay `experimental` — spec §53's reservation is about
freezing the syntax, and stability is the promise that would freeze it (ADR-0012 §3).

## Consequences

- `crates/ono-cli/tests/data_missing.rs` encodes every rule above at the CLI boundary;
  `docker/acceptance/cases/036-data-tail-join-diff.case` proves the three in the container.
- A nested `join` output is one more level to `select` through; a later ADR may add a flat
  spelling once a use case demands one, without breaking `ono.join/1`.
- `Scope` grows a map of pre-run pipeline results; it is cloned per record by `reduce`, which
  is the one transform that rebuilds the scope, and stays cheap because the map is `Arc`-shared.

## Alternatives considered

- **Flat-merged join rows.** Rejected: needs a collision rule §53 says not to freeze yet.
- **`tail` as a bounded transform only.** Rejected: `get log --follow | tail 30` — the
  spec's own example — would hang or be refused.
- **Running the right-hand pipeline inside `ono-command`.** Rejected: ADR-0005 keeps the
  command layer free of the evaluator, and the evaluator already owns sub-pipeline execution.
