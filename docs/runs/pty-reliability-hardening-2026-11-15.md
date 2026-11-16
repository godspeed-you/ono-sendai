# Run report — PTY reliability hardening, 2026-11-15

An autonomous run the owner authorised on 2026-11-15. It had two goals: find and remove the
intermittent PTY failure that kept `scripts/release-check.sh` from completing for v0.6.5, and then
qualify `release-check` ten consecutive times on one committed SHA. The work runs from `abc4e314`
(v0.6.5 plus its post-tag documents) on `implementation`. Nothing was merged into `main`, nothing
was tagged and nothing was released. The tracking issue is #311.

Figures come from the run's own logs in `~/.cache/ono-pty/` (outside the repository), the gate and
CI logs, and commit timestamps. Anything not measured is marked **not measured**. Every run in a
repetition table is one execution of a test binary, timed with a monotonic clock, with the host
load recorded at its start (`~/.cache/ono-pty/bin/repeat.sh`).

## Summary

| | |
|---|---|
| Start | 2026-11-15 08:59 CET |
| Pause | 10:58–13:15 CET: the owner needed the machine; nothing ran |
| Interruption | 2026-11-15 19:48 – 2026-11-16 09:18: systemd-oomd ended the terminal scope during a cold build, and the host was rebooted (*Qualification*) |
| End | 2026-11-16 15:37 CET, the tenth consecutive `release-ready` |
| Qualified SHA | `8c916c03`: ten consecutive `release-check: the shell is release-ready`, no failure, no retry; CI green (run 43747427380) |
| Starting point | `abc4e314` on `implementation`, clean, equal to `origin/implementation`; `main` at `4bfbd890` (v0.6.5) |
| Root cause | the prompt read the spatial session state with `try_lock()` and dropped the place when another task held it; since v0.6.4 the read behind the prompt (ADR-0984) holds it at exactly that moment (#311) |
| Fixes | `c4ed3a3d` (#311, ADR-1032); `7d8c1b0a` (a fuzzy match ended `enter`'s sweep); `8bb2f748` (PTY harness waits); `71b1f2fb` (`change_gates.rs` fixture); `3c238222` (signal tests at default dispositions); `576ebc24` (the gate flushes its build before testing, ADR-1033); `298cec9b` (acceptance case 249) |
| Tests added | 5 unit tests; the PTY suites unchanged in what they assert |
| ADRs | ADR-1032, ADR-1033 |
| Subagents | one, an independent read-only review on Opus |
| Tokens | the review: 149,501; the main session's own usage: **not measured** |
| Disk budget | 300 GB for this run (owner, 2026-11-15); peak 118 GB above the starting point on `/` (*Resources*) |

## The failure

The v0.6.5 run record (`docs/runs/v0.6.5-2026-11-10.md`, *Qualification*) records six consecutive
`release-check` runs that stopped in `spatial_interactive` or `temporal_view`. It also records 4
failures in 20 runs of the `temporal_view` test at `v0.6.4`, 1 in 8 at the v0.6.5 release commit,
and 1 in 8 suite runs of `spatial_interactive` at `v0.6.4`. The board described the symptom as
"`enter compute` leaves the prompt at `~/work`". It had already ruled out load, process churn,
leftover processes and units, and a slower `enter`.

### Reproduction at `abc4e314` (load 1–2 on 8 cores)

| Selection | Runs | Failures |
|---|---|---|
| `temporal_view::should_enter_the_historical_place_and_keep_the_coordinate_when_enter_is_pressed_on_a_past_node` alone (`--exact`) | 20 | 0 |
| the whole `temporal_view` binary | 8 | 3 |
| the whole `spatial_interactive` binary | 8 | 0 |

Every failure had the same transcript. The prompt after the map closed was
`local://~/work git:implementation @09:06:46!  [PAST?] >`. The place segment was missing
entirely, so the prompt looked like the root place's. The board's "names the place before
`Enter`" was not what the transcripts show.

### Diagnosis

1. **A marker on the degraded path.** A diagnostic build (never committed;
   `~/.cache/ono-pty/diag-*.patch`) painted `local/DIAG-LOCKED` where `place_segment()`'s
   `try_lock()` failed. The next failing run read `local/DIAG-LOCKED://~/work …`. The prompt was
   drawn while another task held the spatial state. The place itself had never moved.
2. **Which holder.** A second diagnostic build numbered the background holders. 2 of 12 suite runs
   failed, both with holder 1: the first critical section of `spatial::view::discover`, the read
   behind the prompt that `repl.rs` spawns after **every** interactive command (ADR-0984). That
   section decides on a runtime worker whether the new place needs reading, at the moment the main
   thread draws the next prompt.
3. **Widening the window makes it deterministic.** With that critical section held 200 ms, the
   five named `spatial_interactive` tests and two more failed on every run (7 and 6 of 20 in two
   runs), and `temporal_view` failed 2 of 8 twice. Every failure carried `DIAG-LOCKED-1://~/work`.
4. **When it began.** Both background holders arrived with v0.6.4 (ADR-0982, ADR-0984;
   `7ca20366`, 2026-11-10), which is when the flake was first seen.

Why it was intermittent: the window is an in-memory critical section on another thread, a few
microseconds to a millisecond long. A whole PTY binary running its tests in parallel stretches it
enough to be hit. A single test rarely is.

Why nothing caught it: the prompt degraded silently, by design, to the v0.2 prompt. That is the
same text the root place shows, so no assertion inside the shell could tell. Only the PTY tests
that read the place off a prompt saw it, intermittently, and the board took it for load noise.

**The same mechanism elsewhere.** `place_is_storage()` gave `cd` the same degraded answer under
`spatial.follow_cwd = "storage-only"`, and a `cd` that met the read left the *place itself*
behind the working directory. The review found the prompt's `[PAST]` marker behind the same kind
of `try_lock` on the temporal state. It also found that the prompt's host came from a geography a
holder changes the moment it moves, which can pair a new host with the old place.

**Product, harness, environment.** The defect was in the product. The harness had a weakness of
its own: `spatial_interactive`'s `prompt()` took whatever had arrived 400 ms after a command's
output as the prompt. With the widened window that failed once in five runs even though the
prompt did arrive. No environmental cause was involved in #311.

## The correction

| Commit | Kind | What |
|---|---|---|
| `b2ca74ad` | docs | the claim; the board's PTY entry moved into #311 |
| `c4ed3a3d` | fix | `spatial_session()` hands out a guard that publishes a `Standing` (place path, storage flag, linked host) on release, under the lock; the prompt's place and host segments and `cd` read it; completion keeps a non-waiting borrow of the same guard; the `[PAST]` marker reads the coordinate that is already published (ADR-1032, closes #311) |
| `7d8c1b0a` | fix | a fuzzy match no longer ends `enter`'s sweep (below) |
| `8bb2f748` | test | the PTY suites wait for the prompt they read, within the existing budget; no assertion changed |
| `7d90f92f` | chore | metrics |
| `71b1f2fb` | test | `change_gates.rs` plans units with a unit file (below) |
| `91706f92` | docs | STATE, CONTRIBUTING, this record |
| `3c238222` | test | the tests that signal their shell start it at default signal dispositions (below) |
| `d0c41e91` | docs | STATE and this record |
| `576ebc24` | fix | the gate builds its tests, flushes the build's writes (`sync`), then runs them (ADR-1033) |
| `5a0d5a16` | docs | STATE and this record |
| `298cec9b` | test | acceptance case 249 names its events by identity once the answer gave it (below) |

Regression tests. Each of these failed before its fix, and each holds the contended state itself
while it reads:

- `crates/ono-cli/src/spatial/session.rs::tests::should_name_the_current_place_in_the_prompt_while_a_background_read_holds_the_session`
  (left: `None`, right: `Some("local/compute")` before the fix)
- `…::should_name_the_host_and_the_place_of_one_moment_in_the_prompt_while_a_holder_crosses_a_link`
- `crates/ono-cli/src/spatial/storage.rs::tests::should_know_the_place_is_in_storage_while_a_background_read_holds_the_session`
- `crates/ono-cli/src/temporal/prompt.rs::tests::should_keep_the_past_marker_in_the_prompt_while_another_task_holds_the_temporal_state`
- `crates/ono-cli/src/spatial/commands.rs::tests::should_enter_a_service_by_its_name_when_a_mount_is_named_like_it`

A control run of the first two without the concurrent holder passed on the old code. That
confirms contention alone caused the failure.

Behavioural consequences (ADR-1032): the prompt and `cd` never wait and never degrade. A prompt
drawn while a job's command still holds the state shows where the session stands until that
command lets it go. `cd` into storage waits for a job that holds the state, as every spatial
command does. Completion offers nothing on a Tab that meets a background registration, and the
next Tab asks again. No production path is special-cased for tests.

### Two more defects on the way

- **A fuzzy match ended the sweep (`7d8c1b0a`).** The first full `ono-cli` run after the fix failed
  `spatial_topology.rs::should_say_a_name_is_no_place_without_reading_every_service`. It failed on
  every run (10 of 10) and also at `abc4e314` (3 of 3), so it predates this run, and it would have
  kept the gate red. systemd 256 mounts each unit's credentials at
  `/run/credentials/<unit>.service`, and this host has two of them (journald, resolved).
  `resolved_place` sweeps the cheapest targets first and stopped at the first resolution that was
  not `NotFound`, so the mount's *fuzzy* match ended the search before the service manager was
  asked. §27.3 says a fuzzy match is never taken, so it is no answer either. The sweep now goes on
  while the resolution is `NotFound` or only fuzzy.
- **A fixture that read the live machine twice (`71b1f2fb`).** CI run 43695926135 on `7d90f92f` was
  red in four `change_gates.rs` tests. The fixture planned the first fifty units the runner listed,
  one of them the transient `collect-logs.scope`, which ended before the plan froze it. The fixture
  now plans fifty units with a unit file. Every assertion is unchanged.

- **Tests that inherited the runner's ignored signals (`3c238222`).** The second qualification
  attempt stopped in `jobs_shutdown::should_end_its_jobs_when_the_shells_terminal_hangs_up`: the
  shell did not leave on `SIGHUP` within 120 s. A watcher caught eighteen such shells in a stress
  run and dumped their threads. The termination thread waited in `anon_pipe_read`, so the
  handler had never run, and `SigIgn` was `0x1007`: `SIGHUP`, `SIGINT`, `SIGQUIT` and `SIGPIPE`
  ignored on entry. I had started the qualification driver with `setsid nohup … &`. `nohup`
  ignores `SIGHUP`, a script's background command ignores `SIGINT`, and a shell keeps a signal
  ignored that was ignored on entry (POSIX; ADR-0959, review B2), as
  `keeps_running_on_ignored` asserts. The shell was right and the tests depended on how their
  runner was started. Under a runner that ignores both, that test,
  `temporal_cancellation::should_leave_the_ledger_unlocked_and_intact_when_a_long_historical_query_is_interrupted`
  and `watch_live::should_release_the_query_task_promptly_when_a_live_map_is_cancelled` failed on
  every run. They now start the shell through `env --default-signal` (`ono_testkit::ono_command`),
  and pass under both runners. The qualification driver is now started with default dispositions
  too.

- **Tests that ran while the build was still being flushed (`576ebc24`, ADR-1033).** The third
  qualification attempt stopped in `authorized_clients.rs`: `add client-key`, which `fsync`s,
  took over 20 s. The sampler shows the dirty page cache at 6 GB, written at up to 128 MB/s in
  the gate's first minutes. The gate was recompiling the workspace because I had built
  `-p ono-cli` in between. That is the normal shape of every gate after a change, and the same
  mechanism as the `acquisition.rs` stops (*Found*): the kernel flushes the build behind it, and
  the first tests' writes queue behind the flush. The gate now builds its tests, runs `sync`, and
  then tests. Validation under the very condition: `cargo clean -p ono-cli` (20.5 GiB of
  artifacts), then the full gate of `576ebc24`. The build left 7.4 GB dirty at 14:01:31, the
  flush drained it by about 14:03, and the tests then ran with less than 1 GB dirty: **green**
  (14:00–14:12).

- **An acceptance case that reused short references (`298cec9b`).** CI on `5a0d5a16` (run
  43717871341) was red in one case, `249-correlation-is-not-causation`. Its later `why event @e81e`
  calls failed although the same reference had resolved four steps earlier. §11.6's references are
  the shortest unambiguous prefix plus one digit of headroom (ADR-0783), and every `ono -c` after
  the first records events of its own, so a prefix can come to name two events. The product is
  right to refuse it. The case now takes the full identity from the answer it already parses for
  the commands that are about the answer. No check changed; 5 of 5 container runs passed.

## Review

One independent read-only review on Opus. It read the diff and the code without building or
running anything, after the fix and before the harness and selector commits (149,501 tokens, 38
tool uses, 4 min 50 s). It found no critical or major defect.

| Finding | Severity | Resolution |
|---|---|---|
| `cd` into storage now waits for a holder (before, it answered "not storage") | minor | recorded in ADR-1032's consequences: the same as every spatial command, and nothing holds the state long behind an ordinary prompt |
| the prompt's host came from a geography a holder changes immediately, the place from the publication | minor | fixed in `c4ed3a3d`: `Standing` carries the host; test `…holder_crosses_a_link` |
| the `[PAST]` marker had the same `try_lock` | minor | fixed in `c4ed3a3d`; test `…keep_the_past_marker…` |
| `TURN` serialises only the tests that know about it | minor (test) | documented on `TURN` |
| fixed `settle` after `enter compute` in `temporal_view` | minor (test) | `8bb2f748`: waits for the prompt naming `local/compute`; the 3 s remain because the ledger must reach `-3s` |
| `screen()`'s "only for failure messages" was no longer true | nit | doc corrected |
| publishing while a panic unwinds | preference | not adopted: nothing in the publication panics, and skipping it would leave a move unpublished (ADR-1032) |
| the spatial and the temporal state taken in opposite orders | out of scope | `docs/STATE.md` → *Found* |
| publication order, init, bypass, deadlock, terminal effects, cost | checked | sound |

## Reliability

| What | Runs | Failures |
|---|---|---|
| **Historical** — `temporal_view` test at `v0.6.4` | 20 | 4 |
| **Historical** — `temporal_view` test at the v0.6.5 release commit | 8 | 1 |
| **Historical** — `spatial_interactive` suite at `v0.6.4` | 8 | 1 |
| **Historical** — full `release-check` on 2026-11-11 | 6 | 6 |
| **Before the fix** (`abc4e314`) — `temporal_view` test alone | 20 | 0 |
| **Before the fix** — `temporal_view` suite | 8 | 3 |
| **Before the fix** — `spatial_interactive` suite | 8 | 0 |
| **Before the fix, window widened to 200 ms** — `spatial_interactive` suite | 2 | 2 (7 and 6 tests each) |
| **Before the fix, window widened** — `temporal_view` suite | 2 | 2 |
| **Fix in the tree, window widened** (working tree over `b2ca74ad`) — `spatial_interactive` suite | 5 | 1 (the harness's 400 ms window, then fixed) |
| **Fix in the tree, window widened** — `temporal_view` suite | 5 | 0 |
| **Fix and harness, window widened** — `spatial_interactive` suite | 10 | 0 |
| **Fix and harness, window widened** — `temporal_view` suite | 10 | 0 |
| **After, `71b1f2fb`** — `temporal_view` test alone | 100 | 0 |
| **After, `71b1f2fb`** — `temporal_view` suite | 100 | 0 |
| **After, `71b1f2fb`** — `spatial_interactive` suite | 100 | 0 |
| **After, `71b1f2fb`, eight busy loops (load 8–13)** — `temporal_view` suite | 30 | 0 |
| **After, `71b1f2fb`, eight busy loops** — `spatial_interactive` suite | 30 | 0 |

The suite runs execute every test of the binary in parallel, the conditions under which the
failure was seen. The full `ono-cli` and workspace runs are the gates below and the qualification.

Gates on the way: the gate of `7d90f92f` stopped once in `acquisition.rs`. Six tests hit their
30 s watchdog at load 6.5, and the binary passed alone in 12 s and in both later gates (*Found*).
A second gate was stopped because a test build of mine ran beside it. The gate of `71b1f2fb` is
**green** (10:00–10:11). CI on `71b1f2fb` (run 43697064271) is **green** in all 18 jobs.

## Qualification

Six attempts ended before the final one, and each streak counts from zero:

| Attempt | SHA | Run | Result | Cause |
|---|---|---|---|---|
| 1 | `91706f92` | 1 | stopped in the gate (`acquisition.rs`, 3 watchdogs) | write-back throttling after an aborted start (*Found*) |
| 2 | `91706f92` | 1 | stopped in the gate (`jobs_shutdown.rs`, the `SIGHUP` test) | the runner's ignored signals, fixed in `3c238222` |
| 3 | `d0c41e91` | 1 | stopped in the gate (`authorized_clients.rs`, one watchdog) | the build's write-back under the first tests, fixed in `576ebc24` |
| 4 | `5a0d5a16` | 1 | **release-ready** (54 min) | — |
| 4 | `5a0d5a16` | 2 | stopped by me during the gate | CI on `5a0d5a16` was red in case 249, fixed in `298cec9b`; the final SHA had to carry the fix |
| 5 | `3974a5bf` | 1–8 | **release-ready** eight times (1924–2097 s) | — |
| 5 | `3974a5bf` | 9 | not started: the driver stopped because `implementation` moved | another session committed `8c916c03` (documentation and specifications only) during run 8; a qualified SHA that is not the branch's tip would not qualify the branch |
| 6 | `8c916c03` | 1 | lost during the cold build of a second worktree | systemd-oomd ended the whole terminal scope, the driver with it (2026-11-15 19:48); the owner rebooted the host the next morning. No test had run |

Attempts 6 and 7 ran in a second worktree (`implementation-qual` at `8c916c03`) so that the main
checkout stayed free. From attempt 7 on, every heavy job ran as its own systemd user unit with a
memory cap (`MemoryMax=40G`, no swap) instead of inside the terminal scope, and a warm-up gate in
that worktree (09:34–09:47, green) built it before the first counted run.

**Attempt 7: `scripts/release-check.sh` ran ten consecutive times on `8c916c03`, and each run
printed `release-check: the shell is release-ready` and exited 0.** The tree was clean and equal to
`origin/implementation` before every run, nothing changed between runs, the runs went one at a
time with nothing else heavy beside them, the driver ran with default signal dispositions, and a
sampler recorded dirty pages and write-back once a second throughout (20,416 samples).

| Run | Start (2026-11-16) | Duration | Exit | release-ready |
|---|---|---|---|---|
| 1 | 09:47:40 | 2848 s | 0 | yes |
| 2 | 10:35:08 | 2009 s | 0 | yes |
| 3 | 11:08:38 | 2005 s | 0 | yes |
| 4 | 11:42:03 | 2001 s | 0 | yes |
| 5 | 12:15:25 | 2011 s | 0 | yes |
| 6 | 12:48:57 | 2047 s | 0 | yes |
| 7 | 13:23:04 | 2027 s | 0 | yes |
| 8 | 13:56:52 | 2004 s | 0 | yes |
| 9 | 14:30:17 | 2017 s | 0 | yes |
| 10 | 15:03:54 | 2001 s | 0 | yes |

Ten runs took 20,970 s (5 h 50 min). Runs 2–10 took 2001–2047 s, 2014 s on average; run 1 was
longer because its acceptance image and the release build were cold in the worktree. The sampler
saw up to 7.7 GB of dirty pages and load 13 during builds, and at most seven `ono` processes in
`D` state at once; no watchdog fired. Every run's gate recompiles four crates (`tokio-rustls`,
`ono-remote`, `ono-cli`, `xtask`) that a later step of the previous run left with another
fingerprint; the cause was not investigated, and ADR-1033's flush covers the writes. The logs
(`run-1.log` … `run-10.log`, `summary.txt`, `io.samples`) are in `~/.cache/ono-pty/qual/`, and the
earlier attempts in `~/.cache/ono-pty/qual-attempt1` … `qual-attempt6-oom`.

**The qualified SHA is `8c916c03`.** This record and the board entry that cites it were committed
after the qualification, as documentation only, so `implementation`'s tip is a later commit than
the one qualified. That commit changes no code, test, script or contract; `scripts/gate.sh` ran
on its tree before the commit and was green (2026-11-16, 15:40–15:53), and CI runs on it after the push.

## Resources

| | |
|---|---|
| At the start | `/` 393 GB, 45 GB used, 331 GB free; no `target/` in the checkout; Docker empty (0 images, 0 build cache); `/tmp` 648 MB used of 31 GB |
| During the fix | `target/` 12–17 GB for `ono-cli` alone, 45 GB after the first workspace gate |
| During the qualification | `target/` 50 GB in the main checkout and 40 GB in the worktree; Docker 2.4 GB of images and 9.1 GB of build cache; lowest free space on `/` 230 GB, so at most 118 GB above the starting point |
| Worktrees | one beside the main checkout, `ono-sendai-qual` (the limit was two), removed after the qualification |

The temporary 300 GB budget applied to this run only.

## Remaining uncertainty

The mechanism is proven three ways: the marker on the degraded path, the holder's identity, and a
widened window that turns the failure deterministic before the fix and harmless after it. The
regression tests fail on the old code for exactly that reason. Repetition cannot prove a race
absent. What it shows here is 360 post-fix runs without a failure, under the conditions that
failed 3 in 8 before. The fix also removes the race in kind: the prompt no longer touches the
lock. The lock-order inversion is unfixed and on the board. Write-back from a concurrent writer outside
the gate can still slow a test; ADR-1033 removes the gate's own build from under its tests.
