# ADR-0431: A failure proof carries its own source and its own population

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §0.5.5, §0.5.7, §25.1–§25.7, §32.1, §32.2, §33.2, §33.3, §35.1, §35.2, §57
  (phase H0), §58.2, §60.1, §61.3, Appendix E, Appendix F; AGENTS.md §7 (RED before GREEN, an
  ignored test needs a reason and a board entry), §11 (tests assert outcomes)
- Decided by: agent (autonomous)

## Context

v0.4.1 §57 opens phase H0 with a rule the rest of the tranche depends on: *"No production fix
lands before the corresponding failure proof where practical."* Issue #31 names four of them.
This ADR covers the two that belong to the evaluator and the spatial layer:

- **`each` does not stream** (§0.5.5, issues #75 and #76). §25.1 requires that
  `source | each { … } | downstream` begin the block for the first value before the source has
  completed; §25.2 forbids the complete-input `Vec<Value>` capture; §25.6 makes accepting an
  unbounded source the acceptance criterion. §58.2 writes the proof out: *"source emits one
  value, waits on a barrier, then would emit forever; `each { $it } | take 1` must complete
  before barrier release."*
- **The spatial first-output pathology** (§0.5.7, issues #85, #22, #20). §33.3: *"A supported
  interactive operation MUST NOT spend 30 seconds producing neither output nor progress on the
  reference Profile M/L fixtures."* §61.3 makes it a watchdog.

Both proofs had the same two problems to solve. The first is where the *input* comes from: a
proof about streaming needs a source that has produced a value and has not ended, and a proof
about cardinality needs a system of a stated size. The second is what the proof may *assert*.
Issue #21 and ADR-0252 are the standing record that wall-clock assertions on shared hardware are
this repository's most reliable source of flakes; ADR-0252's own comment says a 50 ms budget
"is flaky on shared hardware", and issue #20 exists partly because a liveness bound fails one run
in ten on a large host.

## Decision

**A failure proof brings the condition it needs with it, and asserts an outcome that is either
present or absent — never a duration that is merely large.**

### 1. The `each` proof brings a barrier source built from production code

`crates/ono-cli/tests/each_streaming.rs` uses `tail file <path> --lines 1 --follow` as §58.2's
source. It is the shell's own file provider, unmodified: it emits the one line the file already
holds, then waits for the file to grow, and it declares itself `Boundedness::Unbounded` because a
followed file never closes. **The barrier is the file not growing**, and the suite holds it shut
for the whole run — every test asserts afterwards that the file still holds exactly one line, so
"completes before the barrier releases" is written as a fact a test can read off the disk rather
than as a stopwatch reading.

Nothing in the proof measures time. A streaming `each` answers in milliseconds; the capturing one
does not answer at all. The 60 s budget in the file exists only so that a red run reports itself
instead of holding the suite, and a machine sixty seconds slower than this one is not a machine
that runs `cargo test`.

The suite carries a **differential**: the same source and the same `take 1`, with `where` in place
of `each`, answer at once and that test is green today. Appendix E classifies both stages as ones
that never require finite input, so the pair names the defect precisely — it is `each`, not the
source, not `take`, and not the shell's stream plumbing.

### 2. The spatial proof brings a real population at a stated cardinality

`ono_testkit::ProcessPopulation` puts the host at one of §32.2's reference profiles by spawning
that many idle `sleep` children and killing and reaping every one of them on drop. §32.2 permits
synthesis but fixes the limit — *"provider/planner code exercised by the benchmark MUST match
production logic"* — so the fixture creates **objects**, never data: the production process
provider reads them out of `/proc` exactly as it reads anything else, no provider is replaced and
no root is redirected. `PROFILE_S`, `PROFILE_M` and `PROFILE_L` carry §32.2's four numbers each
(processes, graph nodes, edges, sockets), so the profiles issue #82 has to deliver already have
one definition rather than one per test.

`PROFILE_L` is deliberately not built by any in-repository test. A `sleep` child costs about
1.35 MB of kernel memory on the reference machine, measured while writing this, so ten thousand of
them is roughly 13 GB — a fixture for the container, where
`docker/acceptance/fixtures/perf/many-processes.pl` already forks that population, not for
`cargo test`. The constant exists so phase H7's benchmark command (§37.1) inherits the number
rather than inventing it.

The fixture is verified by a test that is **green and un-ignored**:
`should_show_a_placed_population_to_the_process_provider_when_a_profile_fixture_is_built` counts
the population through `get process`, so a fixture that stopped being visible to production code
would fail on the next gate run rather than quietly weaken the proof beside it.

### 3. The spatial proof asserts §33.3's floor, not §33.2's target

`should_answer_or_refuse_the_live_map_within_the_interactive_watchdog_on_profile_m` runs issue
#22's own command — `map --live --json | take 3 | to json` — and asserts only that **something**
reached either stream inside thirty seconds. Output, progress metadata and a deterministic
cost refusal all satisfy it, which is exactly the set §33.3 allows, so the proof does not
prescribe which of §35.2's answers the fix chooses.

Thirty seconds is the specification's own number, not a threshold this test invented, and it sits
sixty times above §33.2's 500 ms Profile M target. A working implementation cannot approach it and
the current one never reaches it at all: the run produces zero bytes on both streams and is
killed. That is the widest margin available anywhere in this area, and it is why this shape was
chosen over the §33.2 targets, which are 50 ms and 500 ms and would be a coin toss on a shared
runner the day they went green.

### 4. What was measured and deliberately not encoded

Issue #20's instance — the map of `COMPUTE`, the place the population actually lives in — was
written, run and then removed. On the reference machine at Profile M,
`enter compute; map --live --json | take 3 | to json` answers in **29.7 s**: inside §33.3's budget
by three tenths of a second, and sixty times outside §33.2's Profile M target. A watchdog over it
would pass or fail on machine load, which is the defect ADR-0252 records rather than a proof of
anything. Issue #20's own exit test is a frame-budget assertion under a terminal, and phase H7
owes it one; the fixture this ADR adds is what will make it reproducible, because §34.2's frame
budget is only meaningful against a system of a stated size.

The same measurement pins the shape of the defect the root watchdog catches, and it is worth
recording because it is not what issue #22 assumed. `map --live --json | take 1` answers at the
root in 0.2 s. The blank thirty seconds come from the second value, which never arrives: the root
projection is domains and collections, `MapSnapshot` compares node and edge labels only, and a
picture made of names that cannot change is a picture that never reports a change. So the
pathology at the root is *unconditional* in this build rather than cardinality-driven — the
cardinality is what makes the surrounding measurement reproducible, and what phase H7 needs, but
it is not what makes this particular command silent. The fix owed to §35.1 and §35.2 is a bounded
initial projection that is truthful about pending detail, and that is what will turn the test
green.

### 5. Both suites are named for their subject

ADR-0426 settled that **a test suite is named for its subject, never for the state the product was
in when it was written**, and paid for the v0.4 `*_missing.rs` precedent by renaming twenty-three
suites and every pointer that resolved into them. `each_streaming.rs` and `spatial_first_output.rs`
therefore carry no `_missing`, `_red` or `_proof` suffix. The RED phase is recorded where ADR-0426
put it: in the commit, in the `#[ignore]` reason, in the module documentation, and here. **Nothing
is renamed when these go green** — the `#[ignore]` attribute and the *Deferred* entry are removed
by the increment that earns it, and the file keeps its name.

## Consequences

Easy: phase H6 and phase H7 each start from a test that already says what "fixed" means, in the
words of the section that requires it. The `each` proofs run in under a second, so the increment
that fixes #75 and #76 gets its answer immediately. `ono-testkit` now has the one thing it lacked
for performance work — a fixture that states a cardinality — and it is one definition rather than
the two `Children` helpers that `spatial_map.rs` and `spatial_interactive.rs` had grown
separately (ADR-0427's rule: those two are not byte-for-byte identical and are left alone; the new
one is not a merge of them).

Hard: the Profile M watchdog costs a thousand real processes and up to thirty seconds when it is
run, so it is an `--ignored` proof and not a gate test even after it goes green — phase H7 will
have to decide whether it belongs in the container instead, where §61.3 places the watchdog.
`ProcessPopulation` also depends on `sleep` being on `PATH`, which is the same dependency
`spatial_map.rs` already had.

Also hard, and honest: the root live-map proof does not need the population to be red. It is
filed with the fixture because §32.1 forbids proving a spatial operation performant against one
small fixture, and the reverse discipline is the same — a spatial failure recorded without a
stated cardinality is the failure issue #22 spent a release cycle being told was "the machine, not
the build".

Un-ignoring, in full:

| Test | Un-ignored by | What it then proves |
| --- | --- | --- |
| `each_streaming.rs::should_answer_take_one_before_the_source_closes_when_each_transforms_a_waiting_stream` | issue **#75** | §25.1, §25.2, §58.2: the block runs for the first value before the source completes |
| `each_streaming.rs::should_accept_an_unbounded_source_when_each_transforms_it` | issue **#76** | §25.6, §60.1: an unbounded source is accepted rather than refused with E0801 |
| `spatial_first_output.rs::should_answer_or_refuse_the_live_map_within_the_interactive_watchdog_on_profile_m` | issue **#22** | §33.3, §35.1, §61.3: no blank thirty seconds on a Profile M host |

Each removes its `#[ignore]` and its `// REASON:` comment in the commit that closes the issue
named beside it, and the matching *Deferred / blocked* entry leaves `docs/STATE.md` in the same
commit.

Encoded by: the three tests above, plus
`spatial_first_output.rs::should_show_a_placed_population_to_the_process_provider_when_a_profile_fixture_is_built`
and `each_streaming.rs::should_answer_take_one_before_the_source_closes_when_a_predicate_filters_a_waiting_stream`,
which are green today and fail if either fixture stops being what the proof beside it assumes.

## Alternatives considered

**A synthetic `/proc` tree instead of real processes.** `ProcessProvider::rooted` already accepts
a fixture root and `crates/ono-provider-linux/tests/common/mod.rs` already builds one, so a
Profile L tree could be written to disk for nothing. It cannot reach `map --live`: the live loop
and the projection live in `ono-cli`'s private modules, and there is no environment seam that
points the built binary at a fixture root. Adding one would be production surface, which a failure
proof may not introduce.

**Asserting §33.2's Profile M targets directly** — 150 ms for a spatial query, 500 ms for the
first live frame. They are the real product requirement and they are what phase H7 must meet, but
as the *proof* they would be a stopwatch on a shared runner, and the green side of the assertion
would be the flake. §33.3's floor is red by an unbounded margin today and green by a factor of
twenty after the fix.

**A test double for the `each` source** — a fake unbounded provider registered for the test. It
would prove that `each` streams a fixture, which is not the claim; §25.6's claim is about the
shell, and the file provider's `--follow` is a production unbounded source that already exists.

**Timing the first byte of `map --live` rather than watching for any byte.** The shell collects a
bounded pipeline before rendering it, so there is no first byte to time until the pipeline
finishes; a proof written that way would be measuring §65.7 ("streaming via background
collection"), which is phase H6's subject and has its own proof in `each_streaming.rs`.

**Keeping the `*_missing.rs` name of the v0.4 RED suites.** Rejected by ADR-0426, which is the
decision in force and which had to rename twenty-three files and every gate-resolved pointer into
them to undo that precedent. A name that has to be paid for twice is not a convention.
