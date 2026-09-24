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

**The workspace declares `0.6.2`.** `v0.6.2`, the Verification Foundation over `v0.6.1`, is the
GitHub milestone `v0.6.2` implemented on `implementation` (ADR-0862: the milestone is the release
inventory and each issue its requirement). Its note, with the traceability of all 24 issues, is
`docs/releases/v0.6.2.md`, and its run record `docs/runs/v0.6.2-2026-09-24.md`. `v0.6.1` (2026-09-12)
and every earlier release are tagged, published and on `main`; `gh release list` shows what is
published.

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

**v0.7 is the next tranche** (above), and nothing of it is started. The known problems are in the
tracker, by release milestone (`v0.6.3` … `v0.10.0`); `v0.6.2` — the test suite, the harness, CI,
packaging, release tooling and binary size, including #124–#127 — is implemented. Promoting `implementation` to `main` is the user's
decision; an agent carries it out only when told to, in that request (AGENTS.md §12.1).

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

**Found during the v0.6.2 run (2026-09-24).** Outside the milestone, so not fixed in it (v0.6.1 §24):

- `ono -c 'help' | head -3` panics "failed printing to stdout: Broken pipe", exit 101 (full and core; `| head -30` fine). Likely `println!` in the help builtin (crates/ono-cli/src/builtin.rs). Found by core-build agent 2026-09-24.
- `cargo test -p ono-cli --test plugins` fails 19/26 unless `kuang-example-plugin` and `kuang-compile` are already in target/debug (`cargo build -p ono-kuang-sdk`); the gate builds `--workspace --bins` first, a narrow run does not know it. Found 2026-09-24.
- Possible ETXTBSY in the product: `crates/ono-cli/src/kuang_host.rs:2630` copies plugin files with `std::fs::copy` inside a multi-threaded ono that may later exec them (ADR-0891's mechanism). Not reproduced. Found 2026-09-24.
- Journal follower may survive Ctrl-C under load: one `/bin/sh …/journalctl --output=json --no-pager --follow` shim outlived an adapters.rs run at load ~40; not reproduced in 55 runs with the new death assertion (#162). Hypothesis: the stream is not always dropped before the session ends. Found 2026-09-24.
- Stale processes in a session's maps: within one `ono` run, later maps keep an ended and reaped process with its old state and freshness `polled`. Reproduce: child `sleep 2` of the caller; `enter <caller pid>`; `map --json --all --type Process`; `sleep 4`; `map --json --all`; `sleep 3`; `map --json --all` — the dead pid in all three. Found by the determinism agent 2026-09-24.
- Esc unanswered after a resize in a paused live map: `spatial_interactive.rs::should_show_the_paused_marker_and_keep_its_instant_when_the_terminal_is_resized` fails 1/10 at load 10–15 and 2/8 at +32 busy loops, always as Esc unanswered for 45 s after the resized paused frame. Suspect `ready_key`/`read_event_timeout` around the resize projection; unproven. Found 2026-09-24.
- `get socket | take 1` does not stream its first row: it costs what `get socket | count` costs (35–38 ms of case 152's 50 ms budget in the container). Found 2026-09-24.
- Profile L live map spends most of its 5.7 s CPU projecting every socket (`Projection::project_as`, `ProviderBridge::absorb`) — v0.4 §34.4 incremental-neighbourhood work. Found 2026-09-24.
- Case 152: `stdout-contains: first-socket-row: within budget` also matches `baseline-first-socket-row: within budget` (pre-existing; still guarded by `stdout-not-contains: OVER BUDGET`).
- `xtask perf --profile S --compare docs/contracts/hardening/performance_baseline.json` reports `Regressed` on a quiet machine (load 2.6, below the baseline's 3.84 allowance) for six rows, and the same rows exceed the baseline with the pre-v0.6.2 release profile too (A/B on one tree, 2026-09-24: e.g. `spatial.selector_miss` first 605 ms old profile / 614 ms new vs 571 ms baseline, `completion.first_candidate` 8.6 ms vs 6.44, `spatial.look` cache-hit 3.07 ms vs 1.36, `process.enumeration` estimated_bytes 416k vs 340k — schema growth). The checked-in baseline predates the v0.5/v0.6 growth and no longer describes the tree; every §34 target still holds. What closes it: re-measure the baseline on the reference machine, or record per-row why each figure moved.
- A redirected `inspect plan <ref> --resolution` cuts each target's path at column 80 from the end side (`fit` in crates/ono-cli/src/change/render.rs), so a deep path shows as `…/second.con` and two targets in one deep directory can print identical headings. Deterministic (spec §4.6) but lossy; eliding the middle would keep the file name. Found 2026-09-24 when scratch paths moved into `target/tmp` (#143) and CI's checkout depth cut `second.conf`.

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
