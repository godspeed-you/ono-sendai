# ADR-0747: A section 49 measurement is one process per sample, through the shipped library

- Status: accepted
- Date: 2026-09-09
- Spec refs: v0.5 §32.3, §49, §39.2; v0.4.1 §32.2, §32.3, §37.3, §37.4, Appendix F.4;
  ADR-0489, ADR-0498
- Decided by: agent (autonomous)

## Context

`perf::BENCHMARKS` measures the product the way a user meets it: the runner spawns the real `ono`
binary with a script and times the bytes coming back. That is the right shape for everything
v0.4.1 budgets, and it is not available for seven of §49's eight rows, because the commands that
would run them — `timeline`, `changes`, `at`, `why`, `find event` — are bound to the session by a
work package that is not in the tree.

Waiting for the binding would leave §49 unmeasured, which is the one outcome this package exists to
prevent. Measuring through a harness of my own would break §32.2's rule that the code exercised by
a benchmark must be production logic. So the question is how to measure a library operation without
either.

`perf::Runner::run_completion` already answered it once, for §36.2's completion budget: re-run the
`xtask` executable with `--sample-completion`, take one figure per process, and aggregate in the
parent. Its reason was that the completer caches for five seconds, so the second call in a process
is a different measurement (§37.3). A temporal query has the same property and more of it — SQLite
keeps its page cache, the operating system keeps the file, the planner keeps its prepared statement.

## Decision

**`perf::TEMPORAL_BENCHMARKS`, a second table beside `BENCHMARKS`, measured one process per
sample through `xtask perf --sample-temporal <operation> --sample-index <n>`.**

Both tables produce the same `Measurement`, carrying §32.3's six metrics in Appendix F.4's shape
and naming §37.2's reference environment, so a reader of the baseline cannot tell — and does not
need to — which table a row came from.

Three rules make the figures mean what they say.

1. **One process per sample.** Twenty iterations inside one process would be one cold figure and
   nineteen cache hits, and §37.3 forbids advertising one as the other. Twenty processes are twenty
   samples, which is also what §37.4's "at least 20 iterations … median and p95" is asking for.
2. **Each sample asks a different question.** The window walks back an hour per sample and the
   place rotates, so a repeat is a new question rather than the same one answered from a cache.
3. **Every operation is the shipped call.** `timeline::timeline`, `changes::changes`,
   `Reconstructor::reconstruct`, `CausalEngine::explain`, `search::find_events`,
   `Recorder::maintenance` and `LedgerStore::sweep`, over a real store opened through
   `StoreOptions`. Nothing is reimplemented and no query is hand-rolled; when the command layer
   lands, it will call exactly these functions.

The temperature is `warm` for the query rows and it is the honest word: §37.3's warm is *"a process
that is already running and whose providers are initialised, answering a query nothing has answered
before"*, and the store is the provider. `temporal.startup_disabled` is `cold`, because process
start is the thing it measures.

`cancel_ms` is `null` for every row. §32.6's cancellation contract is about the command that wraps
the call, and a single library call inside one process is not interruptible from outside it. v0.4.1
§2.6 keeps an unknown unknown; a zero here would be a figure nobody measured.

## Consequences

§49's measurements exist now, against §49's fixture, and they will keep meaning the same thing when
the commands land — because what they time is what the commands will call. A row whose subject is
not in the tree declares why and contributes no record, which leaves its target `Unmeasured`
(ADR-0749).

The cost is that these figures exclude the shell around the call: parsing, dispatch, rendering and
the pipeline. A `timeline` command will be slower than `temporal.timeline_15m` by whatever that
shell costs, and `shell.cold_start` says what that is to within a millisecond. When the command
exists, the honest thing is a second row measured through the binary, beside this one rather than
instead of it: the difference between the two is the shell's own overhead, which is a number worth
having.

## Alternatives considered

- **Wait for the command binding.** Rejected: it would leave §49 unmeasured at the moment the
  architecture is most worth measuring, which is exactly when a scaling failure is cheap to fix.
- **Iterate inside one process.** Rejected under §37.3: nineteen of twenty samples would be cache
  hits reported as one number.
- **A `criterion`-style harness.** Rejected: a second measurement vocabulary beside Appendix F.4's,
  a second baseline format, and a dependency, in exchange for statistics §37.4 does not ask for.
