# ADR-0507: The evaluator is eight modules, and a native run is four phases

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §22.2, §23, §25.3, §26.1, §26.2, §30.1–§30.4, §52.2, §55.1, §56.8, §65.7,
  §65.12, §66.6, Appendix I.2; spec §12.3, §16.5, §18.5;
  ADR-0013 (the execution model), ADR-0453/ADR-0457 (what a capture is charged to),
  ADR-0479 (the capture inventory), ADR-0480 (the block bridge), ADR-0481 (`stream_segment`)
- Decided by: agent (autonomous)

## Context

§30.1: "`ono-cli` may remain the composition root, but evaluator orchestration MUST no longer
concentrate statements, expressions, pipelines, functions, blocks and native execution in one
large module." It did: `eval.rs` was 2279 lines and `native.rs` 2537, and §30.2 names the eight
responsibilities that have to become separate.

ADR-0480 left a specific piece of this work behind, in its own words: *"`run_native_segment` grew.
It now assembles, drives and drains, and it was already the longest function in the crate. Phase
H9 (#96) decomposes the evaluator, and this function is the obvious seam."* It also duplicated its
binding and its assembly with `stream_segment`, which ADR-0481 had added beside it.

Two constraints shape the answer. §30.3: the explicit `Flow` for normal status, `break`,
`continue`, `return` and `exit` "MUST be preserved or strengthened", never replaced by "magic
error strings, panics or implicit flags". §30.4: no domain logic moves from a lower-level crate
into `ono-cli` to reduce a file's size. And over both, §65.12: no semantic redesign travels with
this.

## Decision

### 1. `eval.rs` becomes `eval/`, with §30.2's eight names

```text
crates/ono-cli/src/eval/
    mod.rs          Flow, Eval, run_program, status_for — and the façade the crate still calls
    statement.rs    one statement: prefix assignments, alias expansion, `kill %N`
    expression.rs   values, operators, the three-valued logic of spec §10.5
    control.rs      if / while / for / match / try
    block.rs        run_block, and the one-item scope a block stage runs in
    function.rs     resolving a call, binding parameters, running a body
    pipeline.rs     which stages are native, which are programs, and how segments meet
    materialize.rs  budget-aware finite collection
    native/         native execution, below
```

`mod.rs` re-exports every item that was `pub` before, so `crate::eval::stage_arguments`,
`crate::eval::eval_expr` and the rest resolve exactly as they did and no caller outside the
evaluator was touched by the split. §31.2 says the same thing about `Session` and this module
follows it: a public façade over an internal division.

### 2. `native.rs` becomes `eval/native/`, and the seam is four modules

§30.2 draws `native.rs` inside `eval/`, so it moved there — `crate::native` is now
`crate::eval::native`, which is the only call-site change outside the evaluator, and it is a
`use` line in nine files.

```text
crates/ono-cli/src/eval/native/
    mod.rs          the ways a native pipeline is entered: run, run_piped, run_collecting,
                    run_seeded, run_seeded_from, check, Start, Seed, run_from
    segment.rs      which stages are this module's, and what a contract admits and produces
    bind.rs         binding a stage, its scope, and §26.2's streaming continuation
    foreground.rs   run_native_segment: bind, assemble, drive, deliver, in that order
    drive.rs        the block bridge of ADR-0480, the driver loop, the background driver
    result.rs       what a drained segment becomes: reported, counted, written
    external.rs     an adapted external program as a stage of the object pipeline
    remote.rs       a stage that runs on a remote agent
```

`run_native_segment` was 459 lines and is 200. The four phases ADR-0480 named are now four calls:

- **bind** — `bind::bind_stage` is the one definition of "bound": contract, globs, resolution,
  arguments. `stream_segment` and `run_native_segment` had written that loop twice with one
  material difference each (`stream_segment` declines where the other refuses; the drained run
  additionally hands `format table` the session's row limit). The helper answers `Option`, so the
  caller keeps its own difference and the shared part is shared. This is the duplication ADR-0480
  reported.
- **assemble** — stays inside `foreground.rs`, because it is an `async` block over the runtime
  handle and thirteen borrowed pieces; extracting it would have produced a struct whose only
  purpose is to be destructured one line later. §30.2 asks for navigable responsibilities, not for
  every block to be a function.
- **drive** — `drive::drive_segment`, returning a `Drained { values, failures, stopped }`. The
  loop is unchanged line for line; what changed is that it fills a value it returns rather than
  three locals of its caller.
- **deliver** — `result::report_failures` and `result::deliver_segment`. The one reordering is
  that `unanswered` — a pure `any()` over the failures — is computed before the failures are
  reported rather than after, because reporting consumes them. It cannot observe anything else.

### 3. `materialize` owns the capture helpers, and `limits` keeps the numbers

§30.2: the module "SHOULD own budget-aware finite collection helpers so no caller recreates them
ad hoc". `captured_value`, `value_of_pipeline`, `binding_value`, `bare_value`, `captured_text` and
`capture_pipeline` — the last moved out of `native.rs` — are all there, so the evaluator has one
door to "run this pipeline for its values" and `native::bind::stage_scope` walks through it.

`materialize::limits` is the evaluator's one reading of §22.2's materialization budget, and the
four call sites in `native/` go through it. The **numbers** stay in `crate::limits`, which derives
every one of them from the settings catalogue: §52.2 forbids a limit being "independently typed
into five files if one contract can generate the others", and that outranks file locality. Moving
the catalogue reading into the evaluator would have been the inversion §30.4 warns about, one
crate lower.

`run_each_item`, `run_function_body` and `run_collecting` keep opening their own capture scopes.
They are not collection helpers; they are evaluator paths whose scope is a semantic requirement,
each classified in its own right in `docs/contracts/hardening/streaming.yaml`.

### 4. `Flow` is untouched

Six variants, one `From<ErrorValue>`, one `Eval<T>` alias, in `eval/mod.rs`, byte for byte what
they were. `control.rs` exists precisely so the constructs that read `Flow` sit together. No
sentinel string, no panic and no boolean was introduced anywhere in this change (§30.3).

### 5. The capture inventory and its scan follow the code

`docs/contracts/hardening/streaming.yaml` keys every entry on a file and an enclosing item, so a split
moves twenty of the twenty-one entries. Each keeps its class and its reason; only `file:` changed,
except for the one entry the seam divided:

- `run_native_segment` → `Drained` (`native/drive.rs`) and `deliver_segment` (`native/result.rs`).
  One collection, drained in the first and handed on by the second, both
  `semantic_materialization` for the reason the single entry gave. Nothing was reclassified: a
  class that genuinely changed would be a behaviour change, and one does not belong here.

`xtask::scan::check_evaluator_captures` reads a hard-coded list of evaluator sources, and a scan
keyed on a path that no longer exists does not go red — it goes **quiet**, which is worse. The
list was extended to name every file of `eval/` and `eval/native/`. Only that constant changed;
no scanning logic did. `crates/ono-cli/src/eval.rs` and `crates/ono-cli/src/native.rs` stay in it
because `xtask/tests/scan.rs`'s fixtures are written against those paths, and a fixture test
states the rule rather than the tree — it may not be edited in a refactor (AGENTS.md §11). A path
this repository does not have costs one failed `read_to_string`.

## Consequences

Easy: the longest evaluator file is now `eval/pipeline.rs` at 1075 lines, down from 2537, and the
median is under 300. A change to control flow is a change to `control.rs`; a change to what a
segment answers is a change to `result.rs`; the block bridge is one file with the driver that
answers it.

Hard, or newly visible:

- **`eval/pipeline.rs` is still large,** because `run_stage_list` is 444 lines of segment
  dispatch — a decision tree over remote, adapted, seeded, backgrounded and native shapes.
  Splitting *it* is a redesign of how a pipeline chooses its runner, and §65.12 keeps that out of
  this work package. It is reported rather than done.
- **`pub(super)` is a wider door than `fn`,** the same price the parser paid in ADR-0506.
- **`ono_cli::native` is now `ono_cli::eval::native`.** No test referenced it; nine source files
  did.
- **`xtask/src/scan.rs` carries two paths that no longer exist,** kept alive by its own fixtures.
  Removing them is a `test:` increment on the fixtures first, which this phase may not make.

Encoded by: the whole `ono-cli` suite unchanged and green, and specifically Appendix I.2's three
families — control flow and errors (`crates/ono-cli/tests/language.rs`,
`::builtins.rs`), pipeline cancellation and backpressure
(`crates/ono-cli/tests/streaming.rs`, `::each_streaming.rs`, `::resource_limits.rs`), and native
process and job behaviour (`crates/ono-cli/tests/jobs_native.rs`, `::native.rs`, `::signals.rs`,
`::external.rs`). Not one test file was edited.

## Alternatives considered

**Leave `native` a sibling of `eval`.** It would have avoided nine `use` lines. Rejected: §30.2
draws `native.rs` inside `eval/`, and a module that only the evaluator calls is the evaluator's.

**Extract assembly into a struct with a `run` method,** for symmetry with bind, drive and
deliver. Rejected: the struct would carry thirteen borrowed fields, exist for one call, and be
destructured immediately. §4's "no abstraction for a second use case that does not exist" applies;
the four phases are navigable without it.

**Unify `stream_segment` and `run_native_segment` completely.** They differ in what a missing
contract means, in whether a seed exists, in whether a stage may run a block, and in whether the
result is drained. A shared body would need four flags, and a flag-driven merge of two functions
whose tests cannot tell them apart is exactly the shape §65.12 exists to prevent. The binding is
shared; the rest is not.

**Move `crate::limits::materialization` into `eval/materialize.rs`,** which is one reading of
issue #96's "the module the budget-aware materialization helper lands in". Rejected on §52.2: the
helper reads the settings catalogue, which is where every limit in the shell is declared, and a
second reader of it in the evaluator is the duplication that section forbids. `materialize::limits`
is the evaluator's door to it instead.
