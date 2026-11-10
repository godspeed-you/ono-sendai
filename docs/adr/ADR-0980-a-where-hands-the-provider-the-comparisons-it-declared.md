# ADR-0980: A where hands the provider the comparisons it declared

- Status: accepted
- Date: 2026-11-10
- Spec refs: §10.5, §35.3, §36.5, §41.4; v0.3 §1.37; ADR-0014, ADR-0085, ADR-0086, ADR-0222
- Decided by: agent (autonomous)

## Context

A `where` over a provider-backed target is decided by the pipeline, record by record, after the
provider produced each one. Streaming and `take` already stop early, so the cost of a selective
query is the distance to its first match: `get journal | where priority <= 3 | take 5` read and
decoded three days of journal on the developer machine before the first entry passed, 3.3 s in a
release build where `get log --level err | take 5` took 0.1 s (#281). The provider has a native
path for the same question — `--level` becomes `journalctl --priority` — and nothing handed the
predicate to it.

What was there before this decision, established by reading the pipeline:

- `Query` carries `selectors` and a `limit`. Both are filled from the *arguments of the producing
  command* (`get service nginx`, `--limit`). No stage after the producer ever contributed to the
  query; `where` received a stream and nothing else.
- A selector is binding. A provider that is given `Selector::Field` answers only the objects it
  names, and no later stage checks that it did.
- Provider declarations live in `docs/contracts/providers/*.yaml`; `spec-check` holds each one
  against the schemas, and the generated conformance suite holds each provider's code against
  its declaration.

The same shape recurs wherever a target is large or remote: container engines, Kubernetes, cloud
APIs, other log sources. For a remote source every record the provider could have excluded also
crosses the network. The decision is therefore about a mechanism, with the journal as its first
user.

## Decision

**A provider declares which comparisons its source decides with Ono's meaning. The `where`
directly after a producer offers the comparisons its predicate starts with, the producer hands
the provider the ones it declared, and the `where` decides every record the provider returns.**

### The declaration

A provider contract may carry a `pushdown:` block. One entry is one field of one target:

```yaml
pushdown:
  - target: journal
    field: priority
    type: int
    operators: ["==", "<", "<="]
```

- `target` is one of the provider's own targets;
- `field` is a field of that target's schema that is **required and not nullable**. A field whose
  value may be unknown is never declared: a comparison with `null` is undecided in a `where`
  (ADR-0014, spec §10.5), and a source that silently drops such records would take the note about
  them away;
- `type` is the field's type and one of `int`, `string`, `bool` — the scalars whose comparison
  has one meaning on both sides of a provider boundary. Units, timestamps, enums, floats and
  durations are compared by the pipeline alone until an ADR states their native meaning;
- `operators` is a non-empty subset of `==`, `!=`, `<`, `<=`, `>`, `>=`, each named once.

The declaration is a promise about meaning: for every record the provider could produce for the
target, a declared comparison decided by the source keeps exactly the records the same comparison
decided by the pipeline keeps, or more. It may keep more; it may never keep fewer.

In code the same statement is `Provider::pushdown() -> Vec<PushdownCapability>`, which answers
nothing by default. Two checks hold the pair together:

- `spec-check` reads every `pushdown:` block and refuses an entry about a foreign target, an
  unknown field, a field that is optional or nullable, a type that is not the field's or not in
  the subset, an operator outside the vocabulary, an empty or repeated operator list, a second
  entry for the same field, and an entry that leaves a key out;
- the generated provider conformance suite compares `Provider::pushdown()` with the block, in
  both directions. A provider that declares more in code than in its contract fails, and so does
  one whose contract describes a handover the code does not make.

### What a predicate offers (v1)

`ono_command::pushable_comparisons` reads a bound stage and answers the comparisons it offers.
It knows no provider, no target and no field name.

- Only `ono.data.where` offers anything, and only the stage **directly after** the producer is
  asked. A stage between the two sees what the provider produced, so anything withheld would
  change what that stage sees.
- The predicate is read as a conjunction, left to right, with grouping parentheses removed.
- A conjunct is a comparison when it is one field against one literal with `==`, `!=`, `<`, `<=`,
  `>` or `>=`. The literal is an integer, a boolean, or a string without interpolation. A literal
  on the left is mirrored (`3 >= x` is `x <= 3`), so a provider reads one shape.
- The reading **stops at the first conjunct that is anything else**: a disjunction, a negation,
  arithmetic, a variable, a call, a float, a unit literal, a nested path.

The leading run is the rule because `and` evaluates its left side first (ADR-0014). A conjunct
after the stopping point is only asked about records the earlier ones kept, and one of the earlier
ones may fail, or be undecided, on a record a later comparison would have withheld. Handing that
later comparison over would swallow the error or the note.

### What a provider is handed

The producer asks `ProviderRegistry::handed_over(target, offered)`: the leading run of the offered
comparisons for which the answering provider has a declaration with the same target, the same
field, a declared operator and a literal of the declared type. It stops at the first comparison
without one, for the reason above. The result travels in the query (`Query::pushing`,
`Query::pushed`), beside the selectors and apart from them: a selector must be honoured, a handed
comparison may be.

A provider that declares nothing is handed nothing, and its query is the one it received before.
A predicate outside every declaration yields the same.

### The `where` always decides

The predicate is never rewritten and the stage is never removed. Whatever the provider did with
the comparisons, the `where` evaluates the whole predicate over every record it is given. A
provider may therefore ignore a comparison or apply it only where that is sound, and correctness
never rests on its translation.

### Scope of v1

- The offer is made by the foreground evaluator. A pipeline assembled for the background does not
  offer comparisons and runs as before.
- Nothing is handed across a remote link; a remote provider answers the default, an empty
  declaration.
- A query at a past coordinate is answered from the ledger, which reads no handed comparison; the
  `where` decides the reconstructed records.
- The setting `providers.pushdown` (bool, default `true`; `ONO_PROVIDERS_PUSHDOWN=false`) withdraws
  the offer for every provider. It is the second arm of the equivalence tests and the way back to
  the earlier behaviour.
- `explain` names the handover on the producing stage — "hands the provider `systemd-journal` the
  comparison `priority <= 3` of the `where` after it, which the provider declared it can decide at
  its source; the `where` still decides every record (ADR-0980)" — by asking the same two
  functions the run asks. It starts no provider.

### The journal

`systemd-journal` declares `priority` (`int`; `==`, `<`, `<=`) for `journal` and `log`.
`journalctl --priority` takes a ceiling (`3`) or a range (`2..3`) over the syslog priority, which
answers those three operators over the integer both schemas require.

- `!=` has no spelling there. A floor alone (`>=`, `>`) would be a range up to 7 and is left to
  the pipeline in this first declaration (see the alternatives).
- `level` is not declared. It is the same number as a name, but it is nullable, and it is an enum
  ordered by its declared values, the reverse of the number (ADR-0222).
- The handed comparisons and `--level` are folded into **one** `--priority` argument, because
  `journalctl` honours the last one it reads: `--priority=HIGH` when the range starts at 0,
  `--priority=LOW..HIGH` otherwise. A range that keeps every priority, and a set of comparisons no
  priority satisfies, add no argument; the pipeline rejects those records itself.
- The provider ignores what it is handed when the query follows the journal, asks for its last
  lines, or carries a limit. `--lines` and `--follow` count what `journalctl` would print, so a
  `--priority` beside them selects other entries than a filter over the unfiltered tail keeps.

The translation lives in `crates/ono-provider-systemd/src/journal.rs`, in the function that
already maps the options.

### Another provider kind

A container engine or Kubernetes declares the same way, in its own contract and its own crate:

```yaml
# docs/contracts/providers/kubernetes.yaml (illustration)
pushdown:
  - target: pod
    field: namespace
    type: string
    operators: ["=="]
  - target: pod
    field: phase
    type: string
    operators: ["==", "!="]
```

`where namespace == "kube-system" and phase == "Running"` then reaches the API server as a
namespaced list request with the field selector `status.phase=Running`, and the records that
cross the network are the ones the `where` will keep. A container engine declares `state` (`==`)
and turns it into the engine's `status=` filter. Each of them declares only fields its schema
requires, and nothing in the evaluator changes.

## Consequences

- A selective `where` on a declared field costs what the matching records cost. Measured over a
  generated journal served by a `journalctl` shim, with three matches and the first at 90 % of the
  journal (`get journal | where priority <= 3 | select cursor | to json`, median of 5 rounds,
  debug build, AMD Ryzen 9 3900X, load average between 2 and 4.6):

  | entries | `providers.pushdown = false` | `providers.pushdown = true` | ratio |
  |---|---|---|---|
  | 5,000 (the test's default) | 565.4 ms, 5,000 entries read | 71.2 ms, 3 entries read | 7.9x |
  | 20,000 | 1,910.8 ms, 20,000 entries read | 81.2 ms, 3 entries read | 23.5x |
  | 200,000 | 18,827.9 ms, 200,000 entries read | 141.7 ms, 3 entries read | 132.9x |

  Reproduce with

  ```text
  ONO_PUSHDOWN_BENCHMARK_ENTRIES=200000 cargo test -p ono-cli --features systemd \
      --test predicate_pushdown_benchmark -- --nocapture
  ```

  The figures are a property of that machine and that build. None of them is a threshold: the
  test asserts that both runs answer the same and that `journalctl` hands Ono the whole journal
  without the handover and the three matching entries with it, and it prints the timings. The
  shim scans its file with `awk` either way, where journald answers `--priority` from an index,
  so the figures show what Ono saves on its side of the boundary. The release-build baseline of
  `xtask perf` (`docs/contracts/hardening/performance_baseline.json`) is taken on its named
  reference environment and is untouched; a scenario there is a follow-up for whoever runs that
  environment.
- **The equivalence has one stated bound.** A source that excludes records also excludes what
  reading them would have reported. For the journal: an entry that carries no `PRIORITY`, or one
  outside the canonical digits, violates `ono.journal-event/1` and is reported as
  `Ono-Sendai-E0908` when it is read. journald's `--priority` match returns no such entry, so
  with a handed ceiling the entry is never read and the report is not made; the records the
  `where` keeps are the same. `providers.pushdown = false` restores the report.
  `should_not_see_an_entry_without_a_canonical_priority_when_the_ceiling_is_handed_over` pins the
  bound so it cannot widen unnoticed. Every future declaration carries the same bound for decode
  and schema failures on records its source withholds, and states it where it declares.
- Everything else is identical with the handover and without it: records, order, count, the
  note about undecided values, errors raised by later conjuncts, exit status, and how `take`,
  `count` and `sort` after the `where` see the stream.
- Predicates the first version leaves to the pipeline alone: a comparison after a conjunct that
  is not one, `or`, `not`, a second `where`, a `where` behind another stage, floats, units,
  timestamps, enums and nullable fields. Each can be added by widening the subset in one place
  with its own equivalence cases.
- A new provider gains the handover by declaring it in its contract and implementing
  `Provider::pushdown` and the translation in its crate; `spec-check` and the conformance suite
  tell it when the two disagree.
- Tests that encode the decision:
  - `crates/ono-command/tests/pushdown.rs` — what a predicate offers and what a declaring, a
    silent and a differently-declaring provider is handed, through the public command API with a
    test provider;
  - `crates/ono-cli/tests/predicate_pushdown.rs` — the real binary against a `journalctl` shim
    that records its arguments: supported predicates, unsupported ones, mixed conjunctions,
    `--level` folded with a `where`, thirty pipelines answered identically with the handover on
    and off, the `explain` note, and the stated bound;
  - `crates/ono-cli/tests/predicate_pushdown_benchmark.rs` — the benchmark above;
  - `xtask/tests/contracts.rs` (`…pushdown…`) and the generated
    `crates/ono-cli/tests/provider_conformance.rs` — the declaration held to the schemas and the
    code held to the declaration;
  - `docker/acceptance/cases/395-a-where-on-priority-reaches-journalctl.case` — the same through
    the container.

## Alternatives considered

- **A general query optimizer** — rewrite rules that move predicates across `sort`, `take`,
  `select` and joins, with a cost model that chooses among plans. Rejected for this version: the
  problem at hand is one stage beside one producer, and the leading-run rule solves it with a
  proof of equivalence a reader can check in a paragraph. An optimizer has to prove the same
  about every rule and every pair of stages, including their errors and notes (ADR-0014), and
  nothing measured asks for it yet. The declaration format and `Query::pushed` are what such an
  optimizer would feed, so the door stays open.
- **Provider-specific special cases** — the evaluator recognising `get journal | where priority`
  and adding `--priority`, or the journal provider being passed the syntax tree of the following
  stage. Rejected: every further source would add a branch to the evaluator, the knowledge about
  `journalctl` would live in two crates, and no contract could say what is handed over. The
  generic layer names no provider and no field.
- **Lowering into the existing `Selector`** (`Selector::Field` for `==`, a new variant for the
  ordered operators). Rejected: a selector is binding and comes from the command's own arguments;
  a provider answers exactly the objects it names, and no later stage checks that it did. A
  handed comparison is advisory, and a provider written before this ADR has to be able to ignore
  it by doing nothing. `Query` is extended with a separate list instead.
- **Inferring the capability** from the option mapping (`--level` exists, so `priority` probably
  maps) or from the schema. Rejected: an inference cannot be held to anything, and the `level`
  field shows how a plausible mapping is wrong twice (nullable, reversed order).
- **Dropping the `where` when its whole predicate was handed over.** Rejected: correctness would
  rest on each provider's translation, and a provider could no longer apply a comparison
  partially or skip it for `--lines`.
- **Handing over every declared conjunct, wherever it stands.** Rejected: in `where a / b > 1 and
  priority <= 3` the first conjunct fails on records the second would withhold, and the errors
  are part of the answer.
- **Handing over through intermediate stages** (`get journal | take 100 | where …`, a second
  `where`). Rejected for v1: each stage needs its own argument for why the withheld records
  cannot change what it sees; `take` and `sort` have none.
- **Declaring `level`, or `>=` on `priority` as a range up to 7.** `level` is nullable and ordered
  the other way round (ADR-0222). `priority >= 5` as `--priority=5..7` is expressible and sound,
  and was left out to keep the first declaration to the operators the issue measured; adding it
  is a contract line, a match arm and its equivalence cases.
- **No off switch.** Rejected: the equivalence is tested by running both arms through the real
  binary, and a user who meets the stated bound needs the earlier behaviour without a rebuild.
