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
by version, `v0.2.0` … `v0.6.4`.

**Push after every commit.** AGENTS.md §12.1 leaves `main` alone until the user asks for it, and
§12.2 asks that `implementation` be pushed freely so work is not lost; the branch and its phase
tags live on `origin`. Push `main` or open a pull request only when the user asks for it in that
request.

```bash
git push origin implementation && git push origin --tags
```

**The workspace declares `0.6.5`.** `v0.6.5`, KUANG/11 Ecosystem Stabilization over `v0.6.4`, is
**tagged, published (2026-11-11) and on `main`** (`4bfbd890`; release run 43067684227). Its note,
with the traceability of all 13 milestone issues, is `docs/releases/v0.6.5.md`, and its run record
`docs/runs/v0.6.5-2026-11-10.md`. The milestone is closed: all 13 issues met and closed — #224 in
all seven parts, the `subscribe` half of its sixth rejected by ADR-0588 and ADR-1013. `v0.6.4`,
System and Spatial Model Stabilization (`42a9875e`, 2026-11-10), keeps **#187** (selector-miss
latency, not met) open outside its milestone as debt. Every earlier release has its note beside it
in `docs/releases/`, and `gh release list` shows what is published.

**`implementation` past v0.6.3.** Thirty-one commits up to `cd125fd7` (2026-10-16), then the
documentation pass that brings the repository's documents to v0.6.3:

- `80499901` moves wasmtime to 48.0.6 and rustls to 0.23.46 for **sixteen RUSTSEC advisories**
  published after the tag (RUSTSEC-2026-0346 … -0349, -0352 … -0359, -0361 … -0364). v0.6.3 ships the
  affected 48.0.5 and 0.23.45, and `SECURITY.md` promises fixes in the latest 0.6 patch release only,
  so **a v0.6.4 release is urgent** for the advisories alone. The wasmtime step changes the compiled
  artifact key again (ADR-0916): every compiled KUANG/11 component needs `kuang-compile` once more after
  upgrading, as it did from v0.6.2 to v0.6.3.
- `5b920caf` moves wasmtime on to 48.0.8, tokio to 1.54.1, rustls-webpki to 0.103.16 and webpki-roots
  to 1.0.11 for **nine more advisories** (RUSTSEC-2026-0378, -0379, -0381, -0382, -0386, -0420,
  -0422 … -0424) that kept the scheduled audit on `main` red from 2026-10-20; the compiled artifact
  key moves with it, still one `kuang-compile` from v0.6.3.
- `6a9a163b` enables serde_json's `float_roundtrip` (ADR-0961): the scheduled fuzz campaign of
  2026-11-07 found a provider message whose number drifted by one ulp across a re-encode.
- The maintenance prelude #301–#303 (native background jobs and `fg`, a job's relative paths, a job's
  child ignoring `SIGTERM`), closed 2026-10-16 without a milestone, with ADR-0957 (a job's relative
  paths mean its start directory; a plan freezes absolute paths), ADR-0958 (every backgrounded line is
  an evaluator job; `fg` changes the attachment, not the result) and ADR-0959 (a leaving shell ends
  its jobs within one bounded grace). `docs/MIGRATION.md` §14 states what they change for v0.6.4.
- The milestone `v0.6.4` itself, from `41cc0f31` on the sub-branch `implementation-v064`
  (2026-11-09 … 2026-11-10), merged back into `implementation`: ADR-0962 … ADR-0987, acceptance
  cases 130–139, 141–144 and 395.

---

## The specification set

`docs/specs/ono_sendai_shell_spec_v0.2.md` is the **base**, and ten enhancements are layered on it —
v0.3, v0.4, v0.4.1, v0.5, v0.6, v0.6.1, v0.7, v0.8, v0.9 and v0.10, enumerated in AGENTS.md §5.2. All
eleven are immutable (AGENTS.md §5.2, ADR-0026). `spec-check` fails if one is missing its checksum
line in `docs/specs/spec.sha256` or if `AGENTS.md` does not enumerate an enhancement by name.

**Build order for what remains: v0.6.6, then v0.7, v0.8, v0.9 and v0.10.**
v0.4.1, v0.5, v0.6, v0.6.1, v0.6.2, v0.6.3 and v0.6.4 are implemented and released (`v0.4.1` …
`v0.6.4`); v0.6.5 is implemented and in release qualification. Of three stabilization tranches one
remains, each a GitHub milestone that is its authoritative inventory, as v0.6.2's and v0.6.3's were
(ADR-0862, ADR-0929): `v0.6.4` — System and Spatial Model Stabilization (providers, adapters and
process execution; spatial navigation and the map), `v0.6.5` — KUANG/11 Ecosystem Stabilization
(plugins, contributed providers, plugin and provider conformance), and `v0.6.6` — Remote and
Change Stabilization (remote links and trust; the temporal ledger and change planning). The
presentation releases follow them: each of v0.7 … v0.10's own §0.1 progression diagram names the
one before it as its prerequisite, so for those four arrival order and build order coincide.

**v0.7 arrived on `main` on 2026-09-01** as
`docs/specs/ono_sendai_shell_spec_v0.7_presentation_consolidation_rich_tty.md` — Presentation
Consolidation & Rich TTY Interface, 2 616 lines. It is a consolidation release, not a new
surface: one deterministic policy for resolving the existing v0.2 render hints, presentation
profiles and constrained view tree into a production-quality rich terminal path, with
`HistoryEntry`/`ResultRef` and the v0.4–v0.6 context surfaced consistently near the prompt. Its
own §0.5 states it exists so a later Deck workspace has something solid to compose, and must
stay valuable even if that workspace is never built. Merged into `implementation`, checksummed
(`docs/specs/spec.sha256`) and enumerated (AGENTS.md §5/§5.2), **not implemented**, and **the first
presentation tranche**, after the v0.6.4–v0.6.6 stabilization milestones. It has no `docs/ACCEPTANCE.md` checklist yet; writing one is its first task, and it
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


**v0.10 arrived on `main` on 2026-09-15** as
`docs/specs/ono_sendai_shell_spec_v0.10_native_ai_assistance.md` — Native AI Assistance &
Governed Model Interaction, 4 028 lines. It makes AI part of the one language: provider-neutral in
Ono and provider-specific in KUANG/11 packages, with the model reasoning and Ono observing,
validating and acting, data crossing a model boundary only under context and egress policy, and
agency expressed as capability, scope and plan. Its own §0.6 places it after v0.7–v0.9. Merged,
checksummed and enumerated, **not implemented**, and **behind v0.9**; its tracker milestone is
`v0.10.0`.
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

**`v0.6.6` — Remote and Change Stabilization — is the next tranche**, then v0.7 (above); `v0.6.5`
— KUANG/11 Ecosystem Stabilization — is implemented, every one of its 13 issues met, and waits for
its tag. The known problems are in the tracker, and most of them by release milestone (`v0.6.6` …
`v0.10.0`). What v0.6.5 found outside its issues is under *Found, not yet filed* → *Found during
the v0.6.5 run*. What v0.6.4 left: **#187** (a selector miss at Profile M is 577 ms p95 against
250 ms; the sweep still enumerates every process's descriptors to find socket owners) has no
milestone and waits for triage; so do **#306** ("unusually high traffic" needs a provider-stated
rate) and **#307** (`high_memory` against a cgroup's memory limit), split out of #209;
**#308–#310** (live link latency, two-sided cross-host correlation, a second hop on `map links`),
split out of #213, are in `v0.6.6`. `enter container` as an execution context is rejected for
good by ADR-0114 and ADR-0968 (#221). `v0.6.2` — the test suite, the harness, CI, packaging,
release tooling and binary size — `v0.6.3` — command contracts, errors and diagnostics, the shell
language, pipelines and completion — and `v0.6.4` — providers, adapters, process execution,
spatial navigation and the map — are implemented and released.

**The performance baseline was measured again for v0.6.4** (ADR-0989): every §33.2 target holds,
and every moved row says why. Two rows are worth a look at the next re-measurement:
`temporal.retention_sweep`'s p95 is one slow iteration of twenty, and
`spatial.selector_hit_by_sweep` M (188 → 236 ms) is not attributed.

**Unassigned until the user triages them** (AGENTS.md §9): fourteen open issues carry no milestone —
#282–#289, #293, #294, #297–#299 and #305, the v0.6.3 run's findings filed on 2026-10-08 — and the
closed prelude issues #301–#303 carry none either. Promoting `implementation` to `main` is the user's
decision; an agent carries it out only when told to, in that request (AGENTS.md §12.1).

## Done

The tracker and `git log` are the record of finished work (*History*, below); this section names
only what finished since the last release and has no issue of its own.

- [x] The repository's documents brought to the released v0.6.3 (documentation audit of 2026-10-16):
  this board, README, HISTORY, AGENTS, CLAUDE, CONTRIBUTING, SECURITY, ACCEPTANCE, the runbook,
  MIGRATION (with a §14 draft for the next release), the temporal guide, the fuzz and demo READMEs,
  five ADR status lines, the contract sources of the generated reference, and the generator itself
  (`docs/reference/language.md`, ADR-0960) — branch `implementation-docs-063`.

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

**Found during the v0.6.5 run (2026-11-10 …).** Outside the milestone's issues; each checked
against the tracker (no existing issue):

- A plugin instance that crashes with a view open never closes that view through `close_view`:
  `fail_instance` → `close_everything` (`crates/ono-kuang-supervisor/src/supervisor.rs`) answers the
  pending invocations directly, and `open_views` is dropped without an unmount, so whether the
  terminal is restored depends on the mounted view's `Drop` in ono-cli. Pre-existing (before
  ADR-0991). Reproduce: the example plugin's `browse` at a terminal, then `--misbehave=die`.
  Severity low–medium. Exit: `close_everything` closes every open view, with a conformance case
  that kills an instance while a view is open.
- The `working_directory` row of a confinement report can read `applied` for a directory the child
  never entered: the parent records it after `create_dir_all`, the `chdir` happens in the child and
  can fail (case 145 makes it fail). v0.4.1 §2.6 honesty. Severity low. Exit: in the spawn-error
  path the row is not `applied` when the error is the `chdir`'s, or the child records the row.
- The Kubernetes provider's own tests and examples refuse since ADR-1001 (cross-repository, review
  finding F8): `ono-sendai-kubernetes` at 604ee08 runs `remove k8s-resource … --dry_run false`
  without `--confirm` in `crates/ono-kubernetes-plugin/tests/live_cluster.rs:782` and `:918`, and
  declares the example `remove k8s-resource --context prod --kind ConfigMap --name stale` in
  `package/contributions/commands.yaml:206-207` and `contributions.rs:3038-3039`. Each is now
  `safety.confirmation_required`. Severity medium for the provider repository. Exit: the provider's
  examples and tests write `--confirm`; an issue in that repository.
- Every spatial read resolves every edge every package pushed (`relations.contribute`, ADR-1014),
  wanted or not and resolvable or not: `contributions::merge` runs `edge_between` → `place_of` twice
  per pushed edge on each `look`/`map`/`follow`, and `kuang_services::relations_query` takes a
  registry snapshot per pushed edge. An edge whose end never resolves stays until unload. Measured
  by the v0.6.5 re-review (F2): after `echo:relate --from $F --to 200000 --repeat 1000`,
  `enter process $F; look --json` took 4.3 s instead of 0.2 s. Severity minor (cost; the answers
  are right, ADR-1031 bounds the memory). Exit: pushed edges filtered by the relations wanted
  (ADR-1020) and an end that the provider answered "no such object" for dropped or indexed, so a
  default `look` with 1024 unresolvable pushed edges stays within the §33.2 budget.
- No command a user types routes a contributed object to `ProviderRegistry::act`: `set echo-place …`
  is dispatched straight to `command.invoke`, `ProviderMutation` is bound only for core command
  contracts, and the change planner plans only the core's operations (ADR-1012, *Consequences*).
  The provider path is implemented and tested at the registry (`plugin_provider.rs`), and nothing
  observes it from the shell. Severity low (a feature gap). Exit: a contributed action command
  routed through `ProviderMutation`, with an acceptance case.
- A job's pipelines empty the process-wide link table: `pipeline_context` calls
  `crate::spatial::links::publish(&rows)` in every session, a job's included (since a37b08a3), so
  a job running a pipeline replaces the table with `[]` until the foreground's next pipeline, and
  a disconnected link's `last_seen` is lost (v0.4 §35.2). Severity low–medium; needs `remote` links
  and a job. Exit: no global publish when `execution.background_job` is set; a test holding a link
  in the foreground while a job runs a pipeline, then `get link` still shows it with `last_seen`.
- A contributed *command's* answer is still collected whole before the rest of the line runs: a
  foreground `echo:count-forever | count` grows the shell's memory until interrupted (it can be
  interrupted since ADR-0998). Targets stream since ADR-0588 and ADR-1011; a command's answer
  streaming is a contribution-contract change. Severity low–medium. Exit: a command's stream
  consumed as a core producer's, with a bounded-memory test.
- `spatial/contributions.rs` asks a package for its edges (`asserted`) with no interrupt watch, so
  Ctrl-C during a `look` that waits on a slow relation command is not passed to the invocation.
  Severity low. Exit: the edge invocation is cancelled on interrupt, with a test against a slow
  relation fixture.
- Non-destructive capability families stay package-wide (ADR-1022, *Consequences*): a command that
  declares nothing (`echo:sneaky-clock`) reads the clock under a grant another command of the
  package needed. Only `destructive` families are bound to the declaring command. Severity low
  (K11P §6 reads both ways). Exit: a decision whether `mutate` families follow, as an ADR.
- `ono-kuang-testhost` does not check relation-shape attributes: it strips `; name=…; cost=…`, so a
  bad attribute passes the test host and fails at load. Severity low. Exit: the test host calls
  `relation::shape_attributes` and refuses what the shell refuses.
- A named contributed relation's far end is still `<id>.inverse`: §31.26 gives one name, and the
  shape syntax has no `inverse=`. Severity low (a feature gap). Exit: an `inverse=` attribute or an
  ADR declining it.
- `crates/ono-cli/tests/plugins.rs`'s `ono()` (and possibly other `Shell::new()` users) does not
  isolate `XDG_STATE_HOME`, so every run appends to the developer's real
  `~/.local/state/ono/kuang/audit.jsonl` (about 2,860 lines on 2026-11-10). Reproduce:
  `cargo test -p ono-cli --test plugins`, then `grep credential-plugin
  ~/.local/state/ono/kuang/audit.jsonl`. Severity low (test isolation, AGENTS.md §11). Exit: every
  plugin suite runs under a scratch state home.
- ADR-0162 says adding a field "is a schema version bump, which spec-check enforces"; spec-check
  only requires that refs and outputs resolve (ADR-0996 recorded the bump mechanics by hand).
  Severity low (a false claim in an ADR). Exit: spec-check enforces it, or an ADR corrects the
  sentence.
- PTY and wall-clock tests flake under load from parallel builds (load 14–29 on 8 cores):
  `spatial_interactive::should_preserve_the_current_place_when_the_terminal_is_resized_with_a_place_open`,
  `temporal_view::should_enter_the_historical_place_and_keep_the_coordinate_when_enter_is_pressed_on_a_past_node`,
  `jobs_shutdown::should_end_several_stubborn_jobs_within_one_grace_period_rather_than_one_each`,
  `jobs_shutdown::should_leave_promptly_when_its_jobs_end_on_the_signal`; each passed alone. Severity
  low (gate noise). Exit: each waits on what it asserts rather than on time. The `temporal_view`
  one fails alone at low load too, and before v0.6.5: 4 of 20 runs at `v0.6.4`, 1 of 8 at the
  v0.6.5 release commit (2026-11-11); when it fails, the prompt after the map closes still names
  the place the session stood at before `Enter`. `spatial_interactive` shows the same symptom —
  `enter compute` typed at the prompt leaves it at `~/work`
  (`should_return_to_the_previous_place_when_back_is_used_at_the_prompt_and_in_the_map`,
  `should_leave_the_terminal_in_order_for_an_external_program_after_the_map_closes`,
  `should_change_the_place_only_on_enter_when_focus_moves_inside_the_map`): rarely alone (1 of 4
  suite runs with `--all-features` at the release commit, 1 of 8 at `v0.6.4`), and in six
  consecutive full `release-check` runs of 2026-11-11 one of the two suites was red every time,
  also with two test threads. Not caused by load, process churn, leftover processes or units, or a
  slower `enter` (20 ms against 21 ms at v0.6.4). Severity medium: it keeps `release-check` from
  completing in one piece. Exit: find what an earlier suite of the full `ono-cli` run leaves behind
  that makes the next interactive `enter` lose its place, and a full run green ten times.
- The doc comment of `crates/ono-cli/src/plugins.rs::delivered` says the host learns of a handler's
  failure "from the invocation's result rather than from a stream event"; since ADR-0992 the stream
  carries it. Severity cosmetic.

**Found during the v0.6.4 run (2026-11-09 … 2026-11-10).** Outside the milestone's issues, or the
part of one that was left; each checked against the tracker (no existing issue). The release review
of 2026-11-10 classified every entry as safely deferred — an honest `unsupported`, a lower bound,
display text, a feature gap or developer tooling, none a wrong answer. It found one release-blocking
defect besides, fixed before the release: a journal `PRIORITY` written `03`, `+2` or `-1` was read
as a number and the priority handover made the answer differ (20444562, ADR-0988); the entry
"a journal `PRIORITY` outside 0–7 is accepted" was the same defect and is gone with it.

- A cgroup place's `processes` and `services` exits and a namespace place's `members` exit answer
  `unsupported`: `enter cgroup/<path>; look` — no provider joins them from that end (ADR-0971 names
  the forward direction only).
- The netlink targets (`address`, `route`, `neighbor`, `interface`) state no population, so an
  orientation read bounded below the collection counts what was read: `enter network; look` on a
  host with more routes than the orientation bound.
- `follow`'s `Ono-Sendai-E1009` help text carries a run of literal spaces ("It is
  classified …"): a string continuation lost its backslash in
  `crates/ono-cli/src/spatial/commands.rs` (the `PermissionState::Unknown` arm).
- A file whose owner uid has no account keeps `owner — available on request` after
  `follow owner --resolve`: nothing records that the account database was asked and named nobody
  (ADR-0973, *Consequences*).
- A user place's `files` exit reads `unsupported`, while the relation summary would call an
  expensive empty relation `available on request`; no provider enumerates a user's files.
- The journal adapter treats a legal journald entry without `PRIORITY` as an adapter defect
  (`Ono-Sendai-E0908`, exit 1), while `ono.log-record/1` documents `level` as "Null when the journal
  reported no priority". Found with #281; with a priority ceiling handed over the entry is left out
  instead (ADR-0980).
- `get journal` has no `--level` option; only `get log` has one.
- The shipped `ono --agent` announces no clock identity: `crates/ono-cli/src/main.rs` builds its
  `AgentConfig` without `with_clock(ono_remote::clock_identity(..))`, so `ono.link/1`'s `clock_id`,
  `clock_offset` and `clock_uncertainty` are null on every link (`link host x --transport local;
  get link | select clock_id`), although v0.5 §24.2 has the peer state it and the library tests
  cover it.
- `ono.log-record/1` contradicts itself: `level` is documented as "Null when the journal reported
  no priority" while `priority` is `required: true`, and an entry without `PRIORITY` is an E0908
  (the entry above). One of the two sentences is wrong.
- The command contract of `enter container` (`docs/contracts/commands/container.yaml`,
  `ono.container.enter`) names `provider_capability: container.exec`, although the command is a
  narrowing frame that executes nothing (ADR-0114, ADR-0968; #221 rejected the execution context).
  A listening agent therefore authorizes `enter container` as the execute action `container.exec`
  (`crates/ono-cli/src/main.rs`, `with_action_capability`) — the stricter direction, so no safety
  problem, but `help enter container` and `docs/reference/commands.md` name a capability the
  command does not use.
- The perf harness runs `ono` in the caller's real `XDG_*` environment
  (`xtask/src/perf.rs`, `sample`): a measurement reads the developer's configuration and may leave
  history there.
- A terminal that reports a size of 0×0 is taken at its word by the full-screen map: under
  `script --command ono out < fifo` the map draws about twenty columns by four rows and a search
  result is cut off, although `COLUMNS` and `LINES` are set; `stty rows 30 cols 100` typed into the
  session repairs it (cases 107 and 141 do that). `crates/ono-cli/src/spatial/interactive.rs:216`
  falls back to 80×24 only when the size cannot be read at all.

**Found during the maintenance prelude #301–#303 (2026-10-08 … 2026-10-16).** Outside those three
issues, so not fixed in them; each re-checked against the tracker (no existing issue):

- `bg %N` on a native job answers `resolve.target_not_found … there is no job %1`, although the job is listed. Found by the implementation agent.
- `tail file x --lines 5 | take 5` hangs on a two-line file: `follow` defaults to `true` (`docs/contracts/commands/file.yaml`), so the stream never ends — the hang is #283's case, the surprising default is not.
- `ono-process`'s `finish_unheard` (crates/ono-process/src/executor.rs) records a child the kernel no longer knows (ECHILD) as exit 0 — a status that is not known reads as success.
- Ctrl-Z does nothing to a native job under `fg`; Ctrl-C ends it. Job control for native jobs has no stop.
- A sealed opaque plan records no working directory: `plan --opaque rm a.o` in A, `cd B`, `apply` runs the program in B (foreground; a job runs it in the job's directory since ADR-0957).
- In the foreground, `remove file link/ --recursive` on a symlink to a directory empties the link's target and then reports "failed"; without `--recursive` on a symlink to a file it is refused (ENOTDIR).
- In the foreground, `install plugin ./pkg` records its source as the relative `path:./pkg` (a job records it absolute since ADR-0957), so a later reinstall from elsewhere resolves differently.
- `crates/ono-cli/tests/identity.rs::should_trace_a_user_to_its_processes_and_groups` needs fewer than ~255 root processes: `trace user root` fills its 256-node limit with processes before any group node. Reproduced 2026-10-08 while another project's 17-container stack ran (263 root processes). Same family as the `trace interface` entry below.
- `crates/ono-cli/tests/spatial_interactive.rs::should_repaint_a_focus_move_far_inside_the_frame_budget_when_the_map_is_open` failed once at load ~9 (other project's containers running); a wall-clock budget test, passed on a quiet host.
- A signal exit (SIGHUP, SIGTERM, non-interactive SIGINT) now ends the shell's jobs and recorded children (ADR-0959) but still skips the history flush, the audit write and the link hang-up, and a link's transport process (`--transport local` agent, `ssh`) is not ended by it. Before ADR-0959 the signal killed the shell outright, so nothing regressed; the cleanup is what is missing.

**Found during the v0.6.3 documentation pass (2026-10-16).** Checked with the release binary 0.6.3
and a build of `implementation` in a scratch `XDG_*` environment; neither is in the tracker:

- `to json` writes a record's extensions before its declared fields: `get process --tree | take 1 |
  to json` begins every record with `children`, then `pid`, `ppid`, … (0.6.3 and `cd125fd7` alike).
  The contract of `to` (`docs/contracts/commands/data.yaml`, its `note`) promises "a plain object of
  its fields (declared fields first, then the extensions a provider attached)". Either the serializer
  or the sentence is wrong; a script reading the first key sees an extension.
- A bare `cd /etc &` is no job: it runs in the foreground, moves the shell (`pwd` then prints `/etc`)
  and `jobs` lists nothing. ADR-0955's refusal fires only for a `cd` inside a backgrounded function
  or block; a backgrounded line whose only stage is `cd` (or `enter <dir>`) never reaches a job's
  evaluator. Reproduce: a script `cd /etc &` / `pwd` / `jobs`. Same on 0.6.3 and `cd125fd7`
  (ADR-0957–0959 do not touch it). Found by the Wiki agent in a PTY session, re-run here as a script.
- After `fn f() { sleep 2 }; f &`, a bare `fg` answers `Ono-Sendai-E0102 resolve.target_not_found
  there are no jobs` while `jobs` lists `[%1] running f`; `fg %1` works. Reproduce: a script
  `fn f() { sleep 2 }` / `f &` / `fg` / `jobs`. Same on 0.6.3 and `cd125fd7`. The current-job
  default of a bare `fg` finds no job here; the `bg %N` entry above may share the cause.
  Found by the Wiki agent, re-run here.
- The refusal of objects reaching a program, `Ono-Sendai-E0201 … produces objects, and the next stage
  is a program that reads bytes`, still says ``choose the representation: `… | to json | …` ``, also
  for a live stream (`watch process | cat`, `tail file x --follow | cat`), where `to json` never
  writes anything; ADR-0954 made `to jsonl` the answer there. The help is code, in four places:
  `crates/ono-cli/src/eval/native/{mod.rs:449,external.rs:189,foreground.rs:126}` and
  `crates/ono-command/src/impls/mutate.rs:550`. Same on 0.6.3 and `cd125fd7`.
- Two entries of the 2026-10-05 audit below are unchanged in the released 0.6.3 and are the ones the
  documentation now works around rather than repeats: `changes --since 3d` with the recorder off
  still prints nothing and exits 0 while `at -3d` refuses with E1302 (`docs/guides/temporal.md` now
  says so and points here), and the bootstrap catalog still pins the Kubernetes provider at `v0.2.0`
  (`docs/contracts/kuang/bootstrap-catalog.yaml:31`), which is why the README's `install plugin
  kubernetes` transcript still shows 0.2.0; the transcript changes with the pin.

**Filed on 2026-10-08.** The thirty problems the v0.6.3 run recorded here (2026-10-07) were
triaged against the tracker at `v0.6.3` = `b55ff186`, each re-checked: twenty-six went in as the
twenty-four issues #282–#305 (#293 and #297 carry two entries each), one was already #182, and three
needed no issue of their own — one was fixed in v0.6.3, one is contract behaviour whose consequence
is #283, and one was not a defect as recorded (its residue is evidence on #228). #282 also carries the
v0.6.2 run's `help | head -3` entry, which left the section below with it. Nothing of the v0.6.3 run is
on this board any more; the record of where each went:

- Remote refusal metadata lost on the wire → #182 (existing, evidence added) · unary minus on `null` → #284 ·
  `collapse` dropping a word value → #285 · `--print-peer-key` exiting 101 on a closed stdout → fixed in v0.6.3
  by 613b9054, no issue · per-crate `explains` check → #286 · collecting serializers and `from` on unbounded
  input → #283 (follow-up to #68) · native background jobs and `fg` → #301 · the editor's 2 s wait → not a
  defect as recorded: one bounded cursor-position query, paid once per session and only on terminals that
  never answer; evidence on #228 · `explain` `streaming no` beside `execution streaming` → #287 ·
  `from jsonl` → #288 · `help` producing no value → #289 · seven `enter <target>` commands → #290 (v0.6.4) ·
  `plan update package` → #291 (v0.6.6) · ADR-0813's `copy file … to …` → #292 (v0.6.6) · `join (get socket)`
  illustrations and the "Sixty-five" count → #293 · unused `materialize` helpers → #294 · conformance skipping
  `conditional` commands → #295 (v0.6.4) · global builds in `trace` and the spatial queries → #296 (v0.6.4) ·
  completion tests on shared cache and host packages (two entries) → #297 · stdout `println!` exits 101 → #282 ·
  `explain` passes after `&&`/`||` → #298 · 52 `may_be_empty` examples → #299 · a job's relative paths and a
  later `cd` → #302 · live-view failures held unbounded → #305 · `tail file` following by default → contract
  behaviour (`follow: default true`); its hang is #283's case, no issue · a job child ignoring SIGTERM on exit →
  #303 · `change_gates.rs` and docker units → #300 (v0.6.6) · a job's own plugin instance → #304 (v0.6.5).

**Found during the v0.6.2 run (2026-09-24).** Outside the milestone, so not fixed in it (v0.6.1 §24):

- `cargo test -p ono-cli --test plugins` fails 19/26 unless `kuang-example-plugin` and `kuang-compile` are already in target/debug (`cargo build -p ono-kuang-sdk`); the gate builds `--workspace --bins` first, a narrow run does not know it. Found 2026-09-24.
- Possible ETXTBSY in the product: `crates/ono-cli/src/kuang_host.rs:2630` copies plugin files with `std::fs::copy` inside a multi-threaded ono that may later exec them (ADR-0891's mechanism). Not reproduced. Found 2026-09-24.
- Journal follower may survive Ctrl-C under load: one `/bin/sh …/journalctl --output=json --no-pager --follow` shim outlived an adapters.rs run at load ~40; not reproduced in 55 runs with the new death assertion (#162). Hypothesis: the stream is not always dropped before the session ends. Found 2026-09-24.
- Stale processes in a session's maps: within one `ono` run, later maps keep an ended and reaped process with its old state and freshness `polled`. Reproduce: child `sleep 2` of the caller; `enter <caller pid>`; `map --json --all --type Process`; `sleep 4`; `map --json --all`; `sleep 3`; `map --json --all` — the dead pid in all three. Found by the determinism agent 2026-09-24.
- `get socket | take 1` does not stream its first row: it costs what `get socket | count` costs (35–38 ms of case 152's 50 ms budget in the container). Found 2026-09-24.
- Profile L live map spends most of its 5.7 s CPU projecting every socket (`Projection::project_as`, `ProviderBridge::absorb`) — v0.4 §34.4 incremental-neighbourhood work. Found 2026-09-24.
- `xtask perf --compare` can never report a row as held: it compares with `Tolerance::Absolute`, which ADR-0489 reads as "the baseline figure itself", so the same binary measured again minutes after `--write-baseline`, on the same quiet machine (load 2.2), read `perf: 0 held, 8 regressed` on noise of 0.2–2 % (`spatial.look` cold first 20.99 → 21.45 ms, `completion.first_candidate` 8.61 → 8.64 ms, peak RSS +0.2 %). v0.4.1 §32.4's "stable absolute targets" plausibly means the §34 budgets, not the baseline's figures. What closes it: an ADR that gives release qualification a tolerance the reference machine's own run-to-run spread fits in (measured, per metric), or that compares against the §34 targets. The baseline itself was re-measured for 0.6.2 (ADR-0926). Found 2026-09-24.
- `trace interface` orders an interface's bound sockets by inode ascending (`crates/ono-graph/src/kernel/network.rs` ~269), so on a busy host the newest sockets — usually the ones a user just opened — are the ones the 256-node limit leaves out, and the truncation record names only counts. `network.rs::should_include_the_listening_socket_when_tracing_its_interface` therefore skips on such hosts (declared, reported by `skip-check` since ADR-0885). Ranking listeners or recency first, or listing the unexpanded objects, would close it. Found 2026-09-24.
- The PTY completion tests with a 150 ms budget (`completion.rs::should_offer_this_machines_users_when_completing_a_user_selector`) fail when the whole core suite runs in parallel at load ~35 on 8 cores and pass alone — beyond the load the gate is trusted at (ADR-0881/0883), recorded so the next failure there is recognised. Found 2026-09-24.
- Case 077 (`077-adapters-systemd-streams.case`) missed `streamed=early` once in CI (run 36048615701, first attempt, group `core`, 2026-09-24): `journalctl -n 2 | take 1` returned after the shim's 5 s sleep (`elapsed=5`) instead of at the first record, although the product in that image was identical to the run before, where it passed. The case had passed in the 20 CI runs before; the re-run passed; locally 15/15 quiet and 15/15 at load 16. `run_streamed_segment` (`crates/ono-cli/src/eval/native/external.rs`) tells the child's group to stop when the reader is still running after the consumer ended; which of "the first record arrived late" and "the stop did not end the child" happened is not known. What closes it: capture the case's timing of first record and child exit when it recurs (the case prints only the total). Found 2026-09-24.

**Found during the documentation audit (2026-10-05).** Product behaviour the README, `docs/` and
the Wiki were checked against with the release binary of `57de4ff6`:

- `changes --since 3d` with the recorder off (store `none — session only`) prints nothing and exits 0, so an interval nobody recorded reads as "nothing changed". `docs/guides/temporal.md` ("The answer is bounded by retention") promises `temporal.not_recorded` there, and `at -3d` in the same session does refuse with E1302; with the recorder started, `changes --since 3d` refuses too. Case 244 covers only `at`. Found 2026-10-05.
- `sort < file` and `exec:sort < file` both fail `E0001 a stage whose arguments are expressions cannot be redirected (line 1, column 11)`, while `cat < file`, `exec:cat < file`, `exec:sort file` and `cat file | sort` work: a redirect after `sort` is parsed in expression mode, with or without the `exec:` prefix. The Wiki's Running-Unix-Programs page showed the form as working. Found 2026-10-05.
- `link host loop --transport local; get link | to json` lists `job`, `link`, `host`, `host-key`, `client-key`, `package` and `package-source` twice in `targets`. Found 2026-10-05.
- `explain leave` resolves `leave` as "not a native command" with `input bytes` and the note "`leave` has no target ``", although `leave` runs as the native `ono.context.leave`. Found 2026-10-05.
- `ono --help` does not list `--agent`, which the README's quick start and `docs/reference/remote-trust.md` use. Found 2026-10-05.
- `scripts/package.sh` builds packages in `rust:1.94-slim-bookworm` (`BUILD_IMAGE`, and the comment calling it the acceptance image's base) while `rust-toolchain.toml` and `docker/Dockerfile` are on 1.95 since ADR-0928. Found 2026-10-05.
- `docs/contracts/kuang/bootstrap-catalog.yaml` pins the Kubernetes provider at `v0.2.0`; the provider has released up to `v0.2.3` (2026-09-08), whose packages the README's system-package example installs. Found 2026-10-05.

**Found while bringing the Wiki to 0.6.2 (2026-10-05).** Reproduced by the agents that rewrote the pages, with the release binary of `57de4ff6` in a scratch `XDG_*` environment; each is named on the Wiki's What-Is-Not-Built-Yet page:

- Recovering a `remove file` plan on scratch files (`plan remove file <f>`, `apply`, `recover`) asks for `--accept-newer-state-loss` and lists the removed file itself as state that would be discarded (E1809). Found 2026-10-05.
- `resume plan <id> --confirm` on a sealed revision that was never applied executed it, as `apply` would. Whether `resume` is meant to start an unapplied plan is undecided. Found 2026-10-05.
- `why event @e…` answered once, and later refused the same reference on the same store with E1302 ("no source was asked"). Found 2026-10-05.
- `timeline --since 5m` draws one gap two or three times, shows `evidence: ono, ono`, and `timeline service <name>` does not filter by subject. A second `start recorder` returns no E1314. `get recorder` truncates the store path (`…/ledge`). Found 2026-10-05.
- `get config --profile` ignores a `change.profile` set in the session. A configuration parse error prints `Ono-Sendai-E0001 expected …` without the error name. Found 2026-10-05.
- `resolve command raw` and `resolve command adapt` answer E0101, though both are commands. Found 2026-10-05.
- `find plugin --source local` finds nothing in `ONO_PLUGIN_SOURCES`, and `path:~/…` is not expanded. Found 2026-10-05.
- Adapters: `ip -4 route` / `ip -4 address` are rewritten to `ip -j route show -4`, which `ip` refuses (`raw ip -4 route` works); the `git log` adapter starts every hash after the first with `\u{a}`; E0911's help after `raw df -h | take 3` suggests `raw raw df -h` and `get command <tool>`, which returns no rows. Found 2026-10-05.
- `trace connection --remote <ip>` with no matching connection refuses with "no connection answers to ``", an empty name. Found 2026-10-05.

**Filed on 2026-09-15.** Every problem this section held went to the tracker as #137–#224, in nine
milestones cut by subsystem — the subsystem whose code a fix changes: *Shell language, pipelines
and completion*, *Command contracts, errors and diagnostics*, *Providers, adapters and process
execution*, *Spatial navigation and the map*, *Temporal ledger and change planning*, *Remote links
and trust*, *KUANG/11 plugins and contributed providers*, *Test suite and acceptance harness* and
*CI, packaging and release tooling*. The two notes below are not problems and were not filed.

**`Interest::wants` scans the relation table once per provider per observation (2026-09-02).**
Thirty-odd rows, once per `look` — negligible today, indexable if the table grows. ADR-0495.
**Exit test:** none needed until the table does grow; recorded so the next reader knows it was seen.

**The gate now needs `cargo-deny@0.20.2` installed (2026-09-02).** `scripts/gate.sh` runs
`cargo deny --locked --all-features check` and exits 127 with the install command when the tool is
absent, the same shape `cargo-deb` already uses (ADR-0121). Any machine that runs the gate needs
`cargo install --locked cargo-deny@0.20.2`. This is a note rather than a defect; it is here so the
next person to meet exit code 127 finds the reason.

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
  exposed is a trust store in a directory another user can write. The trust store's writer is
  #195.
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
not yet filed*, and from there filed as issues on 2026-09-15. They remain readable in full:

```bash
git show 6485904d:docs/STATE.md    # the board as it stood before the cleanup
```

What they recorded lives on in the commits and ADRs they cite, in `docs/releases/` and
`docs/runs/`, in the milestones and closed issues, in `docs/ACCEPTANCE.md`, and, for the order the
shell was built in, in [`HISTORY.md`](../HISTORY.md).
