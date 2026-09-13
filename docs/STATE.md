# STATE

The shared work board. **Read it first, update it last, every session** (AGENTS.md section 9).

**The backlog is not here — it is the GitHub issue tracker** (ADR-0425). One problem is one
issue, and its evidence lives in the issue body: `gh issue list`. This file holds what a tracker
cannot — the claims in flight, the problems found and not yet filed, the accepted risks, and what
is deferred and why. The history it used to carry is one command away (*History*, at the end).
The stopping rule lives in `docs/ACCEPTANCE.md`: the run ends when `scripts/release-check.sh`
passes, not when this file looks tidy.

Working branch: **`implementation`** — never write to `main` unless the user asks for it in
that request (AGENTS.md section 12.1)

**Commit every increment, and tag every completed phase.** A phase is done when its box in
`docs/ACCEPTANCE.md` section 4.1 is ticked; the commit that ticks it gets an annotated tag
`phase-<letter>` whose message names the exit criterion and the case that proves it. The tags are
how the state after each phase stays findable in a run of hundreds of commits:

```bash
git tag -n99 phase-a          # what Phase A delivered, and what proves it
git switch --detach phase-a   # the tree exactly as that phase left it
```

Tags so far: `phase-a` … `phase-j` (H, I and J share `63a23ece`); the releases after them are tagged
by version, `v0.2.0` … `v0.6.1`.

**Push after every commit.** AGENTS.md §12.1 leaves `main` alone until the user asks for it, and
§12.2 asks that `implementation` be pushed freely so work is not lost; the branch and its phase
tags live on `origin`. Push `main` or open a pull request only when the user asks for it in that
request.

```bash
git push origin implementation && git push origin --tags
```

**The workspace declares `0.6.1`.** `v0.6.1`, the Stabilization and Polish patch over `v0.6.0`,
is tagged, published (2026-09-12) and on `main`. Its note is `docs/releases/v0.6.1.md` and its run
record `docs/runs/v0.6.1-2026-09-12.md`; every earlier release has its note beside it in
`docs/releases/`, and `gh release list` shows what is published.

---

## The specification set

`docs/specs/ono_sendai_shell_spec_v0.2.md` is the **base**. `docs/specs/ono_sendai_shell_spec_v0.3_external_command_adapters.md`
is an **enhancement layered on it** — the External Command Adaptation Layer — and both are
immutable (AGENTS.md §5.2, ADR-0026). `spec-check` fails if either is missing a checksum line in
`docs/specs/spec.sha256` or if `AGENTS.md` does not enumerate an enhancement by name.

**Build order for what remains: v0.7, then v0.8, then v0.9.** v0.4.1, v0.5, v0.6 and v0.6.1 are
implemented and released (`v0.4.1` … `v0.6.1`). Each remaining tranche's own §0.1 progression
diagram names the one before it as its prerequisite, so for these three arrival order and build
order coincide.

**v0.7 arrived on `main` on 2026-09-01** as
`docs/specs/ono_sendai_shell_spec_v0.7_presentation_consolidation_rich_tty.md` — Presentation
Consolidation & Rich TTY Interface, 2 616 lines. It is a consolidation release, not a new
surface: one deterministic policy for resolving the existing v0.2 render hints, presentation
profiles and constrained view tree into a production-quality rich terminal path, with
`HistoryEntry`/`ResultRef` and the v0.4–v0.6 context surfaced consistently near the prompt. Its
own §0.5 states it exists so a later Deck workspace has something solid to compose, and must
stay valuable even if that workspace is never built. Merged into `implementation`, checksummed
(`docs/specs/spec.sha256`) and enumerated (AGENTS.md §5/§5.2), **not implemented**, and **the next
tranche**. It has no `docs/ACCEPTANCE.md` checklist yet; writing one is its first task, and it
claims §4.13 (§4.12 is v0.6's).

**v0.8 arrived on `main` the same day** as
`docs/specs/ono_sendai_shell_spec_v0.8_deck_workspace_composition.md` — Deck Workspace Composition &
Terminal Ownership, 2 897 lines. It adds a persistent Deck host that composes the views,
history, context and safety state v0.2–v0.7 already define around the shell editor, plus one
generic terminal-ownership contract shared by the Deck, existing full-screen Ono views and
foreground external programs; its own §0.1 places it directly on v0.7, and §0.6 bounds it
explicitly against becoming a second type system, history store, context model or window
manager. Merged, checksummed and enumerated, **not implemented**, and **behind v0.7**.

**v0.9 arrived on `main` the same day** as
`docs/specs/ono_sendai_shell_spec_v0.9_live_view_integration.md` — Live View Integration &
Long-Running Workspace Ergonomics, 3 538 lines. Its own §0.2 explicitly rejects an earlier,
abandoned design direction for v0.9 (`Live<T>`, `StateObservation<T>`, a new watermark/
backpressure model) in favour of small, bounded, presentation-local bindings that keep
`Stream<T>`, `watch`, spatial `--live` and the v0.5 temporal cursor usable, honest and
responsive inside the v0.8 Deck over minutes or hours, without a second live-data model. Merged,
checksummed and enumerated, **not implemented**, and **behind v0.8** — the last tranche in the
current build order.

---

## Product direction from the user (2026-08-26)

**"Es muss immer cool sein und Spaß machen, es zu benutzen. Es soll aufregend sein."** The shell
is the Ono-Sendai deck: correctness is the floor, not the ceiling. Where a decision is
otherwise free, prefer the option that feels alive — the prompt as a HUD, tables that update in
place, colour that means something, latency you never notice (spec §34's budgets are product
quality), and answers that invite the next question (`@2 | inspect`). Phase F's `watch` is the
showcase: a live view of the machine should feel like instrumentation, not like polling.

## In progress

## What is left, and why

**v0.7 is the next tranche** (above), and nothing of it is started. Open in the tracker: #124–#127,
the milestone *Binary size — measured, budgeted, reduced*, and #129; v0.6.1 excluded all five by
name (its §22, §23). Promoting `implementation` to `main` is the user's decision; an agent carries
it out only when told to, in that request (AGENTS.md §12.1).

---

## Found, not yet filed

Problems found while working, before they are issues. **The backlog is the GitHub issue tracker**
(ADR-0425): one problem is one issue, and its evidence — reproduction, files, measurements, ADRs,
exit test — lives in the issue body. This section is the staging area in front of it. A defect you
run into while doing something else goes here, because AGENTS.md §4 forbids fixing it in the
commit that found it, and it goes here with the same evidence an issue would need, so that filing
it is a copy rather than a fresh investigation.

**Nothing here is work anybody may pick up**, and no agent opens the issue itself: the user
triages this section, and filing an entry removes it from the board. A problem is on exactly one
of the two surfaces, never both.

```bash
gh issue list --limit 100        # the backlog
gh issue view <NN>               # the evidence for one problem
gh issue list --label class-c    # the large ones, a tranche each
```

- **Two expression evaluators implement the same binary operators side by side (2026-09-12, found
  while typing the operators for completion, ADR-0860).** `crates/ono-command/src/expr.rs`
  `binary_op` (the pipeline filter's evaluator) and `crates/ono-cli/src/eval/expression.rs`
  `eval_binary` (the session evaluator) carry the same match over `BinaryOp` and their own copies
  of `equals`, `contains`, `regex_matches`, `remainder`, `kleene_and` and `kleene_or`. They agree
  today; nothing makes them. `tests/operator_typing.rs` proves the completion table against the
  first only. One evaluator behind both — the session one supplying variables and pipelines
  through `Scope` — closes it; a `refactor` with no test changing.
- **`ss`'s own stderr reaches the terminal unfiltered (2026-09-12, side note of #131).** Bare `ss`
  on a kernel that refuses one netlink family prints `RTNETLINK answers: Invalid argument` above
  the decoded table; the adapter passes the program's stderr through untouched. Whether an
  adapter may hold back a diagnostic its program prints for a family it did not ask about is an
  adapter-contract decision (spec v0.3 §1.59), not a decoding fix — which is why #131 left it.
- **The acceptance image job downloads the filesystem stage's packages from the Ubuntu archive on
  every CI run, and that download is what makes the job take 9 to 18 minutes (2026-09-11).** Two
  diagnostic runs on `implementation-image-timing` (34581843204, 34584197815) timed the stages:
  `builder` 5m11 with the cargo cache working — the 53 workspace crates compile, no dependency
  does — `runtime` 12 s (59 packages from `deb.debian.org` in 7 s), and `runtime-filesystems`
  3m38, 212 s of it `apt-get install btrfs-progs zfsutils-linux` against `archive.ubuntu.com`: 27 s
  to the first response, about three minutes for 50 packages. In 34581843204
  `scripts/acceptance.sh --build-only` took 17m32 with the builder stage already in the layer
  cache, which leaves the Ubuntu stage as the time. The Actions caches carry cargo's registry and
  target only, so the layer is rebuilt on every run. Since 08e89270 the download runs beside the
  Rust build (see *Done*) and costs the job only what it takes beyond the builder; it still
  happens on every run, and its length still varies with the archive. A buildx layer cache in
  GitHub's cache backend would remove it — judged on 2026-09-11 not worth a workflow rebuild and a
  share of the 10 GB cache budget while nobody waits on CI.
- **Planning a restart costs about 0.75 s per target, because every target reads every unit and
  every process again (2026-09-11).** On an idle machine `get service | take 50 | plan restart
  service` takes 38.5 s, `take 5` 3.5 s, and `get service | take 50` alone 0.2 s. `strace` counts
  1,831 `openat`, 1,031 `readlink` and 880 `sendmsg` to the system bus for one target and 8,782,
  5,162 and 2,468 for five — linear per target, about one full pass over `/proc` and the unit list
  each. It is also the suite's largest single cost: seven tests of
  `crates/ono-cli/tests/change_gates.rs` plan fifty targets each (94–99 s, in parallel) and one of
  `change_views.rs` does too (40 s), about 2.3 of a 10-minute local gate (ADR-0853). Reading the
  units and the process table once per plan closes it — a `perf` increment with a benchmark over
  the fifty-target plan.
- **Two tests of `change_claims.rs` take 31 s each, the length of the default verification
  timeout (2026-09-11).** `should_refuse_a_second_apply_naming_the_session_that_holds_the_plan`
  and `should_resume_at_once_a_plan_whose_applier_was_killed_mid_action` block a copy on a FIFO
  and then release it; both finish 31 s after their binary starts, and
  `crates/ono-change-core/src/verification.rs:25` sets `DEFAULT_TIMEOUT` to 30 s. Not confirmed:
  the likely cause is a verification reading the FIFO source after its writer is gone, until the
  timeout ends it. A timed run of one of them confirms or refutes it, and then says whether the
  product or the fixture is wrong.
- **`timeline::should_carry_a_rendered_reference_into_inspect_at_and_why` failed once in CI
  (2026-09-11).** In the first attempt of CI run 34562940989 (`implementation` 0eb0ce34),
  `at event @efa3` answered `temporal.invalid_time … no event with that reference is retained`,
  although `inspect event @efa3`, one shell earlier, had resolved the same reference; the re-run
  of that commit was green. Locally it never failed: 40 isolated runs, five runs of the whole
  binary, runs with `TZ=UTC`, `LANG=C.UTF-8` and the CI variables, 20 under full load on all
  cores, and 600 `at event` calls against a recorded home, 300 of them under full load. Three
  places turn a failure into exactly that answer without saying so: `configure_from` in
  `crates/ono-cli/src/temporal/mod.rs` skips the whole temporal setup, recorder included, when
  `session_state().try_lock()` does not succeed at once, and it discards `start_recorder`'s result
  (`Ok(_) | Err(_) => {}`), so a shell whose store did not open reads the empty session ledger;
  and `LedgerAnchors::instant_of` in `crates/ono-cli/src/temporal/coordinate.rs` drops the
  ledger's error with `.ok()`, so an ambiguous prefix or an unavailable store reads as "not
  retained". What closes it: the three report what happened — the recorder's health through
  `get recorder`, the ledger's error through `at` — and the next failure's cause is read off the
  CI log.

- **`ono_testkit::scratch()` falls back to `/tmp` outside a test binary's own crate.** `Scratch`
  reads `CARGO_TARGET_TMPDIR`, which cargo exports at compile time only, so a helper compiled into
  `ono-testkit` and called from another crate's suite lands in `/tmp`. On this machine `/tmp` is a
  quota'd tmpfs, and a tmpfs is precisely what v0.6 §15's file recovery provider refuses to treat
  as a persistence domain — so a suite that scratches there tests the refusal rather than the
  feature. `crates/ono-recovery-files/tests/` works around it by building its scratch from
  `env!("CARGO_TARGET_TMPDIR")` in its own fixture. What closes it: `scratch()` taking the
  directory from the caller through a macro, so the constant is expanded where the test is.

- **The session registers the file recovery provider only when its store opens (2026-09-10).**
  `crates/ono-cli` builds the protection registry with `ono.recovery.file-copy` only if the
  recovery store could be opened, so a session whose store is unusable has no file provider at all
  and the matrix says "no registered provider offers protection" rather than naming why. Nothing
  unsafe follows — a plan sealed as protected still refuses at `apply` (ADR-0842) — but the reason
  a plan is unprotected is lost. Reproduction: make `~/.local/state/ono/recovery` unwritable, then
  `plan copy file a b --overwrite | to json` — no refusal names the store. What closes it: register
  the provider as unavailable with the store's error, so `analysis.refusals` carries it into the
  matrix.
- **A second release run compares a fresh package against the previous release's manifest
  (2026-09-09, found running `release-check.sh` for v0.5.0).** `scripts/package.sh` writes into a
  `dist/` it does not clear, so the v0.4.1 `.deb`, `.rpm` and `SHA256SUMS` of 2026-09-05 were still
  lying beside the v0.5.0 pair it had just built. `scripts/package-check.sh` then compared the new
  digests against the old manifest and refused: `dist/SHA256SUMS does not match the packages that
  were validated (spec §48.2)`. The guard is right — the manifest genuinely did not describe those
  bytes — and the situation is one only a repeat run on one machine can produce, which is why it
  never appeared in CI. Reproduction: run `scripts/release-check.sh` twice at different workspace
  versions without emptying `dist/` in between. What closes it: `scripts/package.sh` builds into a
  `dist/` holding one build — either by clearing it, or by refusing to write beside artefacts of a
  version it is not building.

- **The RPM is not byte-reproducible across two clean builds; the `.deb` is (2026-09-09).**
  `xtask/tests/packaging.rs::should_produce_identical_hashes_for_two_clean_builds_of_one_commit`
  builds the same commit twice in environments that disagree about locale, timezone and umask
  (v0.4.1 §46.5). The Debian package came out byte-identical at 14,706,872 bytes both times; the
  RPM differed by eighteen bytes — 15,229,350 against 15,229,332. The umask was the first
  suspect and does not explain it: every asset of `[package.metadata.generate-rpm]` in
  `crates/ono-cli/Cargo.toml` states its `mode`, and `scripts/package.sh` pins `umask 022`. The
  cause is not known. Reproduction: `cargo test -p xtask --test packaging
  should_produce_identical_hashes` on an otherwise idle machine, then compare the two RPMs
  (`rpm -qp --dump`, `rpm2cpio`). What closes it: find the header field or payload entry that
  differs and pin the input behind it. Not yet confirmed as pre-existing —
  this run is the first in which it was observed, and the machine was building several things at
  once.
- **The whole-process temporal path grows with the store where the engine does not (2026-09-09).**
  §32.3's budgets hold comfortably at the engine level against §49's million-event fixture — `why`
  1.9 ms, `reconstruct_recent` 54.4 ms, `changes_1h` 38.0 ms, `timeline_15m` 6.0 ms, one process per
  sample, release build (ADR-0776). Whole-process CLI medians over a store the shell itself built
  tell a different story two orders of magnitude earlier: `changes --since 1h` is 124 ms at 7,000
  events, 271 ms at 15,000 and 351 ms at 20,000 against a 150 ms budget, with `timeline` at 105 ms
  against 100 ms. Store *open* alone grows linearly — 16 / 21 / 38 / 67 ms at 1k / 3k / 7k / 15k
  events — which is the part that should be constant and is the likely cause. Reproduction:
  `ono --no-config -c 'start recorder; <query>'` over stores of those sizes, timed whole-process.
  What closes it: find what opening a store reads that scales with its contents, and stop reading it
  at open. §32.2's rule that a benchmark exercise production logic is what makes the gap matter —
  the benchmark measures the engine and the person measures the shell.

- **`why <target>` reports a constant subject identity (2026-09-09).** `why process 1`,
  `why process 9` and `why link testbox` all answer
  `"subject":"ono:stable:241accd898e18ecb3852398a06e31a68"`. §16.2's first form names a target and
  the explanation is meant to be about that target. What closes it: resolve the target to its
  spatial identity before the request is built, the way `at event` resolves a reference.

- **Nothing checks that a command produces the schema it declares (2026-09-09, found while making
  `timeline` pipeline-compatible).** `docs/contracts/commands/temporal.yaml` declared
  `output: stream<ono.temporal-event/1>` for `timeline` from the day the command was registered,
  and the implementation answered with one `ono.temporal-timeline/1` record for just as long. The
  v0.2 §11.3 pre-flight type check validates a pipeline against the *declared* type, so it passed;
  `spec-check` compares the registry against the command's declaration rather than against the
  value it builds, so it passed too. The drift was found by a person reading the spec, which is the
  one method that does not scale. Reproduction, before ADR-0778:
  `ono -c 'timeline --since 1h | where kind == "object.changed"'` → `Ono-Sendai-E0202`, on a
  pipeline the command's own contract lists as an example. What closes it: a conformance check that
  runs each command's declared examples and validates the value against the declared output schema
  — the machinery exists in `xtask/src/conformance.rs` for providers and would need the same for
  commands.

- **`temporal.why.max_candidates` was a cap on the explanation rather than on the work
  (2026-09-09, found while measuring §32.3 against the §49 fixture).** Fixed in this increment, and
  recorded here because the *shape* is worth filing against the rest of the tranche: `why` read
  every event its scope held up to the coordinate — `limit: None` — and the engine then dropped
  all but the last thousand. On the million-event fixture that is the whole store in memory to
  explain one transition. `crates/ono-cli/src/temporal/views.rs` now passes the ceiling to the
  ledger as well as to the engine. What is *not* closed: nothing checks that a command's declared
  bound reaches the query rather than only the answer. Read against that question on 2026-09-13:
  `timeline` and `find event` pass their limit to the ledger; `changes` does not —
  `crates/ono-temporal-query/src/changes.rs` reads its whole window with `limit: None`.
  Reproduction of the class: grep `crates/ono-temporal-query` and `crates/ono-cli/src/temporal`
  for `limit: None`.

- **The §49 perf harness can label a debug figure as a release one (2026-09-09, found while
  reproducing the two missed budgets).** `xtask perf` takes the build profile from
  `target/{release,debug}/ono`, but a temporal row is sampled by re-running the *xtask* binary
  (`perf::Runner::run_temporal` → `std::env::current_exe`), whose profile can differ. A
  `cargo run -p xtask -- perf` with a release `ono` present reports "release build" over figures
  measured by a debug xtask — 419 ms against 220 ms for `temporal.why`, which is the difference
  between a missed budget and a held one. Reproduction: build `--release` for `ono` only, then run
  the perf task through `cargo run` without `--release`. What closes it: sample through the
  built binary's own profile, or refuse to record a temporal row when `current_exe` is not the
  profile the run claims.

- **A backward wall-clock step splits one process into two spatial identities (2026-09-08, found
  while giving the process provider the kernel's boot id, ADR-0713).** Half of a process's
  identity digest is `started`, and `ProcessProvider::started` computes it as
  `boot_time_seconds + stat.starttime / clock_ticks` — `/proc/stat`'s `btime`, which is a
  wall-clock second the kernel recomputes, plus an offset. An NTP correction or a manual clock
  step changes `btime`, so the same running process observed before and after the step yields two
  different `started` values and therefore two different `SpatialId`s. v0.5 §25.4 asks that a
  clock jump not disturb what the ledger knows, and this disturbs identity itself: a
  reconstruction across the step sees one process end and another begin, with no evidence that
  anything happened. Reproduction: observe a process, step the host clock backward by a minute,
  observe it again, and compare `spatial_id`. What closes it: key the identity on the
  boot-relative tick count (`stat.starttime`, which does not move) rather than on the derived
  wall-clock instant, and keep the instant as a rendered field. **This moves every process
  `SpatialId` in the tree**, so it is its own increment with its own ADR and its own list of
  changed tests; the boot-id read landed in v0.5 is a prerequisite for it and not the fix.

- **A plugin artifact behind a private certificate authority cannot be fetched.**
  `ureq` verifies against the Mozilla root store `webpki-roots` embeds (ADR-0607 §4), so an
  enterprise HTTPS artifact server presenting an internally issued certificate is refused —
  exactly the operator K11A §2.5 has in mind, whose path today is the system package or a local
  copy rather than a catalog fetch. What closes it: an operator-configured additional root, or
  the platform verifier, decided in an ADR that weighs it against ADR-0607's reason for a root
  set that is the same on every host.
- **A `docs/architecture/external-system-provider.md` §21.4 violation reachable by any package
  with an unbounded target, fixed in `ADR-0590`, and worth a regression watch (2026-09-06).**
  `plugin_provider::stream_of` returned on the end of the output stream without reading the
  invocation result, so a handler that answered `Outcome::Failed` before emitting anything arrived
  at the prompt as a clean empty answer. Fixed, with acceptance case
  `129-kuang-unbounded-target`. What is *not* fixed and is worth someone's judgement: the
  supervisor answers an invocation on two channels — the value stream and the result — and nothing
  in the protocol contract says a consumer must read both. `snapshot` does; `stream_of` did not,
  for four days. A second consumer will be written eventually. What would close it: either the
  supervisor also sends `StreamEvent::Failed` before dropping the stream, so one channel carries
  the whole answer, or the contract says in words that both must be read.

- **`spatial_orientation_bound.rs` compares two independent reads of the machine's live service
  list, and fails when the machine changes between them (2026-09-06).**
  `should_count_a_bounded_target_by_what_the_provider_says_is_there` and
  `should_leave_what_a_user_asks_for_directly_unbounded` each call `population("service")` — a real
  `get service | count` against this host's service manager — and then compare it to a second read
  taken by `look`. Observed: `left: Some(573)`, `right: Some(570)`, during a session in which a
  `kind` cluster was being created and destroyed in another process, which registers and retires
  systemd scopes for its containers. Both tests pass in isolation, on the same tree, seconds later.
  AGENTS.md §11 names the rule they break: a test must be deterministic and must "never rely on the
  developer machine's real processes unless the fixture creates them". What closes it: take one
  reading and derive both assertions from it, or drive the pair against a fixture that owns its
  units. The bound under test — `limits.orientation_objects` against the whole population — is a
  real contract and worth keeping; only the way the population is obtained is wrong.

- **A contributed command may declare a mutating capability and no risk (2026-09-06).**
  `docs/contracts/kuang/contributions.v1.yaml` → `registration_checks.risk-metadata` says "Every
  mutating command and every assistant mutation tool declares its risk", and ADR-0587 carries the
  declaration into the registry without making it required. Reproduction: a contribution declaring
  `capabilities: [network.connect]` — `Capability::NetworkConnect.risk()` is `mutate` — and no
  `risk:` line registers, and `help` shows no risk at all. `ContributedCommand::into_contract`
  already has both the capability list and the risk in hand, so the check is a few lines; what
  makes it a separate increment is that it *refuses* a package that installs today, and ADR-0587's
  increment was constrained not to break one. What closes it: a test that a contribution declaring
  a `mutate` or `destructive` capability without a risk is refused and reported, plus the risk
  lines the two example packages would then need.

- **A contributed command's declared `destructive` risk asks for no confirmation (2026-09-06).**
  ADR-0587 makes the declaration visible; it does not act on it. §21.5 of
  `docs/architecture/external-system-provider.md` says confirmation belongs to host safety policy
  and that a provider returns structured risk so the host can apply consistent rules — and the
  contributed path in `crates/ono-cli/src/eval/pipeline.rs` calls `invoke_contributed` directly,
  bypassing the binding and confirmation the core commands go through, so
  `CommandContract::confirmation` is decorative for a contribution whichever value it holds.
  Reproduction: a contribution with `risk: destructive` runs without a prompt in an interactive
  session. What closes it: routing a contributed stage through the same confirmation gate a core
  mutating command uses, with a pty case that a `destructive` contribution refuses in a
  non-interactive context (`safety.confirmation_required`) and prompts at a terminal.

- **The host closes every open view when any invocation ends (2026-09-06).** With concurrency in
  the SDK (ADR-0586) an instance can have two invocations open, and
  `ono-kuang-supervisor::supervisor.rs` → `handle_envelope`, the `Pending::Invocation` arm, calls
  `close_all_views(false)` whatever invocation just answered. Reproduction: two invocations of a
  command that opens a view under `RecordingViews`; the first to finish tears down the second's
  view, and the second's `next_view_event` sees `unmount` it did not earn. `OpenView` already has
  a struct of its own and would need the invocation handle beside its `id`, with the close
  filtered by it — spec §31.28 says a view outlives no invocation, which is true of *its own*
  invocation and of no other. What closes it: a conformance case with two view-opening
  invocations open at once, asserting each view survives the other's end. Not fixed in ADR-0586's
  increment because it is a host-side change with a test of its own (AGENTS.md §4).

- **A failing streamed adapter child reports exit 0 under load (2026-09-03).** Gate run after
  the #3 views increment: `adapters.rs::should_report_a_failing_streamed_child_after_its_records`
  — a `journalctl` shim of `echo '<entry>'; exit 3` — came back with status **0** and the
  assertion `run.status().code() != 0` failed; the record itself had arrived. In isolation the
  test passes 3/3 in 40 ms; it failed once in one gate run with the whole workspace's tests
  beside it. Spec v0.3 §1.20 says the child's status still stands after its records, so if
  this is the product, a failing adapter's status is lost when the shell is under load, which
  is the class the board's 2026-09-03 entry on load-sensitive tests warns about. Not
  investigated here: it is outside #3's scope and the user triages. Reproduce with
  `scripts/gate.sh` or `cargo test -p ono-cli` under CPU load.
- **`docs/contracts/schemas/limit.v1.yaml` is not embedded.** `ono_value::builtin_schemas()` lists
  ninety contracts by hand and this one is not among them, so `ono.limit/1` is a schema the
  registry cannot answer for although the document exists. Found while writing the fidelity test
  of ADR-0571, which therefore checks that every embedded document matches disk and leaves
  completeness to `spec-check` — which does not ask this question either. Either embed it or
  have `spec-check` compare the directory with the list.
- **`files.rs::should_report_a_created_file_before_the_next_poll_would_have_come` fails under
  load.** It asserts a wall-clock bound of 3,5 s around a 2,5 s sleep, and failed once in the gate
  while a release build (`lto = "thin"`, `codegen-units = 1`) ran beside it on the same 8 cores;
  it passes alone in 2,55 s. A test of §18.2's subscription-versus-poll distinction that depends
  on one second of free CPU is a coin toss on a shared runner (ADR-0431's argument) — the bound
  should be measured against the poll interval the shell actually configured, or the test should
  read `source` alone and leave the clock out of it.

**A refused link reports `remote.unreachable` instead of `remote.unauthorized` (2026-09-03).**
§12.5's revocation sweep (1 s) calls `ConnectionRegistry::revoke_absent`, which closes **any** live
connection whose fingerprint is not in the store — including the one `serve_registry` is at that
moment refusing for exactly that reason. `closed` wins the `select!`, the transport is dropped, and
the peer sees a socket that went away rather than a refusal that says why. Audit from a probe run:
`connection.disconnected connection_id=revoked … error_code=remote.unauthorized` at `…130200136Z`,
then `connection_id=conn-1 source_address=127.0.0.1:37036` at `…130412437Z` — the sweep beat the
refusal by 200 µs. Fails `authenticated_link::should_refuse_an_authenticated_client_the_agent_never_authorized`
and `::should_report_an_authenticated_but_unauthorized_link_as_exactly_that` in 2 of 6 workspace
runs at load 22–26, 0 of 12 at load 9–13, and 2/15 in isolation at load 23. §54.1 and §59.9 require
the refusal to arrive. **Likely fix:** arm `closed` only when `store.client(fingerprint).is_some()`
at admit — a client that was never granted is not a grant being withdrawn. Needs an ADR; it touches
§12.5 semantics. **Exit test:** both tests green over 30 runs at load 25.

**The suite leaks long-lived children, and one test's final assertion never checked (2026-09-03).**
Found on this host: **158** orphaned `journalctl --follow` stubs, the oldest 25 hours, scratch
directories long deleted, from `adapters.rs::should_follow_the_journal_live_at_the_terminal_until_interrupted`
— whose closing assertion *says* "the follower is gone" and only checks that the prompt returned,
so a real leak has been passing for as long as the test has existed. Plus **8** `ono -c 'enter
socket …; map --live --json | take 3'` from `spatial_relationships.rs`, alive eleven minutes at
~30 % CPU each (231 % together), left behind when a failing `cargo test` ends early. Both pollute
the host population the timing-sensitive tests are sensitive to, so this feeds the flakes above.
ADR-0516 closed the `PtySession` half; this is the rest. **Exit test:** a `cargo test --workspace`
that *fails* leaves no `ono` or fixture child behind, and the follower test asserts the child's
death rather than the prompt's return.
  *Recurred 2026-09-04, unchanged:* **46** more of the same stubs on this host, oldest 24 h, every
  `/tmp/ono-test-*` directory already gone, all reparented to `systemd --user`. Each is a busy loop
  spawning `sleep 0.2` — ~230 spawns/s across the set, 93 MB RSS, ~30 s CPU each. Killed by hand
  again. Note for whoever fixes it: `pgrep -f 'ono-test-.*journalctl'` matches the killing shell's
  own command line, so the sweep must select on `argv[0] == /bin/sh` and `argv[1]` under
  `/tmp/ono-test-`, not on the pattern.
  *First seen 2026-09-02:* **331** of the same followers
  (`/bin/sh /tmp/ono-test-<pid>-4/journalctl --output=json --no-pager --follow`), the oldest five
  days old, killed by hand. The stub is `journal_shim` in `crates/ono-cli/tests/adapters.rs`;
  whatever spawns it must own its lifetime, killing *and reaping* it as `support::run_bounded`
  does for issue #22's fixture.

**`ono` still panics on a closed stderr outside the agent (2026-09-03).** ADR-0549 introduced
`ono_core::diagnostic!` and applied it to the agent paths, where the defect was costing a live
listener. **95 `eprintln!` call sites remain** across the workspace — `ono -c … 2>&1 | head -0`,
the usage path and `--print-peer-key` among them. The stakes are lower because the process is
ending anyway, but it turns exit 1 into exit 101, which is the difference between a refusal and a
crash to anything reading the status. **Exit test:** no `ono` invocation exits 101 because nobody
read its diagnostics.

**`jobs_native::should_finish_a_bounded_background_pipeline_and_say_so` waits a fixed 0.4 s
(2026-09-03).** `get process | count &; sleep 0.4; jobs` — under load the job has not finished. 1 of
12 workspace runs at load ~10. **Exit test:** it polls `jobs` for `done` under a watchdog instead of
sleeping.

**`spatial_topology::should_stream_neighbors_as_pipeline_objects_when_near_runs_at_the_root`
compares two runs of `near` (2026-09-03).** Got 36 against 35: two separate `ono` invocations, and
the host moved between them. Same family as ADR-0552's width comparison. 1 of 12 workspace runs.
**Exit test:** both counts come from one shell run.

**Profile L's live map sets the ceiling on how loaded a gate machine may be (2026-09-03).**
`spatial_first_output::should_answer_or_refuse_within_the_interactive_budget_on_the_profile_l_fixture`
failed 4 of 6 workspace runs at load 22–26 and 0 of 12 at load 9–13. Its 30 s `run_bounded` budget
is deliberately **unscaled** (ADR-0517, ADR-0431), because there the duration *is* the observation —
so this is an accepted cost written down rather than a new defect. What it establishes is a number
worth knowing: the gate can be trusted up to roughly **1.5× `nproc`** and not beyond. **Exit test:**
either the Profile L live map answers inside 30 s at load 25, or the case names the machine its
budget is measured on.

**`get filesystem` calls two tmpfs superblocks one filesystem (2026-09-03).**
`stream_filesystems` dedupes by `(source, type)`, which is right for a bind mount and wrong for two
independent anonymous mounts: `/run` and `/dev/shm` are both `tmpfs|tmpfs` with different device
numbers (`0:29`, `0:69`) and only the first is reported.
`ono -c 'get filesystem | where type == "tmpfs" | count | to json'` answers `[1]` on a host whose
`/proc/self/mountinfo` holds four. Now that `ono.filesystem/1` carries `device_number` (ADR-0553),
the honest dedupe key is the superblock. It changes what `get filesystem` answers, so it needs its
own increment and its own acceptance evidence. **Exit test:** the count matches the superblocks.

**An option whose evaluated value does not fit its declared type is dropped rather than refused
(2026-09-03).** With ADR-0556 in, `get command --verb ["get"]` now *reaches* the command as a
one-element list where `docs/contracts/commands/meta.yaml` declares `string`; `as_str()` fails, the
filter is skipped, and the reader who asked a narrower question receives the whole registry. §2.6
again — a filter that silently did not apply is worse than a refusal. The check belongs in the
binding layer beside the declared type rather than in each command. **Exit test:** a wrongly typed
option is refused by name.

**`cargo xtask perf` cannot adjudicate on a shared machine, and says nothing about that
(2026-09-03).** All eight Profile S benchmarks read three to five times their checked-in baseline —
`shell.cold_start` at 132 ms against 26 ms — while a second build tree held the load. Absolute
tolerance is right for release qualification (§32.4); what is missing is that the comparison
**reports a regression** where it should report that the environment was not the reference one.
`Comparison` already answers `ForeignEnvironment` for the wrong machine (ADR-0489); it needs the
same honesty for the right machine under the wrong conditions. **Exit test:** a benchmark run under
load reports that rather than a regression.

**`SECURITY.md`'s boundary table is a hand transcription and nothing compares it to the inventory
(2026-09-03).** `docs/contracts/hardening/security_boundaries.yaml` exists and
`docs/reference/security-boundaries.md` is generated from it; `SECURITY.md`'s copy is still typed,
so a renamed boundary leaves it silently wrong. §4.8.12's box for #114 says so. **Exit test:** a
boundary renamed in the inventory turns the gate red where `SECURITY.md` disagrees.

**One machine-readable contract lives outside the indexed directory (2026-09-03).**
`docs/baselines/v0.4.1.json` and `v0.5.0.json` are validated by `xtask::baseline::check` in
`spec-check`, but
`registries.yaml` indexes `docs/contracts/hardening/` only, so §52.3's "every contract is indexed"
property has a deliberate exception recorded in ADR-0548's *Consequences*. **Exit test:** either
the index reaches contracts outside that directory, or the snapshots move into it.

**`rustls-pemfile` is archived, and the dependency policy now says so out loud (2026-09-02).**
RUSTSEC-2025-0134: the crate is unmaintained. It is waived in `deny.toml` with a reason and an
`expires = "2027-03-01"`, and `xtask/src/supply_chain.rs` fails the gate once a waiver's deadline
passes, so the waiver cannot quietly become permanent. The replacement is
`rustls_pki_types::pem::PemObject`, and the migration belongs to whoever owns `crates/ono-remote`
— the crate reads the local certificate and key files that are a host's own pinned identity
(ADR-0353, ADR-0449). **Exit test:** the workspace no longer depends on `rustls-pemfile`, and the
waiver is deleted rather than extended.

**`explain … | to json` cannot yield the plan as data (2026-09-02).** The `explain` builtin
consumes the whole line, so `explain get process | sort m | to json` explains the `to json` stage
rather than serialising the plan. `ExecutionPlan::to_value` is correct and unreachable from the
shell. Closes when the builtin stops swallowing the stages downstream of it. **Exit test:**
`explain <pipeline> | to json` answers the plan.

**`measure` materializes for statistics that do not need it (2026-09-02).** `count`, `sum`,
`mean`, `min` and `max` are constant-state; only the percentiles need the distribution held.
Splitting them moves half of `measure` onto the incremental path (ADR-0455). **Exit test:**
`measure count` over an unbounded source answers without materializing.

**`history.result_cache` is a superseded duplicate of `limits.history_bytes_total`
(2026-09-02).** It is kept declared only so existing configuration files still parse (§4.5).
Retire it in a release that may break configuration. **Exit test:** one key names the ceiling.

**A non-interactive completion surface (2026-09-02).** ADR-0252 named it and #21 could not
deliver it: both routes — a flag in `crates/ono-cli/src/invocation.rs`, or a command registered in
`crates/ono-cli/src/native.rs` — are new public surface. #21's budget is now measured directly by
`xtask` re-running itself, so this is no longer blocking a proof; it is what would let the
container measure a first completion end to end. **Exit test:** case `060` measures the first
completion without a terminal.

**§34.4's "visible in `explain`" is unmet (2026-09-02).** An unavoidable global build MUST be
visible in `explain`. The estimate exists in `ono_spatial_query::cost` after H7, and `explain`
lives in `ono-command`, which H7 did not own. **Exit test:** `explain` over a query with a global
acquisition names it and its cost class.

**§36.2's incomplete marker needs a return type that can carry it (2026-09-02).**
`ono_command::ValueCompleter::complete` and `ono_command::complete` return a bare
`Vec<Candidate>`, so a completion truncated at the soft budget cannot say it was truncated. §36.2
makes the marker a MAY, which is why H7 did not force it. **Exit test:** a truncated completion is
distinguishable from a complete one.

**`observe` is sequential across provider targets (2026-09-02).** Fetching concurrently and
absorbing sequentially would bring the selector miss from 943 ms to roughly its slowest provider.
Like the orientation item above, it changes the observation contract rather than one call site.
**Exit test:** a miss across N providers costs about the slowest, not the sum.

**`Interest::wants` scans the relation table once per provider per observation (2026-09-02).**
Thirty-odd rows, once per `look` — negligible today, indexable if the table grows. ADR-0495.
**Exit test:** none needed until the table does grow; recorded so the next reader knows it was seen.

**Error metadata is never rendered by any production path (2026-09-03).** `Reporter::error`
prints only `metadata["details"]`. `ono_render::Layout::render_error` with `Detail::Full` — which
prints `metadata key = value`, and whose own doc comment says it is what `inspect @error` shows —
has callers **only in `crates/ono-render/tests/error_rendering.rs`**. So H2's `denied_because`,
H4's `control` and `execution_tier`, and H5's `limit` and `consumed` reach no screen. Three phases
each proved their metadata exists and none proved a user can see it, because a test that reads the
structured value never passes through the renderer. **Exit test:** a refusal shown to a user
carries the field that names the deciding boundary.

**`Ono-Sendai-E1502` never reaches the peer (2026-09-03).**
`crates/ono-remote/src/listener.rs:544-552` builds `handshake_timed_out`, audits it, `eprintln!`s
it on the **agent's** stderr and returns — without calling `ono_protocol::refuse`, unlike the
E1501 path directly beside it. `refusal_guidance(RemoteHandshakeTimeout)` is therefore
unreachable. **Exit test:** a client whose handshake times out receives E1502.

**Handshake-time refusals lose all metadata over the wire (2026-09-03).** `Reject { code, message }`
carries none, so `store_present` — which E1202's registry help explicitly promises the client —
never arrives. **Exit test:** an unauthorized client can tell an empty store from a store that
lists somebody else.

**`Ono-Sendai-E1103 resource.materialization_limit` is declared and never constructed
(2026-09-03).** Its only occurrences are the enum and a code/name test. Recorded as
`raised: false` in `docs/contracts/hardening/refusals.yaml` rather than quietly deleted. **Exit test:**
either a path raises it, or it leaves the taxonomy.

**`docs/contracts/kuang/errors.v1.yaml:20` says "Nothing here is implemented" (2026-09-03).** Stale for
the K118xx block, which H4 delivered. One line. **Exit test:** the file describes what is there.

**Two worktrees share one acceptance image tag (2026-09-03).** `scripts/acceptance.sh` defaults
`IMAGE` to `ono-sendai:acceptance`, and a run finishing without `--keep-image` **deletes it out from
under a concurrent run in another worktree**, which then reports every remaining case as exit 125
`Unable to find image`. Observed: 16 spurious failures in a full run during the H11/H12 pair. The
run that produced the green result used `ONO_ACCEPTANCE_IMAGE=ono-sendai:acceptance-h11`. This is a
direct cost of running phases in parallel worktrees, and it fabricates failures rather than hiding
them. **Exit test:** two concurrent `scripts/acceptance.sh` runs in two worktrees both report their
own results — by deriving the default tag from the worktree, or by refusing to remove an image
another run is using. The filesystem cases' second image, `${IMAGE}-filesystems` (`c935073b`),
derives from the same default and shares the problem.

  **It can also produce a false _green_.** H12 saw case 200 pass while carrying the message from
  *before* its own change — the run had used the other worktree's binary. A `docker builder prune
  -af` and a clean rebuild corrected it. A tool that lies green under parallel use is worse than
  one that lies red, because nothing prompts a second look.

**`dist/` accumulates across versions (2026-09-03).** `scripts/release-check.sh` writes into it and
`xtask checksums` covers every file there, so a local manifest lists 0.3.0 packages beside 0.4.0.
Truthful about that directory and wrong about a release. **Exit test:** release-check builds into a
directory it owns.

**`ETXTBSY` becomes exit 126 with "found and not executable" (2026-09-02).** A user running a
script somebody else is still writing is told the file is not executable, when it is.
`spawn::exec_failure`'s catch-all arm is where "text file busy" would be said instead. ADR-0520
§Consequences. **Exit test:** running a file held open for writing names the busy file.

**A selector miss at Profile M costs 900 ms against §36.1's 250 ms target (2026-09-04).**
Unchanged by ADR-0576, and measured again by it: a miss sweeps every acquisition class before it
can say `not_found`, and the last class is the systemd enumeration. A hit that resolves out of the
index costs 197 ms. §36.1's MUST — that a miss not be substantially more expensive than a hit
*solely because the system scans an unnecessarily complete global candidate set* — is met by the
cheapest-first sweep of ADR-0497; the 250 ms p95 target is not, and what is left is the cost of
the last class rather than the size of the candidate set. Bounding the sweep is not the answer: an
`enter` that stopped early would report `not_found` about something that is there, which is the
defect ADR-0576 had to fix in its first form. Measured by
`cargo run -p xtask -- perf --profile M --iterations 20` on `ryzen-3900x-ubuntu-2604` and recorded
in `docs/contracts/hardening/performance_baseline.json`. **Exit test:** `spatial.selector_miss` at
Profile M under 250 ms p95.

**Four suites still exec a file they have just written (2026-09-04).** ADR-0578 took the race out
of `ono-model-broker` after CI hit it — a concurrent test's `fork` inherits the write descriptor
and the `exec` answers `ETXTBSY` — by running the script through `/bin/sh` instead. The same shape
is in `crates/ono-kuang-sdk/tests/conformance.rs`, `crates/ono-kuang-supervisor/tests/confinement.rs`,
`crates/ono-cli/tests/adapters.rs` and `crates/ono-provider-linux/tests/package_sources.rs`. The
testkit's helper, `ono_testkit::executable_script`, writes in place too; of the four suites only
`adapters.rs` uses it, and its journal shim retries through `ono_testkit::while_text_file_busy`.
None has been observed failing, so none was changed. **Exit test:** the helper writes through a temporary name and renames,
or the suites take ADR-0578's route, and no `ETXTBSY` appears in a CI log again.

**Case 152's §34 budget sits one millisecond from its limit (2026-09-02).**
`docker/acceptance/cases/152-pathological-sockets.case` measures `get socket | take 1` against a
50 ms budget as the median of 20 runs. In a full 125-case run on a machine at load 6.8 it read
**51 ms** and `get connection | take 1` read **56 ms**, and both were reported OVER BUDGET. Run
alone at load 6.6 it passes; run beside case 151, which forks ten thousand processes, it passes;
and it passed on the GitHub runner. The baseline row on the ordinary host in the same failing run
was **48 ms** — two milliseconds of headroom on a figure the median is supposed to protect.

So this is not a regression and it is not the leak that was first suspected: case 151's children
block on a pipe and exit with their parent, and 151 followed by 152 is green. It is a budget with
no margin, measured on a machine the case does not own. **Exit test:** the case states what it does when the machine cannot meet the
budget, rather than reading a number two milliseconds from the edge and calling it a defect.

**Four stale setting descriptions (2026-09-02).** `crates/ono-cli/src/settings.rs`'s
`limits.remote_*` rows still say *"Declared and validated; enforcement is phase H3's"*. H3 is
delivered and the rows are enforced by `ono-remote`. User-visible through `get config` and
`inspect limits`. One line each. **Exit test:** the description matches `enforced_by`.

**Agent mode honours only the environment layer of `limits.remote_*` (2026-09-02).**
`configured_limits()` in `crates/ono-cli/src/main.rs` reads `Settings::new()` plus
`apply_environment`; `config.ono` is not read, because `config::load` needs a `Session` an agent
does not have. ADR-0504. **Exit test:** a ceiling set in `config.ono` changes what `--listen`
enforces.

**A function cannot be invoked between two pipeline stages (2026-09-02).** `get process | mine |
take 1` resolves `mine` as an external program; `call_function` is reached only for
`list.stages[0]`. Giving a function an input stream is a language feature rather than a streaming
repair, so §65.12 kept it out of H6 (ADR-0481). **Exit test:** a call in a non-head position binds
its input stream, and `explain` names it.

**A function body containing `each { … }` still collects (2026-09-02).** `native::stream_segment`
refuses a block stage, because the block bridge identifies a block by its *position* in the stage
list the driver holds and a body's stages are in a different list. The fix is to make
`BlockRequest` carry a block **site** rather than an index. ADR-0481. **Exit test:** a function
whose body is `each { … }` streams like one whose body is `where`.

**A backgrounded `each { … }` is still unsupported (2026-09-02).** `run_background` has no session
to ask, so it fails as it did before the rewrite. ADR-0480. **Exit test:** `each { … } &` runs.

**`set client-key --allow` takes a comma-separated word rather than a repeated option
(2026-09-02).** §9.7 writes `--allow <capability>...`, and `ono_command`'s binder keeps one value
per option while a bare comma ends a word, so the spelling that works is
`--allow "process.signal,service.manage"`. The binder carries a repeated option now
(`is_repeatable` in `crates/ono-command/src/contract.rs`), but `docs/contracts/commands/remote.yaml`
still declares `allow` a comma-separated `string`; closing it means declaring the option
repeatable (ADR-0468 §Alternatives). **Exit test:** `--allow a --allow b`
grants both.

**The trust store's writer is weaker than the authorization store's (2026-09-02).**
`TrustStore::persist` in `crates/ono-protocol/src/trust.rs` writes a temporary, fsyncs and renames
— with no explicit mode, no directory sync, and it *truncates* a leftover temporary rather than
refusing it. `ono_protocol::write_store`, written for #41, is §9.8's full sequence sitting right
beside it: `create_new` + `0600` + fsync + rename + directory sync. Two files holding key material,
two different levels of care. ADR-0467 §Consequences. **Exit test:** both writers survive the same
interrupted-write proof.

**AGENTS.md §12.1's sub-branch convention cannot be used as written (2026-09-02).** It says
*"Sub-branches are allowed for parallel agents (`implementation/<crate>`)"*, and git refuses to
create one: a ref named `implementation` and a directory `refs/heads/implementation/` cannot both
exist, so `git worktree add -b implementation/h7-spatial-performance` fails with *cannot lock ref
… 'refs/heads/implementation' exists*. The convention is only usable if the trunk branch is
renamed, which is the user's call and would touch §12.1, the gate's `main` guard and every ADR
naming the branch. The parallel worktrees of 2026-09-02 use `implementation-<phase>` instead.
**Exit test:** the convention as documented can be executed, or the document names the form that
can.

**A killed pre-exec child reports the first mandatory control rather than the reason
(2026-09-02).** If the child dies between `fork` and `install_controls`, every row of the
confinement report reads `not_attempted` and the refusal says `an earlier mandatory control failed
first`. Honest, and unhelpful. Closed by folding the `io::Error` from `Command::spawn` into
`ConfinementReport::refusal` when no row reads `failed` (ADR-0445 *Consequences*). **Exit test:** a
spawn that fails before the first control names the spawn failure.

**`Sandbox` is now the least accurate identifier in `ono-kuang-supervisor` (2026-09-02).** It
carries an `ExecutionTier` and has stopped being "the native process sandbox". A pure rename,
deliberately left out of a `feat` increment (AGENTS.md §4, ADR-0448). **Exit test:** the type is
named for what it is.

**`ono.plugin/1` carries both `isolation` and `execution_tier` (2026-09-02).** Two adjacent fields
answering related questions is a real cost; `isolation` holds spec §31.10's *manifest* vocabulary
and `execution_tier` holds what the plugin actually runs inside. Removing `isolation` is a schema
break and belongs to whichever increment bumps `ono.plugin/1` (ADR-0448). **Exit test:** one field
answers one question.

**`ono.link/1`'s `transport_trust` can spell `newly_pinned`, which no production path produces
(2026-09-02).** The CLI's `tcp` link uses `TrustPolicy::Pinned`, so the value is reachable only
through the library. Harmless today; it becomes either a real state or a value to delete once H2
decides whether an operator-facing trust-on-first-use mode exists at all (ADR-0438). **Exit test:**
either a production path produces `newly_pinned`, or the schema stops offering it.

**`--agent --host-key <path>` bypasses the §8.2 identity ladder entirely (2026-09-02).** An
operator can run a listening agent on an identity that diverges from `link_identity.pem`. §8.2
rule 5 is satisfied per role and ADR-0435 records the divergence as deliberate, so this is a
visibility gap rather than a defect: nothing shows an operator that the two files disagree. A
`get identity`-shaped surface naming both would close it. **Exit test:** a diverging pair is
visible without reading the filesystem by hand.

**The gate now needs `cargo-deny@0.20.2` installed (2026-09-02).** `scripts/gate.sh` runs
`cargo deny --locked --all-features check` and exits 127 with the install command when the tool is
absent, the same shape `cargo-deb` already uses (ADR-0121). Any machine that runs the gate needs
`cargo install --locked cargo-deny@0.20.2`. This is a note rather than a defect; it is here so the
next person to meet exit code 127 finds the reason.

**`socket.accepts_connection` is composed, and nothing proves it (2026-09-13, found while cleaning
this board).** The *Deferred* entry that called the relation unobservable was wrong:
`ProviderBridge` composes the edge from a listener and a connection that share a local endpoint
(`crates/ono-spatial-index/src/bridge.rs`, ADR-0147), and `crates/ono-cli/src/spatial/session.rs`
observes through that bridge. No test drives a real listener and a real accepted connection
through it — the only test that names the relation is the synthetic fixture in
`crates/ono-spatial-events/tests/common/mod.rs` — and ADR-0132 and ADR-0135 still describe it as
underived, against ADR-0147. **Exit test:** a test that accepts a loopback connection and finds the
`socket.accepts_connection` edge between the two sockets.

### Carried over from the session records removed on 2026-09-13

Each of these was recorded as open inside a session record and was tracked nowhere else, so it
moved here when the records left the board (*History*). *Open at `6485904d`* means the code was
read and still shows it; *not re-checked* means it was carried over as written. The original text
is at `git show 6485904d:docs/STATE.md`, at the lines given.

- **Three KUANG/11 host services answer `provider.unavailable`.** `relations.contribute` (no store
  for a package's edges), `history.append` (a history entry has no field for a package's
  authorship) and `secrets.request` (no secret store), in `crates/ono-cli/src/kuang_services.rs`.
  Honest gaps rather than fakes. Open at `6485904d`. L598–604.
- **`ono-testkit`'s `Shell` kills the child, not its process group.** The crate forbids `unsafe`
  and a group needs `pre_exec`; `run.rs`, `bounded.rs` and `profile.rs` call `child.kill()`. Open
  at `6485904d`. L786–790.
- **Canonical spaces are not answered by `find place`.** It searches the index, and a space is
  declared geography rather than an observed object; whether `find place compute` should answer
  the domain is a decision nobody has recorded. Not re-checked. L1251–1254.
- **Spatial collections and relations without a provider.** `network/addresses`, `compute/cgroups`
  and `network/namespaces` answer `unsupported` — `cgroup` and `namespace` are `phase: planned,
  schema: null` in the target registry — and `interface.has_address` has the same shape.
  `process.connects_to` is declared and nothing serves it. A file place's `owner` is `unknown`,
  because `user.owns_file` is expensive and not loaded on `near --type user`. The collections are
  open at `6485904d`; the rest is not re-checked. L1400–1405, L1537–1547.
- **`ono.system/1` carries `kernel` and `uptime` as null** (`crates/ono-cli/src/spatial/view.rs`):
  no provider answers for them and §2.16 forbids the spatial layer reading them itself. A `get
  system` producer fills them. Open at `6485904d`. L1408–1410.
- **A file's trail reference reads `file/0:46`.** `ono.file/1`'s identity is `[device, inode]`, so
  a trail step names a file in a form nobody types; a path-shaped alias would fix it. Not
  re-checked. L1630–1634.
- **Four landmark and map gaps.** §26.2's high-memory and restart-loop rules cannot fire (no memory
  budget, no restart count); four §26.2 network rules have no word in §3.7's reason vocabulary;
  clustering has one dimension; `map` honours `COLUMNS` when stdout is redirected, unlike every
  other view (ADR-0166). Not re-checked. L1718–1731.
- **The horizon is one synchronous `look`, and `spatial.reduced_motion` has nothing to disable.**
  §5's asynchronous expensive counts need the live map's update channel. Not re-checked.
  L1802–1807.
- **§21.3's container or namespace marker has nothing to mark.** `ProviderBridge` projects every
  observation into the session's own host scope, so `ScopeKind::Container` and
  `ScopeKind::Namespace` exist in `ono_spatial_core::scope` and no place carries them. Open at
  `6485904d`. L1809–1818.
- **The map's `/` search searches the drawn map, and completion asks no provider.** §23.3's global
  half is `find place`, not wired into the view; `enter <TAB>` in an unvisited collection offers the
  declared geography and no members (§34.1's background discovery). Not re-checked. L1819–1824.
- **The link map lacks five things of §19.** No latency or "last seen" field in
  `ono.link-place/1` (open at `6485904d`); no two-sided cross-host correlation; a neighbour reached
  by hierarchy carries a null `confidence` and `provider`; `map links` draws one hop; two links to
  one machine are two scopes. The last four are not re-checked. L1932–1950.
- **No streaming serializer.** `to json` collects, so `map --live --json | take N | to json` prints
  nothing when cut off before the Nth value; a `to jsonl`, or `to json` forwarding one document per
  value on an unbounded stream, would make a live view scriptable. **Exit test:** `map --live
  --json | take 100` prints its first value before the second arrives. Open at `6485904d`.
  L2018–2023.
- **Four tests compare two runs over a live host.**
  `spatial_map.rs::should_only_remove_edges_when_a_relation_filter_narrows_the_map` and
  `::should_only_remove_nodes_and_leave_no_dangling_edge_when_a_type_filter_narrows_the_map`
  compare the process collection across two `ono` runs;
  `spatial_topology.rs::should_show_the_mounts_the_mount_provider_answers_for_when_entering_storage_mounts`
  compares the mount table across two; and
  `spatial_topology.rs::should_complete_the_relations_available_from_the_current_place_when_tab_follows_follow`
  waits for its own echo rather than for the place. All four were seen red on a busy host and
  green on a quiet one; ADR-0552's fix for snapshot pairs does not name them. Not re-checked.
  L1411–1413, L2024–2028, L2191–2199.
- **Three §15.4 and §8.2 increments.** The optional neighbours `open-by processes`, `owned-by
  users` and `changed recently`; clustering directory entries by kind or name; caching an object
  place's relationship edges instead of expanding them on every `look`. Not re-checked.
  L2137–2146.
- **The plan-time `Ono-Sendai-E0911` refusal has no acceptance case.** A consumer declared over
  objects, fed by an invocation no adapter decodes, is refused before the program spawns; the
  proof is `crates/ono-cli/tests/native.rs` alone, and the only case with `E0911` is `082`, on the
  `adapt` path. Open at `6485904d`. L3210–3217.
- **The release build takes about seven minutes warm, and nobody has measured why.** The release
  profile (`lto = "thin"`, `codegen-units = 1`) is the suspect; it bears on #124, which changes that
  profile. Not re-checked. L3965–3966.
- **Three remote gaps left by the RED suite.** A `watch host` that probes reachability (ADR-0105
  left it a later decision), the multiplexed streams of `trace link`, and the execution context in
  the `ono.execution-plan/1` value (still in `docs/contracts/schemas/deferred.yaml`). Open at
  `6485904d`. L4288–4291.
- **The account tools have no privileged conformance run.** Case `043` asserts refusals only; no
  case runs `useradd` or `groupadd`. Open at `6485904d`. L4275.
- **Three container and package gaps.** `trace container` yields only `container.image`, while
  its contract promises edges to namespaces, cgroups, mounts and processes; `enter container` is
  not an execution context (`container.exec`, ADR-0114); the package mutations' success path has no
  root acceptance case. Open at `6485904d`. L4313–4315.
- **A plugin instance reports no memory or cpu, and the install prompt has no PTY case.** `memory`
  is null in `crates/ono-cli/src/kuang_host.rs` and `plugin.v1.yaml` has no cpu field (open at
  `6485904d`); the interactive `install plugin` prompt is not re-checked. L4328–4329.
- **Three shell-language gaps.** `explain` of a `NAME=value cmd` stage; functions and aliases as
  completion candidates; a terminal tree renderer for `get process --tree`. Not re-checked.
  L4351–4352, L4367.
- **Contributed places and relations stop short in five places.** A relation shape cannot name
  another package's schema; a contributed relation has no short word of its own; its cost is
  always `normal` (ADR-0585); a contributed place's scope is always the local host, and the search
  bound is silent (ADR-0584); `subscribe` and `act` are refused because the protocol has only
  `provider.query`, and `get` takes the collecting route (ADR-0583). Later ADRs (0588, 0596) may
  have closed part of it. Not re-checked. L4073–4078, L4104–4106, L4124–4128.

---

## Accepted risks (security review, 2026-08-26, ADR-0015 checklist)

The defects the adversarial and security reviews of 2026-08-26 found — R1–R8 and F1–F13 — are
fixed, F12 by `8ed200b2` (issue #18); the record is in *History*. What the security review
accepted rather than fixed stays here, with the reason, so the decision is not re-made by
accident:

- **F14** — bidirectional and other format characters pass the sanitiser, because
  `char::is_control()` covers only the `Cc` category. Trojan-Source display spoofing of a
  filename. Proposed as an extension of T1.
- **F15** — an empty `PATH` element resolves to the working directory. Deliberate, matches every
  other shell, and `explain` prints the absolute path it reached.
- **F16** — the trust-store temporary (`<store>.tmp`, `crates/ono-protocol/src/trust.rs`) and the
  history rewrite (`history.jsonl.new`, `crates/ono-history/src/store.rs`) are predictable and
  opened without `O_EXCL`. F7 is fixed and the history directory is created `0700`, so what stays
  exposed is a trust store in a directory another user can write.
- **F17** — a residual TOCTOU window remains between confirming a process's identity and
  signalling it. `pidfd_open`/`pidfd_send_signal` would close it; T13 claims only "re-read before
  signalling", which the code does.
- **F18** — `O_NOFOLLOW` does not stop `openat` descending into a bind mount;
  `openat2(RESOLVE_NO_XDEV)` would. T14 claims only that the walk cannot leave the tree *by name*,
  which holds.
- **F19** — `is_executable_file` tests `mode & 0o111` rather than `access(X_OK)`.
- **F20** — `FdPlan::normalise` opens `/dev/null` in a loop up to the target descriptor, so
  `9999>file` costs ten thousand opens. Self-inflicted.

### What the security review attacked and could not defeat (as of 2026-08-26)

Worth keeping, because a mitigation that survived a real attempt is the most useful line in a
security review — and because re-testing these later costs nothing if they are written down:

- **T1/T9 at the render boundary.** `Theme::paint` sanitises *before* choosing colour, so a pipe
  and a file are covered as well as a terminal; `View::Raw` re-sanitises; every cell, tree node
  and key goes through it; no setting disables it. `\n` (F4) was the only hole found.
- **T4, poisoned completion.** Candidates are filenames, never executed, and painted before
  display.
- **T7, decoder bombs.** JSON and YAML nesting refused past their depth limits at 200 and beyond;
  a 3^N YAML alias fan-out refused at N=8; the netlink decoders check every length against the
  remaining slice and advance by at least one aligned header per step. No overflow, no unbounded
  allocation, no non-terminating input found.
- **T13, identity completeness.** No path reaches a signal with a bare pid: every target carries
  `(pid, started)` from a record or from `providers.resolve()`, and a mismatch refuses.
- **T14, symlink swap.** Each directory is opened once relative to its parent's held descriptor
  with `O_NOFOLLOW`, and no path is ever re-resolved. Could not escape the tree by name.
- **T5/T6, refusal semantics.** A changed key is `remote.host_key_changed` carrying both
  fingerprints, with no continue-anyway; re-trusting is a separate deliberate act.
- **ADR-0019, no word splitting.** `has_pattern` is computed from the *source* characters, so a
  `*` arriving inside a variable's value cannot glob.
- **Environment propagation.** A child gets the session environment and nothing internal.
- **ADR-0007's `unsafe` audit.** Seven blocks, all in `ono-process`. The `pre_exec` path calls only
  `dup2`, `setsid`, `ioctl(TIOCSCTTY)` and `signal`; the one non-libc call,
  `io::Error::last_os_error()`, builds a non-allocating representation. No `format!`, no lock, no
  Rust I/O, no panicking index. No signal-mask inheritance across `exec` and no descriptor leak.

---

## Deferred / blocked

Work blocked on something outside this repository, and every `#[ignore]`d test: `cargo xtask
spec-check`'s unfinished-work scan refuses an `#[ignore]`d test that no entry here names, and
`cargo xtask state-check` refuses an entry that names no ADR. Work that is merely unfinished is an
issue. No test in the workspace is `#[ignore]`d.

- **The local rebuild comparison packages one binary twice.** ADR-0527. A second release *compile*
  needs a second target directory this machine cannot afford, so `rebuild-check.sh` locally proves
  the packaging layer is deterministic and not the compiler. In the release workflow the two builds
  are **two runners** per architecture, so there the binary is compiled twice and the comparison
  covers the whole chain — same script, both places. **Exit test:** a machine with the disk runs
  `rebuild-check.sh` over two independent compiles.

---

## History

The session records, the phase checklists of spec §37, the done log, the 2026-08-29 triage and the
2026-08-26 reviews left this board on 2026-09-13. Every item they still presented as open was
checked against the tree and the tracker first, and either found closed or carried into *Found,
not yet filed*. They remain readable in full:

```bash
git show 6485904d:docs/STATE.md    # the board as it stood before the cleanup
```

What they recorded lives on in the commits and ADRs they cite, in `docs/releases/` and
`docs/runs/`, in the milestones and closed issues, in `docs/ACCEPTANCE.md`, and, for the order the
shell was built in, in [`HISTORY.md`](../HISTORY.md).
