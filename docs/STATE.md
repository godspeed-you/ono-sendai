# STATE

The shared work board. **Read it first, update it last, every session** (AGENTS.md section 9).

**The backlog is not here — it is the GitHub issue tracker** (ADR-0425). One problem is one
issue, and its evidence lives in the issue body: `gh issue list`. This file holds what a tracker
cannot — the claims in flight, the problems found and not yet filed, what is deferred and why,
the session records, the phase checklists and the history. The stopping rule lives in
`docs/ACCEPTANCE.md`: the run ends when `scripts/release-check.sh` passes, not when this file
looks tidy.

Working branch: **`implementation`** — never commit to `main` (AGENTS.md section 12.1)

**Commit every increment, and tag every completed phase.** A phase is done when its box in
`docs/ACCEPTANCE.md` section 4.1 is ticked; the commit that ticks it gets an annotated tag
`phase-<letter>` whose message names the exit criterion and the case that proves it. The tags are
how the state after each phase stays findable in a run of hundreds of commits:

```bash
git tag -n99 phase-a          # what Phase A delivered, and what proves it
git switch --detach phase-a   # the tree exactly as that phase left it
```

Tags so far: `phase-a` … `phase-j` (one per completed phase; H, I and J tagged at the release commit).

**Push after every commit.** AGENTS.md §12.1 keeps `main` untouched and §12.2 asks that
`implementation` be pushed freely so work is not lost; the branch and its phase tags live on
`origin`. Never push `main`, never open a pull request unless asked.

```bash
git push origin implementation && git push origin --tags
```

**`release-check: the shell is release-ready` — printed 2026-08-26 by `scripts/release-check.sh`
at commit 21b37d9, and again on 2026-08-29 by agent `S11c` at the end of the v0.4 tranche.** All
ten phases of spec §37 are complete, proven and tagged; all 143 boxes in docs/ACCEPTANCE.md §4 are
ticked by a named automated proof, and the 2026-08-29 reconciliation traced every one of the 168
test names and every acceptance case they cite to something that exists and runs in the gate. The
containerised suite stands at **96 cases** in `docker/acceptance/cases/` and the workspace at
~2 600 outcome tests across **30 crates**. What remains in the issue tracker is post-release
deepening — every item deliberate, none blocking the deliverable. Promoting `implementation` to `main` is the
user's decision and the user's action (AGENTS.md §12.1).

Phases A–D are complete and tagged. B/C/D landed as: native commands wired into the evaluator
(ADR-0028), partial failure semantics (ADR-0029), the §33.5 interop serialisation (ADR-0030),
path/string comparability (ADR-0031), the pre-flight field check (spec §11.3), shell stdin into
a parsing head (§12.4), unquoted `explain` over a pipeline, the provider registry
(docs/spec/providers/), and acceptance cases 040–044.

---

## The specification set

`docs/ono_sendai_shell_spec_v0.2.md` is the **base**. `docs/ono_sendai_shell_spec_v0.3_external_command_adapters.md`
is an **enhancement layered on it** — the External Command Adaptation Layer — and both are
immutable (AGENTS.md §5.2, ADR-0026). `spec-check` fails if either is missing a checksum line in
`docs/spec.sha256` or if `AGENTS.md` does not enumerate an enhancement by name.

**Build order for the enhancements now on `implementation`, next tranche first: v0.4.1, then
v0.5, then v0.6, then v0.7, then v0.8, then v0.9** — v0.4.1 because its own spec text (§0.1–§0.2,
below) says the v0.5/v0.6 feature work must inherit its hardened guarantees rather than re-solve
them; v0.6 because its own §0.1 places it behind v0.5's evidence and causality model, already
noted below; v0.7, v0.8 and v0.9 because each one's own §0.1 progression diagram names the
tranche directly before it as its prerequisite — v0.8 stacks on v0.7, v0.9 on v0.8 — so for these
three, unlike v0.4.1, arrival order and build order coincide.

**v0.4.1 arrived on `implementation` on 2026-09-01** as
`docs/ono_sendai_shell_spec_v0.4.1_hardening_trust_release_integrity.md` — Hardening, Trust &
Release Integrity, a maintenance layer over the already-implemented v0.4 substrate: security
boundaries, remote trust, KUANG/11 confinement, resource boundedness, streaming correctness,
performance stability, test truthfulness and release provenance (spec §0.1–§0.2). It is merged,
checksummed (`docs/spec.sha256`) and enumerated (AGENTS.md §5/§5.2), and **not implemented**.

The spec places itself explicitly: §0.1's layering diagram stacks it directly on v0.4 as the
"stable implementation base for the future v0.5 and v0.6 feature tranches", and §0.2 says later
feature work MUST inherit its hardened guarantees rather than re-solve them. It therefore
precedes v0.5 in build order despite arriving on disk after v0.6 — **v0.4.1 is the next tranche
to pick up**, ahead of the v0.5 work below. It has no `docs/ACCEPTANCE.md` checklist yet either;
writing one from spec §0.1–§0.2 and whichever sections carry its MUST/SHOULD boundaries, the way
§4.7 was written from v0.4, is that tranche's first task, and it claims **§4.8** — pushing v0.5's
future checklist to §4.9.

**v0.5 arrived on `main` on 2026-08-31** as `docs/ono_sendai_shell_spec_v0.5_temporal_causal_systems_interface.md`
— the Temporal & Causal Systems Interface, 4 147 lines: time as a session coordinate, a canonical
event model, an evidence ledger with explicit coverage and gaps, state reconstruction, `timeline`,
`changes`, `why`, historical spatial navigation, and the rule that correlation is never presented
as causation. It is merged into `implementation`, checksummed and enumerated, and **not
implemented**.

It arrived named `ono_sendai_spec_v0.5_…`, without the `shell_spec` infix v0.2–v0.4 carry, and
the two guards that were supposed to notice an unguarded specification both matched on that infix
— so the gate stayed green with the document neither checksummed nor enumerated. ADR-0423 records
the widened rule; `xtask/tests/narrative.rs::should_find_an_enhancement_whose_name_omits_the_shell_infix`
and `xtask/tests/scan.rs::should_ignore_a_narrative_specification_whose_name_omits_the_shell_infix`
hold it. The user then renamed the file on `main` (`c4ca548`, content untouched) to carry the
infix after all, and `docs/spec.sha256` follows the new path. The widened rule stays: it is what
made the document visible in the first place, and the next one may arrive misnamed too. **The
v0.5 tranche has no `docs/ACCEPTANCE.md` checklist yet, and it is not the next tranche**: v0.4.1
(above) precedes it in build order and claims §4.8, so v0.5's checklist — written from v0.5 §48
and §56, the way §4.7 was written from v0.4 — becomes **§4.9** once its tranche starts.

**v0.6 arrived on `main` the same day** (`9c49bb9`) as
`docs/ono_sendai_shell_spec_v0.6_prospective_change_protection_recovery.md` — Prospective Change,
Protection & Recovery, 4 473 lines. It makes a proposed mutation a first-class object, the
`ChangePlan`, so an operator can inspect what a change would do and how recoverable it is before
it becomes real; v0.6 §0.1 places it after v0.5 in the progression and leaves the earlier
documents authoritative for what they define. Checksummed and enumerated, **not implemented**,
and **behind v0.5**: v0.6 reasons about consequences and recoverability, which is the evidence
and causality v0.5 builds. Nothing in it should be started before v0.5's §4.9 exists — itself
behind v0.4.1's §4.8 (above) in build order.

This one the gate caught by itself, on the first `spec-check` after the merge — the widened rule
of ADR-0423 working as intended, unlike the silent arrival of v0.5 the day before.

**v0.7 arrived on `main` on 2026-09-01** as
`docs/ono_sendai_shell_spec_v0.7_presentation_consolidation_rich_tty.md` — Presentation
Consolidation & Rich TTY Interface, 2 616 lines. It is a consolidation release, not a new
surface: one deterministic policy for resolving the existing v0.2 render hints, presentation
profiles and constrained view tree into a production-quality rich terminal path, with
`HistoryEntry`/`ResultRef` and the v0.4–v0.6 context surfaced consistently near the prompt. Its
own §0.5 states it exists so a later Deck workspace has something solid to compose, and must
stay valuable even if that workspace is never built. Merged into `implementation`, checksummed
(`docs/spec.sha256`) and enumerated (AGENTS.md §5/§5.2), **not implemented**, and **behind
v0.6** in build order.

**v0.8 arrived on `main` the same day** as
`docs/ono_sendai_shell_spec_v0.8_deck_workspace_composition.md` — Deck Workspace Composition &
Terminal Ownership, 2 897 lines. It adds a persistent Deck host that composes the views,
history, context and safety state v0.2–v0.7 already define around the shell editor, plus one
generic terminal-ownership contract shared by the Deck, existing full-screen Ono views and
foreground external programs; its own §0.1 places it directly on v0.7, and §0.6 bounds it
explicitly against becoming a second type system, history store, context model or window
manager. Merged, checksummed and enumerated, **not implemented**, and **behind v0.7**.

**v0.9 arrived on `main` the same day** as
`docs/ono_sendai_shell_spec_v0.9_live_view_integration.md` — Live View Integration &
Long-Running Workspace Ergonomics, 3 538 lines. Its own §0.2 explicitly rejects an earlier,
abandoned design direction for v0.9 (`Live<T>`, `StateObservation<T>`, a new watermark/
backpressure model) in favour of small, bounded, presentation-local bindings that keep
`Stream<T>`, `watch`, spatial `--live` and the v0.5 temporal cursor usable, honest and
responsive inside the v0.8 Deck over minutes or hours, without a second live-data model. Merged,
checksummed and enumerated, **not implemented**, and **behind v0.8** — the last tranche in the
current build order.

**The v0.3 tranche is complete** (started 2026-08-27, delivered by ADR-0052 … ADR-0067; all 39
boxes of `docs/ACCEPTANCE.md` §4.6 are ticked, cases `070`–`089`). **The v0.4 tranche is complete**
(delivered 2026-08-28/29, ADR-0124 … ADR-0212 and the S11c session below; §4.7's boxes are ticked,
cases `090`–`110`). What follows is the paragraph as it stood while v0.3 was running; it is kept
because ADR-0027's analysis and the build order below are still the map of that layer.
v0.2 was released as `v0.2.0` from
`main` at `273d3cd`; the External Command Adaptation Layer is implemented on `implementation`
in the same loop, against the same referee. Its definition of done is `docs/ACCEPTANCE.md`
§4.6 — 39 boxes derived from v0.3 §2.1–§2.6, §1.67 and §1.68 — and `scripts/release-check.sh`
is red until every one of them is ticked by a named automated proof. **ADR-0027 carries the
analysis**: what v0.3 requires, which five existing decisions grow (ADR-0006, -0011, -0013,
-0016, -0022), where v0.2 and v0.3 read differently, and the decomposition of the tranche. Read
ADR-0027 and §4.6 before picking a task; do not re-derive them from the 2182-line document.

Build order (v0.3 §1.69, one increment per line, each one RED-first):

1. `ADAPT-001` OutputDemand computed backwards from the consumer, reported by `explain`
2. `adapter.*` error family (E09xx) in `docs/spec/errors.yaml`
3. `ADAPT-003` guaranteed raw spelling
4. `ADAPT-009` manifest schema + `docs/spec/adapters/`, `spec-check` drift rules
5. `ADAPT-002` registry, identity pinning, negotiation states, conflict resolution
6. `ADAPT-004`/`005` plan execution through `ono-process`, streaming decoders, fuzz corpus
7. `ADAPT-007` provenance on adapted values, `inspect --provenance`
8. `ADAPT-006` version probe cache
9. `ADAPT-010` fixture harness generated from the contracts
10. Tier A tools: util-linux (`lsblk`/`findmnt`/`lsns`), `ip`, `journalctl`, `systemctl`
11. Tier B tools: `ps`, `stat`/`df`/`find`, `git`, `lsof`
12. Tier C tools: `ss`, `curl`
13. `ADAPT-008` KUANG/11 capability mapping, SDK, test host, packs and trust
14. `ADAPT-011` remote negotiation
15. integration surfaces: completion, history, script determinism, muscle-memory diff
16. release evidence: reference pages + compat matrix, overhead measurement, README

The container image gains the tools of step 10–12 when step 10 starts; a tool adapter is not
delivered until its live case runs there (`docs/ACCEPTANCE.md` §4.6.3).

---

## The v0.4.1 build order (2026-09-02)

The tranche's 101 issues carry GitHub milestones **H0 … H12**, one per phase of v0.4.1 spec §57,
which is normative: *"The implementation MUST be staged so safety-critical work lands before
broad refactoring … unless an ADR documents a dependency requiring a local swap."* The phase a
work package belongs to is stated in its issue body (`**Phase H4 · P0 · spec §16.1**`); the
milestone makes it queryable:

```bash
gh issue list --milestone "H1 — Direct remote mutual authentication"
gh api repos/godspeed-you/ono-sendai/milestones --jq '.[] | "\(.title)\t\(.open_issues)"'
```

Within a phase the order below is the intended one — it is a reading of the dependencies, and an
agent that finds a better one records why in the commit body.

**H0 · Baseline and guardrails** — #29 (ACCEPTANCE §4.8) first, because it defines what finished
means for the other hundred; then #31, the four failing proofs §57 requires before any fix lands;
then #30 (frozen baseline) and the two registries #117/#118, which #117 itself asks to land early
so later phases write policy data into them rather than into scattered constants.

**H1 · Mutual authentication** — #32 → #33 → #34 → #35 → #36 → #38 → #39 → #37. #18 (host-key
pinning is dead code on the only production transport) closes here: ADR-0274 records that its
exit test needs exactly the authenticated TCP transport H1 builds.

**H2 · Authorization** — #40 → #41 → #42 → #43 → #44 → #47 → #45 → #46 → #48 → #49 → #50. The
store precedes the commands, and the immutable `AuthorizationContext` (#47) precedes the
offer filter (#45) and dispatch defense in depth (#46), which both read it.

**H3 · Remote limits** — #54 first (one central `Limits` contract), then #51 → #52 → #53 → #57 →
#55 → #56. Starting with the semaphores would create three sources for the same numbers.

**H4 · KUANG fail-closed** — #58 → #59 → #60 → #61 → #62 → #63 → #64. Crate-disjoint from
H1–H3 (`ono-kuang` against `ono-remote`), so it can run beside them.

H1–H4 hold every P0. When they are green, spec §3.2's mandatory scope is met and §66.9's
"zero unresolved P0" is reachable.

**H5 · Budgets** — #65 → #66 → #67 → #68 → #70 → #71 → #72 → #73 → #74 → #69, with #120 behind
#73/#74. `explain` (#69) and `inspect limits` (#120) display finished values, so they come last.

**H6 · Streaming** — #78 first: the capture inventory says what #75 and #79 have to touch. Then
#75 → #76 → #77 → #79 → #81 → #80. H5 precedes H6 because a streamed `each` bounds itself
against a budget that must already exist.

**H7 · Spatial performance** — measurement before optimisation: #82 → #83 → #84 → #85, then the
pathologies #22 and #20 (`map --live`), then #86 → #25 → #87 → #8 → #21. #8 is a design question
(a persistent cross-process index against a bounded last step) and needs an ADR before code.
#21 needs the container-invocable measurement #84 delivers.

**H8 · Test truthfulness** — #88 → #89 → #90 → #91, then the three known flaky or self-satisfying
tests #27, #7 and #6, which are the local instance of §65.10's skip-as-pass, then #92 → #93 → #94.

**H9 · Structural refactor** — #95 → #96 → #97, strictly after H6: §65.12 forbids carrying a
refactor and a semantic redesign in one step, and H6 is what changes evaluator semantics.

**H10 → H11 → H12 · Supply chain, reproducibility, release proof** — #98 … #103, then #104 …
#110, then #111 … #116 with #119 as their gate. #119's refusal texts are written as the
boundaries appear in H2, H4 and H5; H12 proves they all name the deciding boundary.

### What carries no milestone

Twelve open issues sit outside the tranche. #10 and #11 (socket and filesystem identity) are worth
taking before v0.5, whose evidence ledger correlates on stable object identity. #12 (theme
markers) belongs to the v0.7 presentation tranche. #9, #15, #16, #17, #23, #24 and #26 are
independent class-b defects, schedulable whenever a phase leaves room. #3 and #5 are class-c and
are tranches of their own, competing with v0.5 rather than fitting inside v0.4.1.

---

## Product direction from the user (2026-08-26)

**"Es muss immer cool sein und Spaß machen, es zu benutzen. Es soll aufregend sein."** The shell
is the Ono-Sendai deck: correctness is the floor, not the ceiling. Where a decision is
otherwise free, prefer the option that feels alive — the prompt as a HUD, tables that update in
place, colour that means something, latency you never notice (spec §34's budgets are product
quality), and answers that invite the next question (`@2 | inspect`). Phase F's `watch` is the
showcase: a live view of the machine should feel like instrumentation, not like polling.

## In progress

- [agent-S12 | 2026-09-04] **The v0.4.1 checklist is reconciled with the tree and the tranche is
  brought to a release-ready state** — `docs/ACCEPTANCE.md` §4.8, `docs/STATE.md`,
  `xtask/tests/hardening_evidence.rs`, `xtask/tests/spatial_evidence.rs`,
  `xtask/tests/support/mod.rs`, `xtask/src/scan.rs`, `crates/ono-cli/src/spatial/`,
  `crates/ono-provider-systemd/`, `crates/ono-provider-net/`, `docs/releases/v0.4.1.md`.
  Five phases: the §4.8 harvester (ADR-0575), §4.8.2's names, the boxes whose proofs already exist,
  ADR-0496's bounded observation, and the release mechanics.

## What is left, and why

*Empty.* The v0.4.1 tranche is delivered — thirteen phases, and what remains is recorded below
rather than claimed here.

Two issues stay open because **no release has been signed**, and neither is work anybody can do at
a keyboard: **#107** (signature over the checksum manifest) and **#115** (documenting installer
verification). Keyless Sigstore needs an OIDC token that exists only inside a release run, and
§40.2 denies the acceptance container a network. The code is complete on both sides —
`scripts/sign-release.sh` refuses without an identity, `scripts/verify-release.sh` is
identity-constrained and fails closed — and the first `v*` tag is the run that proves them.
Pushing that tag is the user's action, as promoting `implementation` to `main` is (AGENTS.md
§12.1). Their two boxes in `docs/ACCEPTANCE.md` say so in their own text.

No class-c issue remains: **#3** closed with its sixth increment, below.

## What the tranche delivered

One entry per phase, newest first. The reasoning is kept because each entry records something the
issue that ordered the work did not know.


- **The two scheduled workflows had never run, and each died on its first attempt
  (2026-09-04).** `fuzz.yml` and `verification.yml` are scheduled on the default branch, and the
  default branch only carried them from the merge of 2026-09-03: their first runs were their
  first runs ever. Four causes, all in the workflows.

  **A toolchain file outranks an installed default.** Both workflows install nightly, and
  `rust-toolchain.toml` pins 1.94: every cargo invocation in the checkout resolved to stable, so
  libFuzzer's `-Z sanitizer` and `cargo miri` were refused in under thirty seconds. The env
  variable `RUSTUP_TOOLCHAIN` outranks the file and reaches the child processes cargo spawns,
  which is where the builds that failed actually ran.

  **`cargo fuzz` builds for the triple it was itself built for.** `install-action` fetches a
  statically linked musl binary, so the campaign built for musl, and a sanitizer cannot link
  against a static libc. The workflow names `x86_64-unknown-linux-gnu`.

  **libFuzzer refuses a corpus directory that is not there.** The growing corpus is restored from
  a cache, and a cache that has never been written creates nothing. The campaign makes the
  directory it writes into; the committed seeds beside it stay read-only input.

  **Rust has no UndefinedBehaviorSanitizer.** `rustc` has never accepted `-Zsanitizer=undefined` —
  UBSan instruments C and C++ semantics — and §42.3 asks for both sanitizers *"where
  Rust/toolchain support permits"*. ADR-0574: the second job runs the standard library's own
  preconditions under `-Zub-checks` with `-Z build-std`, over the same four crates that hold every
  `unsafe` block. ADR-0522 carries the note.

  Both are green now: seven fuzz targets at one minute each, and Miri, AddressSanitizer and the
  undefined-behaviour job. Nothing either tool reported was a finding in the product — the fifth
  cause would have been one, and there was no fifth. Found on the way: two decisions had both
  taken the number 0571, and the later one — the brokered connection of #3 — is now ADR-0573.
- **GitHub Actions was red for five runs, and none of the four causes was the product
  (2026-09-04).** The tracker had been failing since the wasm increment of 2026-09-03 while every
  local gate was green, which is the gap this entry exists to close: four environment
  dependencies, each in a test or in the gate's own observation.

  **A refusal that depended on the host's path layout.** The conformance case for `process.exec`
  granted the directory `/bin/echo` really lives in and expected `/usr/sbin/reboot` to be refused.
  On a merged-usr machine that canonicalises to `/usr/bin/systemctl` — inside the grant whenever
  `/bin/echo` is `/usr/bin/echo` — so the refusal happened here and not on the runner. The
  refused program now lives in a directory the test makes.

  **An offline run that needed another platform's crates.** `xtask/tests/supply_chain.rs` runs
  `cargo deny --offline` over this workspace, and `cargo metadata` resolves for every target:
  wasmtime brought `mach2` and `winapi-util` into the lock file, which a Linux build never
  downloads. The fixture now runs `cargo fetch --locked` first, which fetches every target's
  dependencies and is a no-op where they are already there.

  **A subject picked from the first ten units.** The systemd provider's agreement test read ten
  units and looked for an active `.service` among them; the runner's first ten held none, so it
  skipped where the registry expects it to run. The schema check still reads ten and the subject
  comes from the whole list.

  **The gate's own skip observation, lost to interleaving.** Standard error is unbuffered, so
  `writeln!` with arguments left `SKIPPED <test>: <category>: <detail>` in the pipe in fragments,
  and `cargo test`'s `test <name> ... ` from another thread landed inside it. `skip-check` then
  reported a declared skip as gone and failed a green run — it failed exactly this way on the
  development machine once the other three were fixed. The marker is built as one string and
  written once, and the reader finds it anywhere in a line, guarded by the six categories so
  prose is not read as an observation.

  What this cost: five red runs nobody was watching while six increments landed. What would have
  caught it earlier: `ONO_CANONICAL_CI=1 scripts/gate.sh`, which selects the packaging suite and
  runs `skip-check` the way the runner does. It is the local emulation of the gate job and it
  found the fourth cause.
- **#3, sixth increment and close (2026-09-03): `views`, the full lens.** ADR-0572, decided
  with the user ("views with the full lens"). A package contributes a view beside its commands
  and drives it by events: `views.open` mounts it when a terminal is there and answers
  `mounted: false` when output is redirected, so the package emits its declared fallback;
  `views.submit` takes a tree of the thirteen components, validated and sanitised by the host,
  and an invalid one is `view.protocol_error` with the terminal restored; `view.mount`,
  `view.event` and `view.unmount` go to the package as requests it answers and the SDK queues
  for `next_view_event`. `Esc` and `Ctrl-C` are the host's and arrive as `cancel`; a view the
  package does not close within the call deadline, or leaves open when its invocation ends, the
  host closes. The shell's view host runs the terminal on a thread of its own — raw mode and the
  alternate screen, a layout for each component, keys named as `view.event` names them — and the
  test host's records every tree and injects events, so the conformance suite proves the
  lifecycle without a terminal. Proven by four conformance cases, the renderer's unit tests, a
  pseudo-terminal test through the binary, and acceptance case 217. It also explains a flake this
  board had staged for filing: `host_domains.rs::should_broker_a_tcp_connection_for_a_granted_package`
  failed once in eight gate runs with `["pong:"]` and no payload, and the cause was the test's own
  peer writing its answer in two calls while the package reads one chunk — a test defect, fixed in
  `032fa8d`, and taken off the staging area. The example plugin's connection reads also waited five
  milliseconds, which is a coin toss under gate load; they wait two seconds now. With it, every call
  `protocol.v1.yaml` declares is served or answered with ADR-0573's honest refusal, and the
  manifest no longer refuses a view contribution. Closed with `gh issue close 3`.
- **#3, fifth increment (2026-09-03): `network.listen`, and `network.request` decided.**
  ADR-0573, decided with the user: a request is the package's own protocol over the brokered
  connection, so the host carries no HTTP client and answers `network.request` with
  `provider.unavailable` naming `network.connect`. A listener is checked against `ports`,
  audited, bound on the loopback address, and read as a stream whose values are
  `{connection: handle, peer}`; each handle is a connection the package reads and writes like
  one it opened. What the increment found: a listener's accepted connections must become handles
  inside `streams.next` itself, because the actor that owns the handle table is the one waiting
  in that call — routing them through its message loop waits out the deadline. Proven by a
  conformance case over the fake host, a run through the binary with a real peer, and acceptance
  case 216. Fifteen of sixteen domains have every call the host will serve; `views` is the last
  and is next, as the full lens.
- **#3, fourth increment (2026-09-03): `process.exec` and `network.connect`.** ADR-0570. A
  program is checked as the resolved path against the `programs` glob scope (ADR-0015 T11) and
  runs through the same confined spawn a native package gets, in a directory of its own with
  only the environment the package gave it; its lines and exit status come back as a stream.
  A connection is checked against `hosts` and `ports` — a port scope now accepts `*` and
  `low-high` — audited either way, and held by the shell: the package reads it with
  `streams.next` and writes it with `streams.emit`, `tcp` only. `network.request`,
  `network.listen` and the `views` calls answer `provider.unavailable` naming the brokered path
  that exists. Proven by two conformance cases over the fake host and two runs through the
  binary (`/bin/echo` under the real confinement, a loopback listener through the broker) and
  acceptance case 215. What the increment found: on a host where `/bin/echo` is a link into a
  coreutils bundle, the resolved-path check refuses `/bin/**`, which is the check doing its job
  and the reason the tests grant the bundle's directory.
- **#3, third increment (2026-09-03): the wasm-component tier.** ADR-0569. `runtime.kind:
  wasm-component` loads: the component runs inside the WebAssembly component runtime Ono embeds
  (`wasmtime` 47, the release the pinned toolchain supports), with a WASI context that holds
  nothing but its standard streams, and speaks the same framed protocol a native process speaks.
  The actor drives both through one `Runtime`. `ExecutionTier::Wasm` is available; its rows in
  `kuang_confinement_controls.yaml` are what a component has by construction (`mandatory`) and
  what only a process could have (`not_provided`); memory is exact through the runtime's
  limiter; CPU is preempted at every epoch and not capped, and the boundary says so. Proven by
  three conformance cases over the example package built for `wasm32-wasip2` into its own
  target directory — tier and controls, typed values over the streams, the broker holding a
  component like a process — declared as skips where the target is absent. Still open:
  `network`, `views`, `process.exec`, and an acceptance case with a component in the image.
- **#3, second increment (2026-09-03): `objects`, `relations`, `history`, `process.signal`,
  `secrets`.** ADR-0568. The supervisor reaches them through one JSON service the loader is
  handed (`HostServices`): check the grant against the value the operation will use, audit it,
  hand the JSON over, put what comes back on the wire — a live stream where the contract says
  stream, pulled with `streams.next` with a deadline for the first value. The shell implements
  the service over its provider registry (objects: get, query, resolve, snapshot, subscribe,
  watch), the kernel relationship sources one hop from an object, the history file scrubbed of
  credentials in flags, assignments, headers and bare tokens, and `act` for a signal; `NoHost`
  answers `provider.unavailable`. A secret handle names a secret and the material stays with
  the host; this shell has no secret store and says so. Proven by six conformance cases under a
  fake host, three more runs through the binary and acceptance case 213. Honest gaps, each
  answered `provider.unavailable` rather than faked: `relations.contribute` (no store for a
  package's edges), `history.append` (the history record has no field for a package's
  authorship), `secrets.request` (no store). `network`, `views`, `process.exec` and the wasm
  tier remain.
- **#3, first increment (2026-09-03): host streams, `context`, `schemas`.** ADR-0567. The
  supervisor keeps the streams it opens for a plugin by handle; `streams.next {handle, max}`
  pulls at most `max` values and says whether the stream is complete, `streams.cancel` drops it,
  and a handle the host never opened quarantines the package. `context.get` answers from a
  source the shell publishes before every pipeline — cwd, the innermost object frame, the link
  host, the host name, interactive and redirected, nothing beyond — and the test host hands a
  fixed one. `schemas.get` and `schemas.list` describe a schema's fields, identity, default
  view and origin (core, package, provider), the list as a stream. The example package gained
  `context`, `schemas` and `schema`. Proven by four conformance cases, three runs through the
  binary (`crates/ono-cli/tests/host_domains.rs`) and acceptance case 212. Nine domains were
  missing; `context` and `schemas` are in, `models` came with #5; `objects`, `relations`,
  `history`, `process`, `secrets`, `network` and `views` and the wasm tier remain, and the issue
  stays open until they are.
- **#5 closed (2026-09-03).** `ono-model-broker` exists (ADR-0566). The crate is the catalogue
  (`<config>/kuang/models.yaml`, beside `policy.yaml`), the data-class policy of §31.44 with the
  three named policies defaulting their lists, the `ono-model/1` wire
  (`docs/spec/kuang/model-broker.v1.yaml`: one JSON document in, one out, over a program the
  operator configured — no HTTP client), and a broker trait that takes an already-chosen
  provider and an already-classified request and has no way to reach a grant. The supervisor
  answers `models.list` (the catalogue filtered by the grant's `providers` scope) and
  `models.infer` (choose, check the grant against the provider id, classify every segment,
  disclose the §31.82 plan to the trail before the first remote call, then send); `get model`
  reads the catalogue; the example package gained `models`, `infer` and `inject`. Proven by
  thirteen unit tests in the crate, seven conformance cases under the test host — scope filter,
  an answer through a configured command, a provider outside the scope refused and audited, a
  denied class refused with the classes named, a transformed class sent redacted with the plan
  disclosed once, untrusted text asking for a capability changing no grant, and no configured
  model told so on the turn — five runs through the binary (`crates/ono-cli/tests/models.rs`) and
  acceptance case 211. What the issue did not know: its resume pointers (ADRs 0377–0400, cases
  190+) were long taken, and the shell swallows a plugin invocation's mid-flight failure (above,
  *Found, not yet filed*), so the refusals are proven through the trail rather than the exit
  status.
- **#17 closed (2026-09-03).** `apt update` has a spelling: `refresh package-source <id>`, or
  `get package-source | refresh package-source` over all of them. ADR-0562's sequence, delivered,
  with its first point corrected by ADR-0565: the target is `package-source`, not `repo`, because
  `targets.yaml` already gives `repo` to §8.2's source repository. `ono.package-source/1` is
  `provider + id` with `name`, `url`, `enabled` and `refreshed`; the verb `refresh` is in
  `verbs.yaml` under the §40.1 review; `package-source.list` and `package-source.refresh` are
  capabilities both package providers advertise. apt is listed through `apt-get update --print-uris`
  (`indextargets` lists only fetched indexes and is empty on a fresh machine; it supplies labels)
  and refreshed by one shared `apt-get update` per pipeline (the run is reused for five
  seconds after it finishes); dnf's repositories are read from `/etc/yum.repos.d/*.repo` because
  `dnf repolist` is a table for people; zypper's through `zypper --xmlout lr`. `changed` is the
  index file's modification time before against after, never the command's prose. Proven by
  seven unit tests over the three parsers, two rpm-provider tests under a scratch root
  (`crates/ono-provider-linux/tests/package_sources.rs`), eight tests through the binary with
  fake managers on PATH (`crates/ono-cli/tests/package_sources.rs`) and acceptance case 210 over
  the image's real apt. What the issue did not know: apt cannot refresh one source, and a
  mutation that gives one result per source would have fetched the same indexes once per source;
  the shared run is the honest compromise, and the ADR says how short its window is and why.
- **#122 closed (2026-09-03).** `Ctrl-Up` and `Ctrl-Down` walk the history without an anchor
  (ADR-0564). The bare arrows were already readline's `history-search-backward` — anchored on
  the text before the cursor — and nothing was bound to the modified arrows, so the walk that
  steps to the previous entry whatever was typed did not exist. Two actions,
  `HistoryPreviousUnanchored` and `HistoryNextUnanchored`, share the existing `HistoryNav`: the
  anchor is taken once when a walk starts and applied only by the anchored steps, so the two
  kinds of step mix inside one walk and the saved line comes back either way. Proven by four
  editor tests (`crates/ono-editor/tests/history.rs`), a pseudo-terminal sending xterm's
  `\x1b[1;5A` (`crates/ono-cli/tests/history_keys.rs`, red without the fix) and acceptance case
  209. What the issue did not know: it offered the inputrc reading — `Ctrl-Up` as a second key
  for the prefix search — as the alternative, and ADR-0564 rejects it because it would leave the
  unanchored walk unreachable, which is the defect itself; a user who wants it has `Keymap::bind`.
- **#123 closed (2026-09-03).** Tab after `cd ..` answers `../`. `path_candidates` is now
  `path_candidates_in(root, prefix)` — the same split at the last `/` and the same `read_dir`,
  plus one rule: a prefix that itself names an existing directory and does not end in `/` is
  offered with its `/`, which is readline's behaviour and covers `.`, `..` and `../..` alike. An
  ordinary directory is not offered twice; the list is deduplicated. Proven by three unit tests
  over a scratch tree (`crates/ono-cli/src/repl.rs`), a pseudo-terminal typing `cd ..<Tab>`
  (`crates/ono-cli/tests/completion.rs`, red without the fix) and acceptance case 208. What the
  issue did not know: `.`'s answer sorts `./` ahead of the hidden entries, because `/` orders
  before every letter — so the current directory is the first offer, not the last.
- **#121 closed (2026-09-03).** `Ctrl-L` clears the screen. `Renderer::redraw_from_top` clears
  the whole screen, homes the cursor, forgets the rows above the cursor and paints the frame; the
  `read_line` loop in `ono-cli/src/repl.rs` gives `Outcome::Redraw` its own arm instead of folding
  it into the no-ops. Proven at three levels: the renderer's bytes
  (`crates/ono-editor/tests/terminal.rs`), a real pseudo-terminal fed `\x0c` mid-line
  (`crates/ono-cli/tests/clear_screen.rs`) and acceptance case 207 under `script(1)`. What the
  issue did not know: the pty test's first draft waited for the word `local`, which the startup
  banner already says, so it typed into the cooked terminal before the prompt existed and the
  terminal's own echo made the assertion pass for nothing. The prompt is the first thing that
  says `local://`, and that is what a pty test has to wait for before it types.
- **#29 closed (2026-09-02).** `docs/ACCEPTANCE.md` §4.8 is the v0.4.1 definition of done: 118
  unticked boxes in fourteen subsubsections following the H0–H12 phase sequence, every one of the
  tranche's 101 issues cited by the box that closes it, and every bullet of §66.1–§66.9 covered.
  ADR-0429 records the five decisions behind the form. Acceptance-case numbers **180–200** are
  reserved for the tranche and ascend with the phase order.
  `xtask/tests/spatial_evidence.rs` now reads §4.7 up to `### 4.8`, so §4.7's evidence harvester
  stops at the tranche boundary instead of sweeping 257 not-yet-written test names into itself;
  the v0.4.1 counterpart `xtask/tests/hardening_evidence.rs` is §4.8.1's first box and the last
  box of the tranche to close. From here `scripts/release-check.sh` fails on §4.8's first open
  box, and that is the correct state for a tranche that has just started.
- **#31 closed (2026-09-02).** All four failure proofs are in, red by design, tracked under
  *Deferred* above (ADR-0430, ADR-0431). Two diagnoses came out of writing them and belong to the
  phases that own the fix: the `map --live` pathology is **unconditional rather than
  cardinality-driven** — `map --live --json | take 1` answers in 0.2 s, and the second value never
  comes because the root projection is domains and collections while `MapSnapshot` compares node
  and edge labels only, so a picture made of names that cannot change never reports a change
  (#22); and issue **#20**'s instance measures 29.7 s at Profile M, inside §33.3's thirty-second
  budget by 0.3 s and sixty times outside §33.2's target, so it needs a phase-H7 frame-budget
  proof under a terminal rather than a watchdog that would be a coin toss (ADR-0252, #21).
- **The five load-sensitive tests, and three product defects behind them (2026-09-03).** Six
  commits. **Two of the five were the product rather than the test**, which means the flakes were
  the only thing reporting a real fault — and twice this board had recorded one as "known and
  pre-existing" and moved on.

  `ono --agent` **died of its own startup summary**: `eprintln!` panics on a failed write, and a
  fixture that reads the first two of §11.2's nine lines and drops its receiver closes the pipe
  under it. On a quiet machine all nine are already buffered; on a busy one the agent exits **101
  between announcing the socket and serving it**, so the *first* `link host` gets
  `E0601 … Connection refused`. Measured: 11 of 30 agents with a minimal harness, and every time
  with `/dev/full` as stderr. `authenticated_link.rs` keeps its receiver alive **with a comment
  saying why**; the product was never brought along. `ono_core::diagnostic!` is `eprintln!` with the
  write discarded (ADR-0549, `e7e22a8`).

  **A completion that runs out of budget silences completion for five seconds**: the
  nanosecond-budget probe's detached read finds nothing and writes that nothing into ADR-0252's
  process-wide cache, where `FRESH` keeps it — the opposite of what that ADR's own comment
  promises. No window a test could honestly wait in would have covered it, which is why ADR-0517's
  scaling was never going to be enough (ADR-0550, `84612cb`).

  The three test defects were fixed by **removing the dependence rather than widening a
  tolerance**: the repaint test waits for the frame it asserts about; the width comparison earns a
  third read when two disagree and skips honestly if the host will not hold still (ADR-0552); and
  the trace filter binds its own loopback connection, because its assertion was **unsatisfiable by
  any correct implementation** — 84 of 256 nodes are sockets with other peers, and reaching them is
  what §22 is for. It was green only because the command refused, and it refused only because this
  host held no TEST-NET-1 connection (ADR-0551).

  Tally: 12 × `cargo test --workspace --all-features` at load 8–13 on 8 processors — all eight
  tests **0 failures, 0 skips**.

- **Nine of the ten class-b defects outside the tranche (2026-09-03), #9–#26.** Twelve commits,
  ADR-0553 … ADR-0562, cases 201–206, `acceptance: 134 passed`. The **acceptance suite caught a
  wrong fix**: #26's first attempt took the refusal from the first withheld group, which is right
  for a relation and wrong for `--type`, and case 094 said so. #9's reproduction no longer held —
  ADR-0517's load-scaled watchdog had closed that half — but the cost was real (`ListUnits` already
  answers with each unit's object path and the provider asked `LoadUnit` for it again; 1361 D-Bus
  calls → 793), and **the ADR carries the release figure that does not move as well as the debug
  figure that does**. #17 stays open as a tranche: a mutation acts on an object it names (§11.5)
  and a refresh has no package to name, so `ono.repository/1` comes first (ADR-0562).

- **H0 complete (2026-09-03), #30, #117, #118 — last rather than first, and that changed two of
  them.** #118's inventory holds §6.1's twelve rows with `input_trust` and `required_enforcement`
  copied character for character, and the test holds them against a table typed from the
  specification rather than read from the file — so the registry cannot drift and take its own
  check with it. It unblocks what H2 recorded as owed, and **a fifth method on the `RemoteService`
  trait is now a red gate**. #117 indexes all seventeen contracts, and the decision that gives the
  index teeth is that `validated_by` must be under `xtask/`; a numeric value in a
  `remote_limits.yaml` ceiling row is now a failure. **#30's honest answer is that it can no longer
  do what it was written for** — the "before" is twelve phases gone — so ADR-0548 carries a
  `Spec deviation` heading and the file says which state it holds. ADR-0546, ADR-0547, ADR-0548.

- **H11 complete (2026-09-03), #104–#110.** Seven commits. The reproducibility work was *run*
  rather than asserted, and the result was not what the issue expected: with ADR-0526's determinism
  block deleted and a build forced under `de_DE.UTF-8`, `Australia/Eucla` (+08:45) and `umask 077`,
  the packages came out **byte-identical anyway** — cargo-deb 3.7.0 and cargo-generate-rpm 0.21.0
  both honour `SOURCE_DATE_EPOCH`, emit no `BUILDHOST`, write uid 0/gid 0 and sort glob-expanded
  assets. What the probe did find is that the second build's *files* were mode `0600`, so
  `compare-builds` now compares the artifact's mode as well as its bytes. Two smaller findings came
  out of writing the tests: `dpkg-deb --contents` renders mtimes in the **reader's** timezone, and
  both packaging tools write numeric `0/0` rather than `root/root`.

  #109's glibc floor is **read out of the ELF** rather than assumed (first run measured
  `GLIBC_2.34`), and #110 replaces `action-gh-release` with verify → draft → upload → download back
  and compare digests → clear the draft, so the bytes that were tested are the bytes that are
  published. ADR-0526 … ADR-0532, case 199, `acceptance: 127 passed, 0 failed`.

  **#107's box is open on purpose** — see *Deferred* — and that is the second time this tranche a
  phase has declined to tick a box it could not prove (H8's #94 was the first).

- **H8 complete (2026-09-02), #88–#94 and the three flaky-test defects #6, #7, #27.** Eleven
  commits on `implementation-h8-test-truthfulness`. Three findings are worth more than the boxes:

  **The skip marker had never reached a log.** libtest captures the print macros and shows them
  only for a *failed* test, so every §38.1 skip this repository thought it was announcing was
  invisible to anything reading the output. `skipped` now writes to `std::io::stderr()` directly,
  and `cargo xtask skip-check <log>` is §38.3's verification step. RED for #88 was 41 unannounced
  bare `return;`s plus one multi-line `eprintln!("skipped: …")` in `spatial_map.rs` that had
  survived ADR-0428 for a whole release.

  **The 331 leaked processes were not the systemd fixture.** `ono_process::PtySession` had **no
  `Drop` at all**, so every PTY test orphaned its session leader and everything under it, and
  `Shell::try_run` reported an overrun and walked away from the child. Both are fixed; ADR-0431's
  deliberate difference between `run_bounded` and `Shell` survives. Stated residual: `Shell` kills
  the child rather than a group, because `ono-testkit` forbids `unsafe` and a group needs
  `pre_exec`.

  **#6's test was hiding a product defect.** Written so that it really needs a resize, it went red
  *every* time and named the cause: `ready_key` called `read_event_timeout` every 5 ms during a
  projection and dropped everything that was not a key — including the resize. `read_event_timeout`
  now reports a size change until `remember_terminal_size` acknowledges it, which also closes the
  case where a resize beat crossterm's SIGWINCH handler into existence. The test went from 45 s to
  2.5 s.

  Also: #27 and #7 are **one** defect — a thread forking between the test's `open` and `close`
  inherits the descriptor and `execve` answers `ETXTBSY`, arriving as exit 126. And **eight**
  §4.8.13 boxes named an acceptance case that does not exist under that name. ADR-0513 … ADR-0522.

- **H7 complete (2026-09-02), #82–#87 and the five class-b defects #8, #20, #21, #22, #25.**
  Sixteen commits on `implementation-h7-spatial-performance`, merged without a conflict. Measured
  before optimised, on the named reference environment `ryzen-3900x-ubuntu-2604` with the run's own
  load average recorded beside every figure: `spatial.map_first_frame` at Profile L falls from
  **25 748 ms to 3 514 ms**, a selector miss at Profile M from **530 ms to 181 ms**, and
  `service.enumeration` at S from **870 ms to 422 ms**.

  Two diagnoses overturned the assumption in the issue that carried them. `enter compute;
  look --json` costs **942 ms with no extra processes and 1 135 ms with sixteen hundred** — an
  almost flat curve from the origin, so the Profile M failure was never cardinality; it was 569
  systemd units against three *sequential* D-Bus round trips. And two thirds of the Profile L map
  cost sat in `MapHorizon::place`, which deduplicated by scanning the vector it was building — five
  billion comparisons at 100 000 sockets, and not the global graph build #87 named.

  §33.3's floor is now met by a live map that **says it is waiting** rather than falling silent,
  and the stillness clock resets on a value being *sent* rather than on an event arriving — timing
  each wait never fired, because events arrive that change no picture. `Comparison` answers
  `Unmeasured` and `ForeignEnvironment` as distinct verdicts so §65.10's skip-as-pass cannot happen
  to a benchmark. ADR-0488 … ADR-0498, `E1401`, cases 195–198.

  **#71's measured half is written**, and it was cheap once `cargo xtask perf` existed: `cancel_ms`
  is the p95 of twenty samples rather than one measurement — 2.2–4.5 ms at S and M, 20.9 ms at L,
  against §23.3's p95 < 100 ms. A p99 wants about a hundred samples; that is the only difference.

- **H3 complete (2026-09-02), #51–#57.** One central `Limits` contract whose every setter clamps
  into the range `limits.yaml` declares, so an unlimited instance cannot be written down;
  `docs/spec/hardening/remote_limits.yaml` created as §52.1's registry holding **no numbers** — one
  row per ceiling pointing at its `limit_key`, its refusal, its audit class and its enforcement
  stage. A `ConnectionRegistry` behind a real 32-connection ceiling, a pending-handshake semaphore
  that gates *TCP accept* (so nothing is spent on the peer and §13.1 is not violated by sending a
  refusal frame), a 10 s timeout wrapping both TLS and the opening `Hello`, a per-fingerprint limit
  keyed on the authenticated fingerprint rather than the address, and TLS moved off the accept loop
  onto a per-connection task. `AuditKind::ConnectionLimitDenied` — declared by H2, unreachable
  until now — is raised. `E1501`, `E1502`. ADR-0501 … ADR-0505, case 188, §4.8.4 ticked.

  **Live revocation landed rather than being deferred a second time.** ADR-0470 deferred it on one
  stated condition — that H3 would build the registry — and the agent treated the expiry of that
  condition as binding: a one-second sweep re-reads the store and closes every session whose
  fingerprint it no longer lists, well inside §12.5's five seconds, with ADR-0470's immutable
  per-connection `AuthorizationContext` untouched. The *grant* is still fixed for the life of the
  connection; only the connection's existence changes.

- **H6 complete (2026-09-02), #75–#81.** `each` streams. `eval.rs::run_each_block` and its second
  `StageList` are gone: `each { … }` is bound and assembled as a stage of its own pipeline, and the
  stage asks the evaluator over a bounded channel of one while a driver loop answers and drains at
  the same time. Both H0 failure proofs are un-ignored with **no assertion touched** — the diff of
  `each_streaming.rs` is the two `#[ignore]`/`// REASON:` blocks and one paragraph of module doc —
  and the `where` differential stayed green throughout. `each {…} | each {…}` works at all now; it
  answered `provider.unsupported` before. Memory is measured rather than asserted: two block
  invocations for a 200-value source and two for a 2000-value one. #78's capture inventory is 21
  classified sites in `docs/spec/hardening/streaming.yaml` with a gate that fails in four
  directions, and it caught two real removals as the code changed under it. ADR-0479 … ADR-0483,
  cases 193 and 194, §4.8.7 written and ticked. No code in the reserved `E1301`–`E1319` range was
  needed: every refusal this tranche makes already had one.

- **H2 complete (2026-09-02), #40–#50.** A v0.4.1 listening agent now authenticates every client
  **and authorizes only the ones an operator listed**: §59.1 moved from "the unknown client reads
  the provider inventory" to "the unknown client is refused with `Ono-Sendai-E1202` before provider
  negotiation". `authorized_clients` with a fail-closed parser that distinguishes *missing* from
  *corrupt* and exits before `bind` on the latter; atomic updates; four `client-key` commands;
  observe-only by default, with no option on `add` that could grant an action; `ActionGrant` as a
  newtype whose only constructor refuses to represent a wildcard; an immutable per-connection
  `AuthorizationContext` built from the fingerprint alone; `ServerConfig::offer()` replaced by
  `offer_for(&PeerAuthorization)`; dispatch checked again independently on all four paths;
  `E1201`–`E1204`; audit events whose record has no field a payload could occupy; and the four
  trust words as four fields on `ono.link/1`. ADR-0466 … ADR-0475, cases 182–187, §4.8.3 ticked.

- **#98, #99, #100 closed (2026-09-02), out of phase order and deliberately so.** H10 touches
  `.github/`, `docker/`, `scripts/` and `xtask/` and no runtime code, so running it beside H1 and
  H4 delays no safety work; §57's staging rule is about refactoring landing before safety work,
  and nothing here refactors anything. Seven third-party Actions are pinned by commit SHA and four
  release-critical images by digest, `release.yml` drops from workflow-wide `contents: write` to
  the publishing job alone, and `pull_request_target` is banned outright. The gate scan
  `xtask/src/supply_chain.rs` keeps all three true — 28 tests in `xtask/tests/supply_chain.rs`,
  ADR-0433. Three boxes of §4.8.11 are ticked.

## Session records (2026-08-27 … 2026-08-29)

Every session below is complete. They are kept because each carries the reasoning behind a
decision the code no longer shows, and because several of the problems now in the tracker were
first observed here.

**The five parallel tranches have landed (2026-08-29).** The board carried them as claims while
they ran; every one of their branches is now an ancestor of `implementation`, every worktree is
clean, and nothing is held. What each of them delivered is recorded under *Done* and in the
commits their merges carry.

| Tranche | Branch | Merged as | What it delivered |
|---|---|---|---|
| KUANG/11 | `close-kuang` | `3fc924a` | isolation, the wasm tier, the missing host API domains, contributed commands (C-4, B-kuang-3) |
| security | `close-security` | `0b09af9` | the §35.6 fuzz targets and package signature verification (C-2, C-5) |
| remote | `close-remote` | `045fb0c` | agentless mode and an authenticated transport (C-3, B-remote-2) |
| honesty | `close-honesty` | `5865421` | three release-checklist claims made checkable rather than judged (ADR-0401, ADR-0402) |
| last | `close-last` | `c7a63f4` | B-data-9; C-6 stays designed-not-written and is recorded under C-6 |

**What running six agents on eight cores cost, kept because the next parallel run needs it:**
load average 49 and no idle CPU, under which the testkit's 20 s timeout fires on healthy code and
agents start chasing phantom failures — which costs more load. The ones that kept running were
capped at `-j 2` / `--test-threads=2`, told to pass `--all-features` on *every* cargo command (a
bare `cargo test` between two gate runs changes the feature set and rebuilds the workspace
twice), and told never to delete `target/` or the incremental caches to save disk.

**S11c — the four defects the v0.4 dogfooding session left open — is complete (2026-08-29,
agent `S11c`).** Seven commits, gate green on each; the container ran on image
`ono-sendai:acceptance-s11c`. Final verdicts: `gate: green`, `acceptance: 88 passed, 0 failed`,
`release-check: the shell is release-ready`.

| Commit | What it delivers |
|---|---|
| `fix(spatial)` | a null a provider left is not an empty exit (ADR-0209), and `ono.socket/1`'s `process` carries a refusal where the owner scan was refused |
| `fix(spatial)` | `find place` refuses a question it cannot ask (ADR-0210): E0202 for a field nothing declares, and evaluation errors surface |
| `test(spatial)` | the PTY budgets are liveness bounds, not a race with the machine |
| `fix(spatial)` | a refusal lists its candidates as values, not as newlines in its message (ADR-0211) |
| `fix(spatial)` | the hidden count says what it counts (ADR-0212) |
| `fix(spatial)` | two corrections the container found in the first two of those — an exit is "stated" by its group, and a record rather than a target decides whether a predicate can be asked |
| `test(spatial)` | the jump-refusal budget is a hang guard, not a race with the machine |

**Two things the container caught that the workspace suite did not**, both in this session's own
first drafts, both now covered by a workspace test as well: an exit is keyed by its *group*
(`process`) and not by its `follow` label (`owner`), and a provider *target* may serve several
schemas, so the *record* decides whether a predicate can be asked of it. Cases `091`/`094`
(`44.2m`, `44.5g`) and `092` (`44.3c`) are the assertions that found them.

**One flake seen twice and not fixed by that session — fixed since, by `4e53ee4` (ADR-0230):**
`ESRCH` from a `/proc/<pid>/stat` read had no `std::io::ErrorKind` name and was classified
`provider.unavailable`, so a vanished row became a partial failure instead of being omitted. The
shell was wrong and the three tests were right; none of them changed. What that session recorded:
`spatial_topology_missing.rs::should_bound_the_root_horizon_instead_of_listing_every_known_object`
runs `get process | count`, and on a busy machine a process listed by the enumerator exits before
its `/proc/<pid>/stat` can be read, so v0.2 §9's partial-failure semantics give the run exit 1 —
correct behaviour, and a test premise that only holds on a quiet host. Seen in one gate run and
one `release-check` run; green on the next of each. Filed on the board, and an issue since.

The four findings, and the one that was offered as a bonus:

- [x] finding 2 — a `null` a provider answered is rendered as `empty` (ADR-0209)
- [x] finding 3 — `find place --where` swallows unknown fields and evaluation errors (ADR-0210)
- [x] finding 4 — a multi-line diagnostic prints `\u{a}` instead of its line breaks (ADR-0211)
- [x] finding 1 — `look`'s hidden count does not describe the list above it (ADR-0212)
- [x] `help here` (§38.2, a SHOULD) — filed on the board by S11c and **delivered on the same
  day by `13b6157` (ADR-0271)**, with the help metadata, the completion and the acceptance case it
  needed: `enter process 1; help here` names every exit with what is behind it and the spelling
  that traverses it, and says `permission_denied` where the provider gave one rather than a count
  it does not have. `ono-cli/tests/spatial_help.rs` (three cases), `ono-command/tests/completion.rs`
  (two topic cases), case `102` s4x.

**S6 + S7 + S8 + the map correction are integrated on one branch (2026-08-28, agent `integrate-1`).**
Three merges, in that order, on top of `implementation` at `cbbcd2c`; gate green, acceptance 75/75.
The resolutions worth knowing later:

- **`home` extends the navigation history (ADR-0184).** S8's ADR-0170 had excluded it; §20.1 lists
  `home` in the `movement` enum and §2.4 makes every movement reversible, so `back` returns
  through it. ADR-0170 is superseded on that one point, and
  `docker/acceptance/cases/106-spatial-remote.case` s8u spends three `back`s where it spent two —
  its assertion is unchanged.
- **`map --live` has two surfaces, and `Invocation::displays()` (ADR-0173) decides which.** Where
  the values are *shown* — an interactive terminal — it is S6's full-screen polled view
  (ADR-0176); where they are *consumed* it is S7's event-driven stream (ADR-0180), which is what
  `map --live --json | take 3 | to json` reads. Shown with no terminal to draw into it is still
  refused with `spatial.unsupported`, which §25.2 requires rather than a faked view. ADR-0180
  itself assigns the alternate screen to S6 and the stream to S7; this is that split.
- **The expansion memory of ADR-0183 lives in `crate::spatial::map::project_at`**, the one
  re-projection path the still map, the full-screen view and the live stream all take (§45.4).
- **`look --changes` answers `unknown`, not `unsupported`** (ADR-0181), so case 102's s4q reads
  the new word. It is delivered now; `unsupported` was the honest answer while it was not.
- **A case body that ends with a background job still running makes the acceptance runner report
  exit 129**, however green its assertions are: the orphan holds the outer `script`'s
  pseudo-terminal open. Reproducible with `( sleep 5 ) & exit 0` and nothing else. Case 107 now
  reaps its typist inside `drive`; any future PTY case must do the same.

- **What a target answered belongs to a moment and to a host (ADR-0190).** ADR-0186's target
  cache collided with two other decisions when the branches met. With ADR-0180, a live map
  re-projected by reading the answer from *before* the change, so `live::reproject` now calls
  `SpatialSessionState::forget_targets` first — an event is precisely the statement that §33.3's
  lifetime assumption no longer holds (§33.2). With ADR-0169's remote scopes, the cache key was
  the target name alone, so a session that jumped into a link recalled the *local* answer for the
  remote host; the key now carries the scope (§43.7). Case 106's s8l catches the second, and
  case 108 the first.

Two tests are environment-dependent on a developer machine and green in the container. Neither is
a merge regression:

- `spatial_relationships_missing::should_show_the_connection_edge_appear_and_vanish…` — **the
  TIME_WAIT identity collapse S7 recorded**, diagnosed exactly here and **fixed since by
  `79d6a9c` (ADR-0231)**: a record supplying none of its identity components states no identity,
  so two TIME_WAIT sockets are two objects. Was: `ono.socket/1` declares
  `identity: [inode]` and a socket in TIME_WAIT has no inode, so *every* TIME_WAIT socket on the
  host projects to the same `SpatialId`. The test's own closing connection is then merged with
  whatever else on the machine happens to be in TIME_WAIT, and the third live value describes a
  foreign peer instead of the closure. The acceptance container has no other TIME_WAIT sockets,
  which is why case 108 is green and this is not. **Exit test:** two TIME_WAIT sockets are two
  places. The fix belongs to the v0.2 identity contract — a record whose identity components are
  all null has no identity and must not merge (§2.17, §35.3) — and is its own increment, not an
  integration's.
- `spatial_topology_missing::should_complete_the_relations_available…` — a PTY completion test
  with an 8 s budget; it fails under parallel load and passes with `--test-threads=1`.

**The v0.4 tranche is running (started 2026-08-28).** The specification is
`docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md`; its executable requirements are
the nine `crates/ono-cli/tests/spatial_*_missing.rs` suites (175 tests) and the ten
`docker/acceptance/cases/09x-spatial-*.case.v04` scenarios (139 assertions). The build order is
§50's own dependency-driven sequence, and a phase is done when its suites are un-ignored and
green — never by judgement:

**A phase is done when the tests the map assigns to it are green — not when a suite is.** The
test-to-phase analysis of 2026-08-28 (175 tests read body by body) gives the counts below and
found that §50 leaves eleven normative areas unassigned; they are slotted here, and the S4 block
is split, because 102 of the 175 tests first become attemptable when the command surface exists:

| Phase | Tests | Also owns (areas §50 never assigns) |
|---|---:|---|
| S1 | 5 | §47 configuration declarations |
| S2 | 1 | §18 device spaces — §50's identity list omits Device although §7.7 makes DEVICES a domain |
| S3 | 3 | — (`find place` + the ADR-0124 rewrite in one commit) |
| S4a `look`/`near` + domains | ~30 | §31 `trace` interop (a trace never moves the place) — **done**, 32 tests |
| S4b `enter`/`follow` | ~30 | §30 `cd`/place integration, §35 permission honesty |
| S4c `back`/`up`/`home`/`trail`/`jump`/`pin` | ~25 | §46 session state, §29.2 script isolation — **done**, 19 tests |
| S4d storage and the cwd distinction | ~12 | §15 mount boundaries |
| S4e the `spatial.enabled` refusal path and the §34 budgets | ~5 | §47 behaviour half |
| S5 | 27 | §26 landmark engine, §39 ASCII fallback — **done**, 26 tests; 2 deferred (ADR-0165) |
| S6 | 13 | §5 startup horizon, §21 prompt/HUD, §27.2 picker, §9.4 completion — **done**, 14 tests |
| S7 | 7 | — |
| S8 | 12 | — |
| S9 | 2 | — |
| S10 | 3 | — |
| S11 | 0 | the ten `.case.v04` scenarios renamed and green, the §34 budgets as gates, **`docs/ACCEPTANCE.md` §4.7 written from v0.4 §52** — without it `release-check.sh` cannot see this tranche — **the ten scenarios done**, 87/87 cases green |

| Phase | Delivers (§50) | Suites it turns green |
|---|---|---|
| S1 | spatial core contracts: `SpatialId`, projection, canonical places, relation registry, hierarchy, trail, structured errors, machine-readable registries | `spatial_contracts_missing` (registry, errors) |
| S2 | provider identity and relation bridge, canonical parents, permission-state propagation, conformance | `spatial_identity_missing`, `spatial_contracts_missing` (conformance) |
| S3 | index, aliases, selector resolution, `find`, neighborhood, pins, freshness | `spatial_topology_missing` (discovery) |
| S4 | the navigation commands | `spatial_navigation_missing`, `spatial_topology_missing`, `spatial_storage_missing` |
| S5 | `SpatialMap`, ranking, clustering, zoom, text renderer, relation inspection | `spatial_map_missing` |
| S6 | the interactive full-screen map | `spatial_interactive_missing` |
| S7 | live topology, tombstones, landmark updates, freshness | `spatial_relationships_missing` (live), `spatial_identity_missing` (tombstones) |
| S8 | remote federation | `spatial_remote_missing` |
| S9 | KUANG/11 spatial SDK | `spatial_contracts_missing` (§36) |
| S10 | v0.3 adapter reconciliation | `spatial_contracts_missing` (§37) |
| S11 | release hardening: the ten §44 scenarios renamed to `.case` and green | the acceptance suite |

Architecture is normative in responsibility, not in name (§45): `ono-spatial-core` (identity,
places, relations, trail, tombstones — no rendering), `ono-spatial-index` (registration,
reconciliation, aliases, freshness, canonical parent, pins), `ono-spatial-query` (look/near/find
plans, ranking, zoom, clustering), `ono-spatial-render` (text and full-screen), `ono-spatial-events`
(event merge, diff, live). `ono-cli` parses, dispatches and owns the session place — nothing more
(§45.6).

- (empty — the v0.4 tranche is complete through S11b; no agent holds a claim)

**S1 — spatial core contracts — is complete (2026-08-28, agent `S1`).** Five commits, gate green
on each:

1. `feat(spatial)` the fourteen §40 errors as the `spatial` family `Ono-Sendai-E1001`–`E1014`
   (ADR-0125, ADR-0127) in `docs/spec/errors.yaml` and `ono_core::ErrorCode`.
2. `feat(spatial)` the §41 registry: `docs/spec/spatial/{spatial,spaces,relations,landmarks}.yaml`
   (ADR-0126, ADR-0128), wired into `xtask spec-check`.
3. `feat(spatial)` `crates/ono-spatial-core` — `SpatialId` and the §10 tiers (ADR-0129), the
   `SpatialObject` projection, `SpatialScope` with boundary detection, `Place`, `HierarchicalEdge`
   and `RelationshipEdge`, the canonical geography, the canonical-parent resolver (ADR-0130), the
   navigation trail and tombstones.
4. `feat(spatial)` `crates/ono-spatial-index` — registration and §42.1 reconciliation, the alias
   and search index, freshness, canonical-parent lookup, bounded relation summaries and pins
   (ADR-0131).
5. `feat(config)` the eleven `spatial.*` settings of §47, plus the five landmark thresholds §26.3
   requires to be configurable.

Green from `crates/ono-cli/tests/spatial_contracts_missing.rs`:
`should_register_the_whole_spatial_error_family_in_the_error_taxonomy`,
`should_ship_the_machine_readable_spatial_registry`,
`should_declare_every_canonical_space_with_the_fields_the_registry_requires`,
`should_declare_every_relation_with_its_direction_labels_and_confidence`,
`should_expose_every_spatial_setting_as_a_typed_setting_with_its_default`.

Still ignored in that suite and correctly so: `should_serve_exactly_the_canonical_spaces_the_registry_declares`
and `should_serve_every_relation_it_declares_and_declare_every_relation_it_serves` run `map` and
`near`, which S3–S5 deliver. The registry-versus-implementation drift they describe is already
enforced against `ono-spatial-core` by `cargo run -p xtask -- spec-check`
(`xtask::contracts::check_spatial_implementation`); those two tests add the third party — what the
*commands* serve — and go green with S5.

**What S2 needs from S1** — the three things:

- `ono_spatial_core::Projection::project_as(record, object_type)` is the provider seam. The type
  is the caller's, because `ono.socket/1` is a `Listener` or a `Connection` and `ono.file/1` is a
  `Directory` or a `File`; `spatial_types_of(schema)` lists the candidates. `project` (without the
  type) works only where exactly one candidate exists.
- Identity is scoped and opaque (ADR-0129). Everything but a process takes its schema's identity
  fields plus the scope chain; a process takes boot identity, pid, start time and pid namespace
  (§10.2), reading `pid`, `started` and, where a provider supplies it, `pid_namespace`. Registering
  the same `(scope, ObjectRef)` under two ids is `spatial.identity_conflict`.
- The §42 provider claims block (`spatial:` under each entry in `docs/spec/providers/*.yaml`) is
  **not** written yet; `spatial_contracts_missing::should_declare_the_spatial_claims_on_every_provider_that_feeds_the_spatial_index`
  is S2's. Its `identity_strategy` must be one of `stable`/`lifetime`/`observation`, matching
  `ono_spatial_core::IdentityTier`, and its `cost_class` one of `ono_spatial_core::CostClass`.

**S2 — provider identity and relation bridge — is complete (2026-08-28, agent `S2`).** Six
commits, gate green on each:

1. `feat(spatial)` the §42 provider claims in `docs/spec/providers/*.yaml`, enforced by
   `xtask::contracts::check_provider_claims` (ADR-0132).
2. `feat(providers)` `pid_namespace` on `ono.process/1` and `ono.process-detail/1`, read from
   `/proc/<pid>/ns/pid` (ADR-0134).
3. `feat(spatial)` `ono_spatial_index::bridge` — which place a record is, and reconciliation
   (ADR-0133).
4. `feat(spatial)` the core exact relations, composed from provider facts (ADR-0135).
5. `feat(spatial)` permission-state propagation from the provider to the group (ADR-0136).
6. `test(spatial)` the bridge's type table held against the canonical geography.

**§50's gate for S2 — "provider objects can be reconciled into one graph without duplicate
identity for known-equal objects" — is met**, proven twice in
`crates/ono-spatial-index/tests/bridge.rs`: one process seen through `ono.process/1` and
`ono.process-detail/1` is one place, and one disk seen through `linux.sysfs` (`ono.device/1`) and
the util-linux `lsblk` adapter (`ono.block-device/1`) is one place — which is also §37.1's
adapter identity merge, four phases early.

Green from `crates/ono-cli/tests/spatial_contracts_missing.rs`:
`should_declare_the_spatial_claims_on_every_provider_that_feeds_the_spatial_index`. The other 46
new outcome tests live in the crates: `crates/ono-spatial-index/tests/{bridge,relations,conformance}.rs`
(37) and `crates/ono-provider-linux/tests/process.rs` (3), plus the existing suites unchanged.

**What S3 needs from S2** — the three things:

- **`ProviderBridge` is the entry point, not `Projection::project`.** `bridge::spatial_type_of`
  decides a record's place from the record (`ono.socket/1` → Listener or Connection,
  `ono.file/1` → Directory or File, `ono.device/1` → BlockDevice or Device), and
  `ProviderBridge::absorb(index, records, at)` registers a batch and settles its relations.
  `Absorbed` keeps four outcomes apart: `added`, `reconciled`, `unplaced` (a schema §7 gives no
  domain — a value, not an error) and `refused` (a place that could not be built). A schema that
  no canonical domain holds — `ono.image/1`, `ono.link/1`, `ono.plugin/1`, `ono.package/1` — is
  deliberately not a place (ADR-0133).
- **Selector resolution has two different keys, and they are not the identity.**
  `SpatialIndex::by_alias`/`search` answer what a *user* types (S1's alias index);
  `ProviderBridge::resolve(type, key)` answers what another *record* names — a pid, an interface
  name or index, a unit name, a uid, a container's full or short id, a path, a socket inode. It
  walks `SpatialType::is_a`, so a reference to a `Socket` reaches the `Listener` it is. Neither is
  the `SpatialId`, which stays opaque.
- **A neighborhood group may already be refused before ranking sees it.**
  `SpatialIndex::relation_summary` returns a `withheld` group with one of §35.2's six states and
  the provider's own message wherever a field carried an error, and `total()` is `None` there —
  so `near`/`look` must render the state, never fall back to a count. `SpatialIndex::withheld(id)`
  lists them. Three places are composed rather than served — `Endpoint`, `Cgroup`, `Namespace` —
  and are ordinary index entries with real identities (ADR-0135); `up` from a file follows the
  Unix path tree through `ono_spatial_core::PATH_PARENT`, not a relation.

**S3 — index queries, selector resolution, `find place`, neighborhood, pins — is complete
(2026-08-28, agent `S3`).** Five commits, gate green on each; `scripts/acceptance.sh` 68 passed,
0 failed:

1. `feat(parser)` a words-mode option whose value is a predicate expression (ADR-0138), declared
   in `docs/spec/language.yaml` and held against the parser by `spec-check`.
2. `feat(spatial)` `crates/ono-spatial-query` — §27.1 selector resolution, §27.2 ambiguity, §27.3
   fuzzy that never acts, §3.6 neighborhood ranking, §6.8 search, §34 cost-aware planning
   (ADR-0139).
3. `feat(spatial)` `find place` — the contract (`place` target, `ono.spatial-place/1`,
   `docs/spec/commands/spatial.yaml`) and the implementation in `ono-cli` (ADR-0140, ADR-0141),
   **with the ADR-0124 rewrite of every Table 3 site in the same commit**.
4. `feat(spatial)` pins that outlive the session — `$XDG_STATE_HOME/ono/pins.json` (§46.1).
5. `test(acceptance)` `docker/acceptance/cases/101-spatial-find-place.case`, the S3 gate in the
   container.

Green from `crates/ono-cli/tests/spatial_navigation_missing.rs`:
`should_stream_places_with_scope_and_provenance_when_find_searches_with_a_predicate`,
`should_compose_with_the_v02_pipeline_when_a_find_result_is_filtered_and_counted`,
`should_run_the_native_spatial_find_and_keep_the_external_find_reachable_when_both_exist`. The
other 40 new outcome tests live in the crates:
`crates/ono-spatial-query/tests/{resolution,neighborhood,search}.rs` (34),
`crates/ono-cli/tests/spatial_pins.rs` (6) and `crates/ono-parser/tests/parse_commands.rs` (3).

**What S4a needs from S3** — the four things:

- **`near`'s ranking is `ono_spatial_query::neighborhood_of(index, center, request, pins, now)`.**
  It returns S1's `Neighborhood` — `groups`, `landmarks`, `hidden_count`, `generated_at`,
  `completeness` — already bounded and ranked. `NeighborhoodRequest` carries §6.2's five options
  (`along`, `of_type`, `changed_within`, `limit`, `all`) plus `in_terminal_rows` for §3.6's
  terminal-size input. S4a builds the *command* and the record shape; it must not re-rank, and it
  must render a withheld group's §35.2 state rather than a count (`total() == None`).
- **`look`'s view is that neighborhood plus a place.** The place record is
  `ono.spatial-place/1` (ADR-0140) — `spatial_id`, `name`, `display_name`, `object_type` (the
  v0.2 schema), `spatial_type`, `place_path`, `scope`, `parent`, `freshness`, `observed_at`,
  `identity_tier`, `capabilities`, `pinned`, `provenance` — built by
  `crate::spatial::find::place_record`, which S4a should lift out of `find.rs` when `look` needs
  it too. A place carries no `pid`, `cpu` or `state`: those are the object's, one `inspect` away.
- **A place a user typed is `ono_spatial_query::resolve(index, selector, context, now)`.**
  `Resolution::require(selector)` turns it into the place or into §40's structured refusal —
  `spatial.ambiguous_selector` with §27.2's three columns, or `spatial.not_found` whose help
  lists the near misses. `SelectorContext::at(current_place)` is what makes steps 1 and 2 of
  §27.1 (visible child, visible neighbour) mean anything, so S4a must pass the session's current
  place, not `anywhere()`.
- **The index is built per command and thrown away (ADR-0141).** `find place` asks only the
  provider targets its query needs (`ono_spatial_query::discovery::targets_for`). S4a owns the
  step that changes this: §46's `SpatialSessionState` holds the current place *and* the index, so
  `look` twice reads the index rather than the providers, which is what §34's budget and S4e's
  `should_answer_repeated_looks_far_inside_the_look_budget` need. `spatial::local_scope()` gives
  the host and boot every observation belongs to (§10.2).

**Not S3's, and still open:**

- Canonical spaces are not answered by `find place`: it searches the index, and a space is
  declared geography rather than an observed object. If a later phase wants `find place compute`
  to answer the domain, that is a decision for it to record.
- ~~The `argument_mode`-versus-ADR-0009 check in `xtask::contracts::check_commands` is dead
  against this repository's own `docs/spec/language.yaml`.~~ **Repaired** by `harness` on
  2026-08-28 (ADR-0159): `expression_heads()` reads the sequence of named modes the registry
  actually writes, an empty declaration is now reported instead of short-circuiting the check,
  and the fixture is written in the registry's shape so it can no longer certify a blind reader.
  Exit test:
  `xtask/tests/contracts.rs::should_reject_an_argument_mode_that_disagrees_with_the_grammar_this_repository_declares`.

**Open, and deliberately not S1's:** `docs/ACCEPTANCE.md` has no v0.4 section yet, so
`scripts/release-check.sh` cannot see this tranche. §4.7 needs writing from v0.4 §52 before S11,
the way §4.6 was written from v0.3.

**S4a — `look`, `near`, and the six domains as real places — is complete (2026-08-28, agent
`S4a`).** Four commits, gate green on the last; `scripts/acceptance.sh` 69 passed, 0 failed:

1. `fix(command)` a bare flag followed by another option was dropped — `get dir --all
   --recursive` set only `--recursive`. A pre-existing defect, found because `look --all --json`
   hit it; fixed red-first in its own commit.
2. `feat(command)` an option whose value is optional (ADR-0144), so `look --changes [duration]`
   and `near --changed [duration]` are spelled as §6.1 and §6.2 write them.
3. `feat(spatial)` the commands themselves: contracts in `docs/spec/verbs.yaml` and
   `docs/spec/commands/spatial.yaml`, seven new schemas, `crates/ono-spatial-render`,
   `SpatialSessionState` in `ono-cli`, and `look`/`near`/`enter`/`home` (ADR-0142, ADR-0143,
   ADR-0145).
4. `test(acceptance)` `docker/acceptance/cases/102-spatial-look-near.case`, the S4a gate in the
   container: 48 assertions, none of which types a name the shell has not printed first.

Green now, all previously `#[ignore]`d — 32 tests:

- `spatial_topology_missing` (20): `should_report_the_system_root_as_the_current_place_when_home_runs`,
  `should_list_exactly_the_six_canonical_domains_when_looking_at_the_system_root`,
  `should_carry_a_permission_state_on_every_domain_so_an_unavailable_one_stays_visible`,
  `should_bound_the_root_horizon_instead_of_listing_every_known_object`,
  `should_describe_the_current_place_with_an_id_kind_name_scope_and_permission_when_looking`,
  `should_keep_the_same_spatial_id_for_the_root_across_separate_sessions`,
  `should_enter_every_canonical_domain_when_named_at_the_root`,
  `should_offer_the_{compute,network,storage,identity}_groups_the_spec_names_when_entering_*`,
  `should_keep_containers_and_devices_enterable_with_a_state_when_no_provider_contributes`,
  `should_show_the_users_the_user_provider_answers_for_when_entering_identity_users`,
  `should_show_the_mounts_the_mount_provider_answers_for_when_entering_storage_mounts`,
  `should_show_a_block_device_the_device_provider_answers_for_when_entering_devices`,
  `should_bound_the_neighborhood_and_count_what_it_hides_when_a_place_has_many_neighbors`,
  `should_expose_a_reason_on_every_landmark_when_a_place_reports_landmarks`,
  `should_stream_neighbors_as_pipeline_objects_when_near_runs_at_the_root`,
  `should_distinguish_an_unavailable_group_from_an_empty_one_when_a_domain_has_no_provider`,
  `should_resolve_find_as_a_spatial_verb_while_the_external_tool_stays_reachable_by_path`.
- `spatial_navigation_missing` (5): the three `look` tests, `should_bound_the_neighborhood_to_the_requested_size_when_near_is_limited`,
  `should_run_the_native_spatial_look_and_keep_the_external_look_reachable_when_both_exist`.
- `spatial_map_missing` (3): the three §24 tests —
  `should_describe_identity_state_exits_and_landmarks_when_look_json_reports_a_place`,
  `should_mark_a_group_as_an_exit_only_when_it_can_be_entered_when_look_lists_groups`,
  `should_not_invent_a_change_section_when_no_snapshot_or_event_source_exists`.
- `spatial_relationships_missing` (1): `should_keep_the_current_place_when_trace_projects_the_relationship_graph`.
- `ono-command::binding` (1, new): `should_bind_both_flags_when_one_bare_flag_follows_another`.

**What S4b needs from S4a** — the four things:

- **`enter` is already dispatched in two places, and both move the place.**
  `crate::context::claims` sends `enter` to the v0.2 context stack only when its first word names
  a target `docs/spec/commands/` declares for `enter` (`dir`, `process`, `service`, `user`, …);
  anything else — a domain, a collection, a pid, a quoted spatial id, or no argument at all —
  reaches `crate::spatial::commands::Enter`, the target-less `ono.place.enter`. §30.2 applies to
  both spellings, so `context::enter_record` now also calls `crate::spatial::enter_observed`
  (ADR-0142). S4b owns `enter @<result-ref>`, `enter .` and the piped form of §28.2.
- **`enter <object>` needs the object in the index first.** `Enter` resolves against what is
  known, and observes the current place's surroundings only when the declared answer misses. That
  is why `enter <pid>` at the root still refuses: the root observes the container and device
  targets and nothing else (ADR-0143's source table). The step S4b owns is planning the targets a
  *selector* implies — `ono_spatial_query::discovery::targets_for` already does exactly that for
  a predicate — so `enter 1842` from anywhere reaches the process.
  `spatial_navigation_missing::should_stream_neighbors_that_compose_with_the_pipeline_when_near_runs_in_a_script`
  is the near test that waits on it.
- **The trail is recorded, and nothing reads it yet.** Every move — `enter`, `home`, and the v0.2
  `enter <target>` — records a `NavigationStep` with its `Movement` in
  `SpatialSessionState::trail_mut()`. `follow` records `Movement::Follow` with the relation;
  `back`, `up` and `trail` (S4c) read what is already being written.
- **A place view is one function.** `crate::spatial::view::place_record` builds every
  `ono.spatial-place/1` the shell emits — `look`, `near` and `find place` — so a field `follow`
  needs is added once. `view::neighborhood_here` decides which of the two projections applies: a
  canonical space gets `ono_spatial_query::space_neighborhood` over observed exits, an object
  gets `neighborhood_of` over its edges. `follow <relation>` reads the second.

**Left ignored, and why** (S4a's assignment ends here):

- `spatial_topology_missing`: `should_answer_look_near_and_map_without_an_object_name_when_at_the_root`
  (all-or-nothing, and two of its six scripts are `map` — S5);
  `should_reach_a_process_it_never_names_…`, `should_offer_the_process_exits_…`,
  `should_follow_the_parent_relation_…`, `should_discover_a_listening_socket_…`,
  `should_reach_a_running_service_…` (all need `enter @-1` or `follow` — S4b); the two completion
  tests (§9.4, PTY — S6).
- `spatial_navigation_missing`: everything that needs `enter <object>`, `follow`, `jump`, `back`,
  `up`, `trail` or `map`.
- `spatial_map_missing`: everything about `map` (S5).

### S9 + S10 + ADR-0191, the last six tests of the tranche (2026-08-28, agent `S9S10`)

**Nothing in the nine spatial suites carries `#[ignore]` any more.** The six tests the table above
listed are delivered and green; `grep -rn '#\[ignore' crates/*/tests/*.rs` finds nothing.

- **S9 — KUANG/11 spatial extensions (§36, ADR-0194).** A package's `contributions.relations`
  shapes now register real relations in its own namespace, gated by `relation.write`: without the
  grant nothing is contributed, so §35.5's "filter before merging" holds by construction. A
  package asserts its edges as data — the new canonical schema `ono.spatial-relation/1`, answered
  by a contributed command whose target is `spatial-relation` — and the host resolves both ends
  through the canonical provider, so a package can say two objects are related and can never say
  an object exists. Every contributed edge carries the package as its provider and its `origin`
  and a §11.5 confidence the host never raises to `exact`.
  Files: `crates/ono-spatial-core/src/relation.rs` (the contributed registry),
  `crates/ono-cli/src/spatial/contributions.rs` (new), `crates/ono-cli/src/spatial/map.rs`
  (the merge into the horizon), `crates/ono-cli/src/plugins.rs` (adopt at load),
  `crates/ono-spatial-query/src/map.rs` (`--relations` accepts a contributing package),
  `crates/ono-kuang-sdk/src/bin/kuang-example-plugin.rs`,
  `crates/ono-kuang-testhost/{src/lib.rs,tests/spatial_package.rs}`.
- **S10 — v0.3 adapter reconciliation (§37, ADR-0193).** An adapted record never mints an identity
  the canonical provider would not: `enter` resolves it through the provider first, so
  `ps … | enter process` stands where `enter process <pid>` stands. A place keeps every source
  that observed it, exposed as `sources` on `ono.spatial-place/1` and `ono.spatial-neighbor/1`, so
  `inspect` on `lo` names `linux.netlink` **and** `adapter:org.ono.compat.iproute2.ip-link`. A
  whole-document adapter's batch is offered to the index; a stream is not buffered to index it.
  `… | enter` on bytes is `spatial.not_enterable`.
- **ADR-0191 — one `enter`, one refusal.** A failed `enter` is `spatial.not_found`
  (`Ono-Sendai-E1001`) in both grammars; `resolve.target_not_found` keeps every other job. Four
  assertions were adjusted to the new spelling and named in the commit body:
  `identity_missing::should_refuse_to_enter_a_user_that_does_not_exist`,
  `network_missing::should_refuse_to_enter_an_interface_that_does_not_exist`,
  `processes_missing::should_refuse_to_enter_a_process_that_does_not_exist`, and
  `docker/acceptance/cases/044-remote-links-as-objects.case` (`enter link` after `remove link`).
- **The TIME_WAIT flake above is fixed (ADR-0192)**, and it was a product defect rather than a
  fixture one: a socket in `time-wait` or `close` has no inode — `ono.socket/1`'s identity — so
  the index registered the kernel's 2MSL remnant as a *second* connection beside the one that had
  just ended, and `map` carried two nodes for one connection (the duplicate §37.1 and §42.1
  forbid). A released socket now has no place at all; `get socket` still lists it with its state.
  `spatial_relationships_missing::should_show_the_connection_edge_appear_and_vanish…` failed two
  runs in three before, and is green in four consecutive runs of its file after, unchanged.
- **Acceptance:** `docker/acceptance/cases/110-spatial-contributions.case`, 13 assertions
  (`s9-a`–`s9-g`, `s10-a`–`s10-f`). The two §4.7.1 boxes for §36 and §37 are ticked.

**Next up from this increment:**

- A contributed relation is an edge on the map and in the index, and is not yet a navigable exit:
  `look` does not print it and `follow`/completion do not offer it (ADR-0194 §Consequences). Exit
  test: `follow <contributed relation>` from a place the package's edge starts at moves there.
- `spatial_topology_missing::should_complete_the_relations_available…` still fails under parallel
  load and passes alone; the PTY budget in it is the fixture problem S6 recorded, not this.

**Found, not fixed, and deliberately outside this increment:**

- `network/addresses`, `compute/cgroups` and `network/namespaces` report `unsupported`: no v0.2
  provider target serves an address, a cgroup or a namespace as an object, although the bridge
  composes cgroups and namespaces from process records (S2, ADR-0135). Composing the collections
  from the same facts is a real increment, and §7.3 only requires the place to exist and to say
  what it could not tell. `storage/directories` reports `unknown — available on request`, because
  §33.3 makes the filesystem query-driven; S4d owns storage and the cwd distinction.
- `ono.system/1` is declared from §7.1 field for field, and `look --all` at the root carries it
  with `os`, `kernel` and `uptime` null: no provider answers for them, and §2.16 forbids the
  spatial layer from reading them itself. A `get system` producer fills them.
- `spatial_topology_missing::should_show_the_mounts_the_mount_provider_answers_for_when_entering_storage_mounts`
  compares two separate `ono` runs against a live mount table. On a workstation where Docker is
  creating and removing netns mounts it can lose the race and see a mount the first run did not.
  Seen once, passing on re-run and green in the container; the test is right and the environment
  is what moved.

**S4b — `enter` on any place, and `follow` along a real edge — is complete (2026-08-28, agent
`S4b`).** ADR-0146 to ADR-0149; gate green; acceptance case
`docker/acceptance/cases/103-spatial-enter-follow.case` added (31 assertions).

The increment turned on the thing S4a left dark: **an object place had no exits**. `near` at a
process answered nothing, because the only source of relationship edges was the record-field
bridge, which reads a `ppid` and a `cgroup` and cannot know which files a process holds open.
ADR-0146 makes the edges of an object place the ones the **v0.2 relationship providers** of
`ono-graph` assert about that object — the same providers `trace` walks — translated into the
declared relations of `docs/spec/spatial/relations.yaml`. A neighbour therefore reports the
relation word and the provider id `trace` reports for the same edge (§2.16, §31.3), and the
record-field bridge keeps only the relations no relationship provider serves (cgroup, namespace,
container, and the listener a connection was accepted by).

**What S4c needs to know** — the five things:

- **The trail is written and still unread.** Every movement records a `NavigationStep`:
  `enter`/`home` from S4a, and now `follow` with `.along(relation)` — §6.4's "the relation
  traversed MUST be recorded". `back`, `up`, `trail` and `jump` read what is already there.
  `crates/ono-cli/tests/spatial_relationships_missing.rs::should_record_the_relation_it_traversed_when_a_follow_enters_the_trail`
  is the test waiting on `trail --json`, and
  `spatial_topology_missing::should_follow_the_parent_relation_from_a_discovered_process_to_its_spawner`
  is green up to its last statement, which is `trail --json`.
- **`up` is `place.canonical_parent`, and it is already on every place view.** The place record
  carries `canonical_parent` (§11.3, §33.1) and no longer carries a second `parent` field
  answering the same question (ADR-0148). `ono_spatial_query::resolve::parent_of` computes it.
  `spatial_identity_missing::should_move_to_the_declared_canonical_parent_deterministically_when_going_up`
  asserts `up` lands on exactly that id and that `follow parent` lands somewhere else.
- **Resolution and observation are one function.** `crate::spatial::commands::resolved_place`
  resolves a selector the way §27.1 orders it and, when nothing visible answers, plans the
  provider targets the *selector* implies and asks those. `jump` is the same resolution with
  §27.1's step 6 allowed (`SelectorContext::across_links`) and a `Movement::Jump` step; it needs
  no new observation machinery.
- **A place view is still one function**, and it now carries what a movement needs to be checked
  from outside: `canonical_ref`, `lifetime`, `state`, `summary`, the `exits` map keyed by the
  word `look` prints, and the object's own identity fields at the top level, so
  `look --json | from json | where pid == 1842` is an ordinary pipeline (ADR-0148).
- **A refusal prints its dotted name.** `ono: Ono-Sendai-E1006 spatial.history_empty …` — the
  renderer shows both halves of §43's identity now, so `back` at an empty trail and `up` at the
  root are distinguishable in a terminal as well as in `catch e { $e.name }` (ADR-0148).

Green now, all previously `#[ignore]`d — 39 tests:

- `spatial_relationships_missing` (9): `should_enter_the_open_file_when_following_it_from_the_holding_process`,
  `should_name_the_holding_process_among_the_file_neighbors_when_the_file_is_the_place`,
  `should_name_the_same_relation_and_provider_as_trace_when_the_neighbor_is_the_open_file`,
  `should_enter_the_listening_socket_when_following_it_from_its_owner_process`,
  `should_reach_the_accepted_connection_when_following_it_from_the_listening_socket`,
  `should_refuse_the_traversal_with_no_relation_when_the_process_owns_no_socket`,
  `should_refuse_to_follow_a_canonical_child_that_is_not_a_relationship_edge`,
  `should_bound_the_neighborhood_by_default_and_widen_it_with_all`,
  `should_report_the_unreadable_namespace_group_as_unknown_rather_than_absent`.
- `spatial_navigation_missing` (8): the two `enter` tests, `should_traverse_the_relationship_edge_when_following_the_parent_relation`,
  `should_answer_no_relation_when_following_an_edge_the_current_place_does_not_have`,
  the three ambiguity tests, `should_leave_the_callers_place_untouched_when_a_called_script_navigates`.
- `spatial_identity_missing` (11): the four identity tests (287, 313, 334, 362), the three
  permission-honesty tests, `should_keep_every_relationship_parent_while_naming_one_canonical_parent`,
  `should_carry_source_provenance_and_confidence_on_every_relationship_edge`,
  `should_use_the_defined_confidence_vocabulary_and_never_call_an_inferred_edge_exact`,
  `should_expose_how_fresh_the_data_behind_a_place_is`.
- `spatial_storage_missing` (9): the six §30 tests (cwd, place, `cd`, `PWD`), the two §44.3
  walking tests, `should_refuse_a_path_that_does_not_exist_with_a_structured_error`.
- `spatial_topology_missing` (1): `should_discover_a_listening_socket_by_its_port_and_follow_it_to_its_owning_process`.
- `spatial_contracts_missing` (3): `should_refuse_an_unknown_place_with_a_structured_spatial_error`,
  `should_serve_every_relation_it_declares_and_declare_every_relation_it_serves`,
  `should_report_denied_information_as_denied_rather_than_as_an_empty_collection`.

**Left ignored, with the reason on the test** (each carries it in its `#[ignore]` line):

- `spatial_topology_missing::should_reach_a_process_it_never_names_…` and
  `…should_offer_the_process_exits_…` — **delivered and green with `--test-threads=1`.** The
  fixture selects its process with `ppid == std::process::id()`, and under cargo's default
  parallelism that also matches the children every other test in the same binary spawned, so the
  discovery walk reaches one of theirs. The fixture needs a predicate unique to itself.
  *(Fixed 2026-08-28 by agent `fixtures` — see below.)*
- `spatial_topology_missing::should_follow_the_parent_relation_…` — the `follow parent` half is
  green; the test's last statement is `trail --json` (S4c).
- `spatial_topology_missing::should_reach_a_running_service_…` — **the test and the inherited
  v0.2 contract disagree.** It selects with `--where state == "running"`, and `ono.service/1`
  declares `state` as `active | reloading | inactive | failed | activating | deactivating |
  unknown` and reports `running` as the *substate*. No service on a systemd host answers to it.
  In the acceptance container there is no service manager and the test takes its skip branch.
  *(Resolved 2026-08-28 by ADR-0167 — see below.)*
- `spatial_contracts_missing::should_refuse_an_ambiguous_selector_in_a_script_…` — the ambiguity
  path is delivered, but the fixture copies `/bin/sleep` to a new name and runs it twice; on a
  host whose coreutils is a multi-call binary (Ubuntu 25.10) the copy refuses to start
  (`coreutils: unknown program 'ono-spatial-twin'`), so nothing answers to the name and the
  refusal is `spatial.not_found`. The fixture needs a program it can rename.
  *(Fixed 2026-08-28 by agent `fixtures` — see below.)*
- everything that needs `back`, `up`, `trail`, `jump`, `pin` (S4c), `map` (S5), the mount
  boundary and the directory summary (S4d), or tombstones (S7).

**The three fixture-blocked v0.4 tests are delivered (2026-08-28, agent `fixtures`).** No
assertion was weakened; only the fixtures were corrected, per AGENTS.md §11. One commit,
ADR-0167.

- `spatial_topology_missing::should_reach_a_process_it_never_names_…` and
  `…should_offer_the_process_exits_…` — `SleepChild::selector()` now spells
  `ppid == <test pid> and pid == <child pid>`. Parentage alone matched every other test's `ono`
  shells in the same binary; the child's own pid is known to the fixture, and §9's "discovery
  without prior names" forbids naming the *object* (its command name), not pointing at one's own
  fixture. The walk is still `find place` → `enter @-1` → `look`. The same selector now serves
  `should_follow_the_parent_relation_…` and the `follow` completion test, which carried the same
  latent race.
- `spatial_contracts_missing::should_refuse_an_ambiguous_selector_…` — the twins are now a
  **symlink to `/bin/sh`**, not a copy of `/bin/sleep`. Two facts fix the fixture: the kernel
  takes `comm` (the `name` of `ono.process/1`) from the basename of the path handed to `execve`,
  symlink included, and it truncates it to 15 characters — hence `ono-twin-place`, not
  `ono-spatial-twin`. A *copy* additionally loses to `ETXTBSY` under parallelism, because a
  concurrent test's `spawn` inherits the copy's write descriptor across `fork`; a symlink leaves
  no descriptor. Each twin is `sh -c 'read line'` on a pipe the test holds, and the test waits
  for `/proc/<pid>/comm` before asking the shell to resolve the name.
- `spatial_topology_missing::should_reach_a_running_service_…` — ADR-0167: a running service is
  `state == "active" and substate == "running"`, held in the suite's `RUNNING_SERVICE` constant.
  `running` is a *substate* in `ono.service/1`, never a `state`; requiring both also keeps
  `active`/`exited` oneshots and `active`/`plugged` `.device` units — which have no process for
  §44.2's "follow one of its processes" — out of the selection.
- Proof: each file run ten times in a row under cargo's **default** parallelism, 10/10 green,
  in a clean worktree at `da26bba` carrying only these fixture changes.

**Found, not fixed, and deliberately outside this increment:**

- `process.connects_to` is declared and nothing serves it: the v0.2 graph reports a process's
  sockets, not its endpoints, and the endpoint at the far end is the *socket's* `peer`. The exit
  answers `unsupported`, which §35.2 makes a real answer; removing the relation or serving it is
  a decision for whoever writes the endpoint provider.
- `interface.has_address` has the same shape: `network/addresses` has no provider target, so an
  interface's `addresses` exit is `unsupported` rather than a list.
- A file place's `owner` is `unknown — available on request`: `user.owns_file` is
  `CostClass::Expensive` and no user record is observed at a file place. Loading it on
  `near --type user` is one line in `relations::adjacent_targets` and one test.
- **`cargo test` retains no results between statements of one `-c` script until now.**
  `stage_scope` did not populate `Scope::previous`, so `@-1` in a command argument resolved to
  null while `@-1` at the head of a pipeline worked. Fixed here because `enter @-1` is §28.2;
  every other command that takes a value argument gains the same reference.

**S4c — movement through history and hierarchy (`back`, `up`, `home`, `trail`, `jump`,
`pin`/`unpin`) — is complete (2026-08-28, agent `S4c`).** ADR-0150 to ADR-0153; acceptance case
`docker/acceptance/cases/104-spatial-back-up-home-trail.case` added (25 assertions).

The increment turned the trail from something written into something read, and fixed the one rule
that made §44.6 undemonstrable. Six commands are new — `back`, `up`, `jump`, `trail`, `pin`,
`unpin` — with their contracts in `docs/spec/commands/spatial.yaml`, their verbs in
`docs/spec/verbs.yaml` and one new schema, `ono.navigation-step/1`.

**What the next phases need to know:**

- **`trail` answers `ono.navigation-step/1`** (ADR-0150). §20.1's six fields, plus `from_ref`/
  `to_ref` (the `<type>/<key>` spelling a user can type back), `from_name`/`to_name`, `relation_id`
  beside the `relation` *word*, and `host`. `trail` streams the records, `trail --json` writes them
  as one array, `trail --compact` writes §20.2's breadcrumb. **S8** will need `host` to become
  per-step rather than the session's — it is set in one place, `movement::step_record`.
- **`scope_crossing` is already recorded and already rendered.** Every `jump` and `up` compares the
  scope of both ends and records the boundary where they differ, as a record with `kind`, `from`,
  `to`, `entering` and `remote`. **S4d**'s mount boundary (§44.3) and **S8**'s host boundary both
  need only the two ends to carry different scopes; nothing in the trail has to change.
- **A socket's canonical parent is `network.listeners`, not the process that owns it**
  (ADR-0151, a fix). The S1 rule chain made `up` from a socket land on the same place as `back`,
  which is precisely the distinction §44.6 exists to demonstrate. `parent_rules(Listener)` and
  `parent_rules(Connection)` are now empty and fall through to the collection space;
  `docs/spec/providers/linux-netlink.yaml` declares the same chain, because `spec-check` compares
  them. A socket's `place_path` is therefore `local/network/listeners`.
- **`still_a_place` in `crates/ono-cli/src/spatial/movement.rs` is the seam S7 needs** (ADR-0152).
  §20.3's four outcomes are all implemented — return, skip-with-a-notice, `spatial.destination_gone`,
  `spatial.history_empty` — behind one predicate that today answers "the session still knows this
  place". A tombstone makes that predicate answer differently and makes `back` return the tombstone.
- **A pin stores the place's *name* as its selector, plus its type** (ADR-0153). `jump @<pin>` reads
  what `with_pins` already resolved; a pin whose place is gone is `spatial.destination_gone` and
  stays in the store. **S5**'s landmark engine gets `user_pinned` from the same registry the query
  layer already ranks by; nothing new is needed there.

Green now, all previously `#[ignore]`d — 19 tests:

- `spatial_navigation_missing` (9): `should_move_across_scopes_and_record_both_ends_when_jumping_to_a_resolved_place`,
  `should_return_to_the_process_when_back_follows_the_navigation_history`,
  `should_move_to_the_network_hierarchy_parent_when_up_follows_the_canonical_hierarchy`,
  `should_return_to_the_system_root_when_home_runs_after_deep_navigation`,
  `should_answer_history_empty_when_back_runs_with_no_previous_place`,
  `should_answer_no_parent_when_up_runs_at_the_system_root`,
  `should_record_every_movement_with_its_kind_and_relation_when_the_trail_is_read_as_json`,
  `should_answer_not_found_when_a_navigation_argument_names_nothing`,
  `should_start_at_the_system_root_with_an_empty_trail_when_a_new_session_begins`.
- `spatial_contracts_missing` (4): `should_refuse_to_go_back_or_up_from_the_root_with_a_named_spatial_error`,
  `should_start_every_session_at_the_local_system_root`,
  `should_keep_a_scripts_navigation_out_of_the_callers_place`,
  `should_keep_the_trail_session_local_while_a_pin_survives_the_session`.
- `spatial_relationships_missing` (3): `should_return_to_the_process_with_back_after_following_a_socket_edge`,
  `should_leave_the_relationship_chain_with_up_after_following_a_socket_edge`,
  `should_record_the_relation_it_traversed_when_a_follow_enters_the_trail`.
- `spatial_identity_missing` (2): `should_move_to_the_declared_canonical_parent_deterministically_when_going_up`,
  `should_not_confuse_the_old_and_the_new_process_when_a_place_is_replaced`.
- `spatial_topology_missing` (1): `should_follow_the_parent_relation_from_a_discovered_process_to_its_spawner`
  — its last statement was `trail --json`.

**One assertion changed, with ADR-0151 in the same commit.**
`spatial_navigation_missing::should_move_to_the_network_hierarchy_parent_when_up_follows_the_canonical_hierarchy`
built its haystack from `display_name` and `scope`; under ADR-0140 the field that names the
canonical location is `place_path`, and `scope` is the §3.2 boundary (`host:web01`). `place_path`
is now in the haystack. What the test demands is unchanged: `up` lands under NETWORK and is not
where `back` lands.

**Left ignored, with the reason on the test:**

- `spatial_identity_missing::should_return_the_tombstone_and_keep_the_trail_record_when_back_points_at_a_dead_place`
  — `back` returns to the recorded place and the trail keeps the record, but the test also demands
  that the place say it is dead, which is S7's tombstone (§10.3). Attempted and left.

**Found, not fixed, and outside this increment:**

- **`up` from a file place answers `spatial.no_parent`.** `parent_rules(File)` is `[path.parent]`,
  and `path.parent` is only supplied by `canonical_parent_with`, which `resolve::parent_of` does not
  call because only the caller knows which directories have been observed. §15.1 makes the enclosing
  directory a file's parent, so this is a real gap and it is **S4d's**: it needs the directory
  observed, which is the same query §15.4 and §44.3 need anyway.
- `docs/spec/schemas/file.v1.yaml` gives a file the identity `[device, inode]`, so a trail step's
  `from_ref`/`to_ref` for a file reads `file/0:46`. It is honest — that *is* the provider's
  reference — but it is not a spelling anyone types. Whoever gives `ono.file/1` a path-shaped alias
  fixes the trail's readability for free.

**S5 — semantic maps, the landmark engine and the ASCII fallback — is complete (2026-08-28, agent
`S5`).** ADR-0162 to ADR-0166; gate green; acceptance case
`docker/acceptance/cases/105-spatial-map.case` added (55 assertions).

Delivered:

1. `crates/ono-spatial-query/src/map.rs` — the `SpatialMap` projection: §23.1's ranking, §8.1's
   five zoom levels, §8.2's clustering, §8.3's expansion, the §34.2 budgets and the §6.9 filters.
   It is handed a *horizon* by the shell and asks no provider anything (§45.3, §2.16).
2. `crates/ono-spatial-query/src/landmark.rs` — **the landmark engine §50 assigns to no phase**
   (ADR-0163). Eight of §3.7's fourteen reasons are produced from real provider fields; the other
   six are documented absences, not silent branches.
3. `crates/ono-spatial-render/src/map.rs` — the default textual map of §23.2 as a ranked tree,
   width-aware, with the ASCII fallback §39.2 requires (ADR-0166).
4. `crates/ono-cli/src/spatial/map.rs` — the `map` command, its contract in `docs/spec/verbs.yaml`
   and `docs/spec/commands/spatial.yaml`, and five new schemas: `ono.spatial-map/1`,
   `ono.map-node/1`, `ono.map-edge/1`, `ono.map-cluster/1`, `ono.hidden-summary/1` (ADR-0162).
5. `spatial.map.node_budget`, `spatial.landmarks.*` and `spatial.look.change_window` are now
   *read* — `crate::spatial::configure_from` hands the session what the user configured, which is
   what makes §26.3's "inspectable and configurable" true rather than advertised.

Green now, all previously `#[ignore]`d — 26 tests:

- `spatial_map_missing` (21 of the 24; the three §24 tests were already green): the six §22
  contract tests, the two §43.2 filter tests, the four §8 zoom and cluster tests, `--focus`, the
  three landmark tests, and the three §23.2/§39 rendering tests.
- `spatial_contracts_missing` (2): `should_serve_exactly_the_canonical_spaces_the_registry_declares`,
  `should_bound_the_default_map_to_its_node_budget`.
- `spatial_navigation_missing` (1): `should_answer_a_bounded_graph_when_map_json_runs_without_a_tty`.
- `spatial_topology_missing` (1): `should_answer_look_near_and_map_without_an_object_name_when_at_the_root`.
- `spatial_identity_missing` (1): `should_resolve_every_edge_endpoint_to_a_node_or_an_explicit_off_map_endpoint`.
- `spatial_relationships_missing` (1): `should_explain_every_edge_with_relation_provider_and_confidence_when_mapping_a_process`.
- 17 new crate-level outcome tests: `crates/ono-spatial-query/tests/{map,landmarks}.rs`.

**Left ignored, with the reason on the test** (both in `spatial_map_missing`):

- `should_show_more_than_the_default_when_the_map_is_asked_for_all` — **its two halves contradict
  each other and the contracts suite.** The first (`--all` is strictly larger than the default) is
  delivered and green. The second asks that `--all` at a 300-process collection contain one
  particular freshly spawned process; `spatial_contracts_missing::should_bound_the_default_map_to_its_node_budget`
  requires `--all` to stay inside `spatial.map.node_budget` (100) and §34.2 prohibits unbounded
  rendering, so only a clock-relative ranking could reach it — and that makes the two §43.2 filter
  tests compare two maps of two different moments and fail. ADR-0165 carries the analysis under a
  `Spec deviation` heading. Reconciling it needs either a second, larger explicit bound or a
  fixture the map is guaranteed to rank in.
- `should_yield_exactly_the_members_and_keep_the_place_when_a_cluster_is_expanded` — **delivered
  and green with `--test-threads=1`.** It compares a cluster's member count from one `ono` run
  against the nodes a second run draws, and every sibling test in the binary spawns and reaps
  twelve processes between the two, so the collection it counts is a different size each time.
  Same family as the two topology fixtures S4b left.

**What S6 needs from this renderer** — the four things:

- **The seam is `spatial_map`'s input, not its output.** `ono_spatial_render::spatial_map(record,
  width, charset)` is the whole text projection; the full-screen view of §23.3 takes the same
  `ono.spatial-map/1` record — already ranked, bounded and clustered by `ono-spatial-query` — and
  adds a viewport, a cursor and the key bindings. It must not re-select or re-rank, or the two
  views will disagree about what the system looks like (§45.4, §49.5).
- **Focus is already a request, not a mode.** `MapRequest::focus(node)` goes in and
  `SpatialMap::focus` comes out beside `center`; moving the cursor is a new projection with a new
  focus and no movement of the place (§23.4). `Enter` on the focused node is `enter <id>`, which
  `crate::spatial::commands::resolved_place` already resolves.
- **The interactive budget is the same number.** §34.2's 100 nodes is `spatial.map.node_budget`,
  which `--all` already uses and `crate::spatial::configure_from` already reads.
- **Colour is S6's to add and no semantics may depend on it.** §39.1 lists six things colour must
  never be needed for; all six are carried by a word or a glyph today (`◆` for a landmark, `~~▸`
  for an inferred edge, the confidence word, the state word), and the ASCII/Unicode choice is
  `Charset`, decided from the locale and `TERM` in `crate::sink`.

**What S7 needs from this map projection** — the three things:

- **`live_capable` is `false` and says so honestly.** Nothing in this build subscribes to a
  provider event, so §25.1's live map has no source; S7 flips the field when it has one, and
  `map --live`/`map --changes` of §6.9 are declared in no contract until then.
- **`MapEdge.changed` and `MapNode` are ready for a change state.** The edge already carries a
  `changed` field (null today, §24.3 forbids inventing one), and the three change reasons of §3.7
  — `new_object`, `removed_object`, `connection_spike` — are exactly the ones ADR-0163 leaves
  undelivered because they are differences between two observations (§25.4), not facts about one.
- **Landmarks are recomputed on every `absorb`,** so a live update recomputes them for free; what
  S7 adds is the diff that makes a *change* visible, and the rule that a landmark asking for
  attention reorders the map while one that merely informs does not (ADR-0165).

**Found, not fixed, and deliberately outside this increment:**

- §26.2's high-memory rule cannot fire: no provider serves a host or cgroup memory budget, so a
  share cannot be computed and §2.16 forbids the spatial layer reading `/proc/meminfo` itself. The
  threshold setting stays inspectable. Same for the restart-loop rule: `ono.service/1` declares no
  restart count.
- §26.2 names four network rules — interface down, route change, unusually high traffic, new
  remote peer — that §3.7's closed reason vocabulary has no word for. A core landmark may not
  invent a reason (§3.7), so they are absent rather than approximated.
- Clustering has one dimension, the canonical collection (§8.2's first). A cluster by user, by
  cgroup or by container is a real increment with its own test; the dimension is already a field
  on `ono.map-cluster/1`, so adding one changes no contract.
- `map` honours `COLUMNS` even when stdout is redirected, which no other view does (ADR-0166).
  Whoever decides that the whole renderer should do the same has one function to change,
  `crate::sink::terminal_width`, and the table snapshots to re-check.

**S6 — the interactive spatial surface — is complete (2026-08-28, agent `S6`).** ADR-0173 to
ADR-0177; acceptance case `docker/acceptance/cases/107-spatial-interactive.case` added — 39
assertions driven through a real pseudo-terminal — and **the containerised suite stands at 73
cases green, 0 failed** (`scripts/acceptance.sh`, 2026-08-28).

Delivered — the phase, plus the four areas §50 assigns to nobody:

1. **§5 the startup horizon.** An interactive session runs `look` once before the first prompt
   and never in a pipe. It is `look`, not a second renderer of the root, so §49.5 cannot be
   broken by the two drifting apart (ADR-0175). `spatial.startup_horizon` and `spatial.enabled`
   each switch it off.
2. **§21 the prompt and the HUD.** `ono_spatial_query::resolve::concise_path` is §21.2's rule as
   a function — `local`, `local/compute`, `local/process/nginx` — and the prompt, the place
   view's heading and the map's header all read it. The working directory stays in the prompt
   beside the place, because §30 keeps them different state (ADR-0175).
3. **§23.3 the full-screen map.** `ono_spatial_render::view` holds the whole view model with no
   terminal in it: `MapView` (viewport, cursor, search, help, detail overlay), `Action` (§23.3's
   twenty-one semantic actions), `Keymap` (§23.3's table, overridable through the new
   `spatial.map.keys`), `Effect` (what is left for the shell to do). `crate::spatial::interactive`
   is the terminal side: alternate screen and raw mode as guards, resize, and the same
   `go_back`/`go_up`/`go_home` the commands call (ADR-0174).
4. **§27.2 the ambiguity picker,** interactive only; a script still gets
   `spatial.ambiguous_selector`. `Candidate` now carries the identity key, so §27.2's rows read
   `nginx/1842` and disambiguate rather than repeating one name three times — which also improved
   the non-interactive refusal (ADR-0177).
5. **§9.4 completion as spatial discovery.** `enter`/`jump`/`map <TAB>` offer the neighbourhood
   and `follow <TAB>` the relations this place actually has, with §9.4's compact count or §35.2's
   state word. Shown on the first Tab through the new `Completion::listing`; ordinary word
   completion is unchanged (ADR-0177).
6. **§25.1 `map --live`** as the explicit polling source §25.1 permits, saying `live polled` in
   §25.3's vocabulary, refused with `spatial.unsupported` where there is no terminal (ADR-0176).
7. **The seam that made all of it safe:** `ono_command::Invocation::displays` — the evaluator
   tells the last stage of a foreground statement that its values will be *seen*. `map | to json`,
   `map > file`, `$(map)` and `ono -c 'map'` therefore never open a screen (ADR-0173).

Green now, all previously `#[ignore]`d — 14 tests:

- `spatial_interactive_missing` (all 12): the horizon at a TTY and never in a pipe, `look` at 80
  and at 40 columns, the prompt following the place, the picker, the map opening and closing, focus
  that does not move the place, `back` at the prompt and Backspace in the view, Ctrl-C leaving the
  live map, resize preserving the place, `stty`/`pwd` in order afterwards, `enter <TAB>`.
- `spatial_topology_missing` (2): `should_complete_the_places_of_the_current_neighborhood_when_tab_follows_enter`,
  `should_complete_the_relations_available_from_the_current_place_when_tab_follows_follow`.
- 10 new crate-level outcome tests: `crates/ono-spatial-render/tests/view.rs`.

**Nothing was left ignored by this phase.**

**What S7 needs from this view loop** — the four things:

- **The loop is where a live update lands.** `crate::spatial::interactive::run_map_view` reads a
  terminal event with a timeout and, when the timeout expires and the view is live, rebuilds the
  projection and calls `MapView::redraw`. S7 replaces the *source* of that rebuild — an event
  subscription instead of a one-second poll — and changes nothing else: `redraw` already keeps the
  cursor on the node it was on, and `MapView::set_live(live, freshness)` already takes §25.3's
  word, so flipping `polled` to `event_driven` is one argument.
- **Nothing repaints unless the drawing changed.** The loop compares the new frame with the one on
  the screen and writes nothing when they are equal. That is what makes §39.4's `reduced_motion`
  true by construction today; when S7 adds a change highlight, `spatial.reduced_motion` is the
  switch that turns *that* off, and it is already read into the session (`configured_flag`).
- **`Effect` is the whole vocabulary between the view and the shell.** A new key means a new
  `Action` variant, a default binding and an `Effect`; the config syntax, the `?` help table and
  the key-name parser follow for free.
- **The map projection is one function.** `crate::spatial::map::projection(ctx, session, center,
  request, now)` builds every `ono.spatial-map/1` the shell emits — `map`, `map --json`, every
  frame of the full-screen view. A live diff belongs in what feeds it, never beside it.

**Found, not fixed, and deliberately outside this increment:**

- §5's "providers SHOULD populate expensive counts asynchronously and update the horizon when
  available" is a SHOULD this build does not do: the horizon is one synchronous `look`. It costs
  what `look` costs. Making it asynchronous needs the same update channel S7 builds for the live
  map, and belongs there — not in a second mechanism.
- `spatial.reduced_motion` is read and inspectable but has nothing to disable, because this
  renderer draws no animation at all (§25.2 forbids decorative motion). ADR-0176 says so; S7 gives
  it something to switch off.
- **§21.3's third marker has nothing to mark yet.** The section requires privilege, remote *and
  namespace* changes to be recognisable in a colourless terminal. Privilege is the ` root`
  segment and the `#` marker (v0.2 §17.2); remote is the link segment, which takes the host's
  name instead of `local` (§14.4). A container or namespace boundary cannot be shown because
  nothing produces a place in one: `ProviderBridge` projects every observation into the session's
  own host scope, so `ScopeKind::Container` and `ScopeKind::Namespace` exist in the model
  (`ono_spatial_core::scope`) and no place ever carries them. A marker written now could never
  fire. The prompt is one line away from it — compare the current place's scope with the
  session's and print `container:<id>` or `ns:<kind>/<id>` when they differ — and the increment
  that makes a container's processes carry the container scope is the one that should write it.
- The `/` search of §23.3 searches the *drawn* map. §23.3 says "search visible/global map"; the
  global half is `find place`, which already exists as a command, and wiring it into the view's
  search line is a real increment with its own test.
- Completion asks no provider anything (§34's 50 ms budget), so `enter <TAB>` inside a collection
  nobody has looked at offers the declared geography and no members. A background pre-observation
  of the current neighbourhood would fix it and is exactly the "background discovery" of §34.1.
- Two boxes in `docs/ACCEPTANCE.md` §4.7 — "full-screen map works on supported interactive
  terminals" and "PTY interaction tests pass" — now have all their *unit* proofs green, but both
  name case `099`, which is still `.case.v04`. They are S11's to tick with the rename.
- **`spatial_map_missing::should_only_remove_{edges,nodes}_...` are flaky under a parallel run,
  and were before this increment.** Both compare a map from one `ono` run against a map from a
  second; every sibling test in the binary spawns shells between the two, and a process that
  appeared in between is `recently_changed`, ranks into the second map and is absent from the
  first. They pass with `--test-threads=1` and failed about one gate run in three here; seen
  green in the gate before this increment and green again after it, and nothing in S6 touches
  map ranking. Same family as the two ADR-0165 defers and as the two topology fixtures S4b left.
  The fix is a fixture the two runs are guaranteed to agree on — not a change to either test.
- **The twelve PTY tests are load-sensitive, and the gate runs them under load.** All twelve pass
  in 47 s when `spatial_interactive_missing` runs on its own, and repeatedly; in a
  `cargo test --workspace` on a machine also running two other worktrees (load average 16 on
  8 CPUs) three of them exceeded their own 8 s screen budget waiting for `map` to open at
  COMPUTE, because opening the view costs one full projection — the same providers `map` asks,
  including systemd over D-Bus. Two things are true and both are worth writing down: the view is
  unresponsive while a projection is in flight, which §34.2's view budget will eventually have to
  answer for (S11 owns the budgets); and the picker's own fixture copies `/bin/sleep` and can hit
  `ETXTBSY` when a sibling test forks across the copy, which
  `spatial_contracts_missing::should_refuse_an_ambiguous_selector_in_a_script_rather_than_open_a_picker`
  already documents and avoids by using a symlink instead. Run the file alone before believing a
  failure in it.

---
**S8 — remote systems as space — is complete (2026-08-28, agent `S8`).** ADR-0168 to ADR-0172;
gate green; acceptance case `docker/acceptance/cases/106-spatial-remote.case` added (51
assertions, all proved locally against the real binary).

Delivered:

1. **A host's geography is its own** (ADR-0168). `ono_spatial_core::space` now keeps the
   geographies this process knows: `stand_in` moves into one, `learn` registers one without
   moving, `space_of_id` says which space an id names *and whose*. `SpatialIdentity::space_in`
   adds the host to a canonical space's identity for a remote scope and nothing at all for a
   local one, so every id built before S8 is unchanged and `testbox`'s `COMPUTE` is not this
   machine's. Twenty-odd call sites became host-correct without being edited.
2. **`jump <link>` crosses the boundary, visibly** (§19.2, §53). The destination is the linked
   host's root `SystemPlace`; the crossing is stated in words on stderr, so a colourless terminal
   sees it and a script's object stream stays objects; the trail step carries both ends and the
   `scope_crossing` naming the scope entered; the prompt takes the host in `local`'s place
   (§21.1, §21.3) whether `enter link` or `jump` put the session there.
3. **The session's host follows the place.** `enter`, `follow`, `up`, `back` and `jump` all call
   `SpatialSessionState::arrive_at`, which moves the geography, the provider bridge and — through
   `Session::pipeline_context` — the provider registry to wherever the place actually is (§14.4).
4. **Remote identity does not merge with local** (§43.7, ADR-0169). A remote scope is named by
   the *link*, never by what the far side calls itself, and its boot identity is honestly unknown;
   the provider bridge is per host, so its key memory cannot bridge a link; and §27.1 step 4 is
   now the *current host's* index, so `enter process/1` on `testbox` is not answered with the
   local pid 1 the index still holds.
5. **The link map** (§19.1). `ono.link-place/1` is a new contract, and `ono.place-view/1` gains a
   nullable `links` field, present at the root of a host. A link that is not connected stays in
   the map with the state that says so.
6. **A link that is gone is `stale`, never empty** (§35.2, §43.7, ADR-0171). `detach link` keeps
   its v0.2 meaning and adds one: this session stops *following* the link. Standing on such a
   host, `look` and `near` ask nothing at all — every exit is withheld `stale` with the link
   named, the place's `permission` and `freshness` are `stale`. That is not only about age:
   provider calls fall back to the local registry when no link is reachable, so asking would
   answer a question about `testbox` with this machine's objects.
7. **Provenance and confidence on every far-side relation** (§19.4, §11.4). A relationship edge
   observed across a link carries `Provenance::remote(provider, host, …)`, and so does the
   declared geography of a linked host — a remote observation is never indistinguishable from a
   local one.
8. **The federated map** (§19.3, ADR-0172). `map links` is its own command, `ono.place.map-links`,
   with the target word §19.3 writes; it draws this host's root beside every linked host's root,
   joined by `host.linked_to` edges whose confidence is the evidence's — `exact` for a link this
   session negotiated, `user_declared` for a definition nobody has connected. The default `map`
   mentions no linked host at all, which is §19.3's other half.

Green from `crates/ono-cli/tests/spatial_remote_missing.rs` — **all thirteen**, none left ignored:
`should_list_a_linked_host_among_the_places_when_looking_at_the_local_root`,
`should_give_a_linked_host_a_root_place_distinct_from_the_local_root`,
`should_announce_the_boundary_in_plain_text_when_jumping_to_a_linked_host`,
`should_mark_the_remote_host_in_the_prompt_after_a_jump`,
`should_record_the_host_and_the_scope_crossing_of_every_step_in_the_trail`,
`should_return_home_to_the_local_root_from_a_remote_place`,
`should_keep_a_remote_process_place_distinct_from_the_local_one_with_the_same_pid`,
`should_report_a_place_behind_a_detached_link_as_stale_rather_than_empty`,
`should_keep_a_detached_link_visible_with_its_state_in_the_link_map`,
`should_carry_provenance_and_confidence_on_every_relation_that_comes_from_the_far_side`,
`should_refuse_to_jump_to_a_hostname_that_is_not_a_known_link`,
`should_not_expand_a_remote_graph_into_the_default_root_map`,
`should_show_the_linked_hosts_when_the_federated_map_is_asked_for`.

**Two RED tests of this tranche contradict each other, and S7 owns the other one.** ADR-0170 has
the trace in full. `spatial_remote_missing::should_return_home_to_the_local_root_from_a_remote_place`
is only satisfiable if `home` does not push the place it left onto the stack `back` walks;
`spatial_identity_missing::should_return_the_tombstone_and_keep_the_trail_record_when_back_points_at_a_dead_place`
(still ignored, assigned to S7) is only satisfiable if it does. The two scripts are structurally
identical — `L → P → home(L)` against `L → T → C → home(T)` — so no rule about `home` alone
satisfies both. S8 implemented the S8 reading (`Movement::Home.extends_history() == false`, on the
same argument that already made `back` not a toggle) and recorded the collision. **Whoever
un-ignores the tombstone test reads ADR-0170 first.**

**What S9 and S10 need from this phase** — the three things:

- **A place's host is `SpatialSessionState::current_scope()`, not `scope()`.** The latter is the
  machine the shell runs on and never changes; the former is the host the session is standing on.
  Anything that projects, ranks or signs an observation wants the second.
- **`crate::spatial::links` is the only place that answers "may I cross this link".** Both
  `Session::pipeline_context` (which registry answers) and the spatial views (is this place stale)
  read `links::reachable`. A plugin space or an adapted object that lives on a linked host asks
  the same function.
- **The link name is the scope id.** `remote_host:<link>` is the whole identity of a remote scope
  (ADR-0169), so an adapter or a plugin that wants to place a remote object composes its scope
  from the link name and nothing else.

**Found, not fixed, and deliberately outside this increment:**

- §19.1's link map has no latency and no "last seen": `12ms` and `last seen 3h ago` are in the
  spec's own example, and nothing in this build measures either. `ono.link-place/1` carries no
  field for them rather than a null one nobody fills.
- §19.4's *genuinely* two-sided cross-host correlation — a connection whose remote endpoint maps
  to a linked host (§14.5) — is not built. What is built is the honesty requirement that holds for
  every far-side edge: it says who observed it, from where, and how sure it is. The richer fixture
  §43.3 asks for needs two hosts with a real connection between them, which an unprivileged
  offline container cannot make.
- A neighbour reached by canonical hierarchy rather than by an edge still carries a null
  `confidence` and a null `provider`: there is no relationship to explain, and §2.6 forbids
  inventing one. Every neighbour of a *process* has an edge, which is why the §19.4 test passes;
  a place whose exits are collections would show the nulls.
- `map links` draws one hop. §19.3's picture has `prod/web01 ----- prod/db01`, a link between two
  *remote* hosts, which this session cannot observe: it would have to ask `testbox` for its own
  link table, and nothing in the protocol carries one.
- Two links to the same machine under two names are two scopes and therefore two sets of places.
  That is a false distinction rather than a false merge, and §2.17 prefers it — but a session that
  does it will see the same process twice.

---

**S7 — live topology, tombstones and the change section — is complete (2026-08-28, agent `S7`).**
ADR-0178 to ADR-0181 and ADR-0184; acceptance case `docker/acceptance/cases/108-spatial-live.case`
added (23 assertions, dry-run against the real binary and the real fixtures).

Delivered:

1. `crates/ono-spatial-events` (§45.5) — the change model, §25.3's freshness vocabulary, the
   §25.4 snapshot comparison, the event merge over the v0.2 watch envelope, and §26's landmark
   recalculation trigger. It reaches no provider, no terminal and no clock (ADR-0178).
2. **Tombstones** (ADR-0179). A place becomes one when a provider that was asked about it does not
   answer for it — and only then: `io.not_found` is the object saying it is gone, every other
   error is a reading failure, which §35.2 forbids rendering as absence. The index keeps the entry
   (the identity is what tells a tombstone from a place that never existed), its lifetime closes,
   and the relationships nobody asserts any more are dropped from both ends. `look`/`near`
   describe it, `back` arrives at it, `follow` and `enter` refuse with `spatial.destination_gone`.
   `spatial.tombstone.lifetime` (1m) is what "short-lived" means.
3. **`map --live`** (ADR-0180) through the v0.2 watch runtime rather than a second one
   (`ono_command::watch_events`, §2.16). It waits on events, drains a moment before drawing it,
   re-projects through the still `map`'s own path, and emits only a difference. `live_capable` is
   now answered rather than assumed; every value carries `live`, `freshness`, `change_source` and
   the `ono.spatial-change/1` list §45.5 calls the live map update message.
4. **`look --changes`** (ADR-0181) — the §25.4 comparison against what this session last saw
   around the place, with §24.3's three distinct answers: `unknown` (no baseline), `empty` (a
   baseline and no difference), `available` (the differences). It compares the *complete*
   neighborhood, because comparing the ranked one reports the ranking as change.
5. **`home` extends the navigation history** (ADR-0184), settling the conflict between
   `spatial_identity_missing::should_return_the_tombstone_…` and
   `spatial_remote_missing::should_return_home_to_the_local_root_from_a_remote_place`. §20.1 writes
   a step for `home` and §2.4 makes every movement reversible, so `back` returns through it.
   ADR-0170 is superseded on that point; **the remote test's assertions are unchanged** and only
   the number of `back`s it spends walking its own history moved from two to three.

Green now, all previously `#[ignore]`d — 5 tests:

- `spatial_identity_missing` (4): `should_report_a_tombstone_rather_than_a_live_place_when_the_visited_process_has_exited`,
  `should_refuse_to_traverse_a_relationship_when_the_place_is_a_tombstone`,
  `should_never_resolve_a_tombstoned_place_to_a_live_object`,
  `should_return_the_tombstone_and_keep_the_trail_record_when_back_points_at_a_dead_place`.
- `spatial_relationships_missing` (1): `should_show_the_connection_edge_appear_and_vanish_when_the_connection_opens_and_closes`.
- 20 new crate-level outcome tests: `crates/ono-spatial-events/tests/{snapshot_comparison,event_merge}.rs`
  (15), `crates/ono-spatial-core/tests/trail.rs` (2), `crates/ono-spatial-index/tests/index.rs` (3).

**Left ignored, with the reason on the test:**

- `spatial_identity_missing::should_distinguish_a_tombstone_from_a_place_that_never_existed` —
  §40's two conditions are delivered and distinct, and the `gone` half of the test passes. The
  `never` half asks for two things this increment does not owe, and ADR-0179 §Spec deviation
  carries both: `enter <target> <identity>` keeps v0.2 §14.3's `resolve.target_not_found` for an
  identity nothing answers to (`identity_missing::should_refuse_to_enter_a_user_that_does_not_exist`
  pins `Ono-Sendai-E0102`), and the script's exit status is its last statement's under ADR-0008,
  while the refused `enter` leaves the place where it was, so the following `look` succeeds.
- `spatial_interactive_missing::should_keep_the_shell_alive_when_ctrl_c_ends_the_live_map` — it
  asserts the alternate screen goes on and off around `map --live`, which is **S6's full-screen
  view**. S7 delivers the live *stream* and the change model; the view that renders it and the
  key that leaves it are S6's, and the test is theirs to un-ignore.

**Found, not fixed, and deliberately outside this increment:**

- **A TIME_WAIT socket has inode 0, and `ono.socket/1`'s identity is `[inode]`,** so every
  TIME_WAIT socket on a host reconciles into one place whose label is whichever record was
  absorbed last. Visible in a live map as a connection node that "appears" carrying an unrelated
  peer. It is a v0.2 identity contract question (which fields make a socket that socket), not a
  spatial one — exit test: two TIME_WAIT sockets are two places.
- An unbounded stream must be bounded and serialised to reach stdout (v0.2 §18.3), and `to json`
  collects, so `map --live --json | take N | to json` prints nothing at all if it is cut off
  before the Nth value. A streaming serializer — `to jsonl`, or `to json` forwarding one document
  per value on an unbounded stream — would make a live view scriptable without knowing in advance
  how many changes to wait for. Exit test: `map --live --json | take 100` prints its first value
  before the second arrives.
- `spatial_map_missing::should_only_remove_{edges,nodes}…` fail on a loaded host and pass on a
  quiet one: each compares two `ono` runs over the whole process collection, so a process started
  between them is a node the earlier map does not have. Same family as the cluster-expansion test
  S5 left; they failed identically on this tree before S7 touched it.

**S4d + S4e — the storage remainder and the configuration behaviour — are complete
(2026-08-28, agent `S4de`).** ADR-0185 to ADR-0188; gate green; acceptance case
`docker/acceptance/cases/109-spatial-storage.case` added (25 assertions) and
**`scripts/acceptance.sh` stands at 74 passed, 0 failed**.

Delivered:

1. **§47 the switch.** `spatial.enabled = false` leaves the typed shell and ordinary commands
   working and answers every `ono.place.*` verb with `spatial.unsupported` (Ono-Sendai-E1009) —
   a named refusal a script can branch on, not a command that vanished, which matters because
   `look` shadows util-linux `look` (ADR-0185). One guard at the point the shell binds a native
   stage, foreground and background alike. The setting is read from the *live* session settings,
   not the `spatial.*` snapshot, because it is the one key whose purpose is to be flipped. The
   spatial side effects of ordinary commands stop with the verbs: `enter <target>` still pushes
   its v0.2 context frame and no longer moves the place, `cd` no longer synchronises one, and
   §9.4's completion offers no neighbourhood.
2. **§33.1/§34 the warm view.** The session remembers what each provider *query* answered and
   when; a command inside that target's §33.3 lifetime reads it back instead of asking again
   (ADR-0186). The lifetime is the index's own TTL policy over the kinds of place the target
   produced, shortest first. `look --json`'s `freshness` is now `cached` when every target was
   recalled and nothing was asked — §25.3's own word — and stays `polled` where it did ask.
   Marginal cost of a repeated root `look` in a **debug** build on a loaded machine: ~70 ms →
   ~44 ms, with no provider asked at all in the repeat. S11 owns the number as a release gate.
3. **§15.3 the mount boundary as a place.** `ono.place-view/1` carries a nullable `boundary` of
   the new `ono.mount-boundary/1` — local path, filesystem, source, `remote`, plus `read_only`
   and the mount's `spatial_id` — every field composed from what `get mount` answered (§2.16),
   and `ono-spatial-render` prints the block §15.3 draws. `remote` is decided from the filesystem
   type and the shape of the source, conservatively in both halves (ADR-0187).
4. **§3.2/§2.18 the crossing.** `movement::crossing_between` asks the two places' own scopes
   first — a host or a container must not be understated — and only then whether the two paths
   sit on different mounts, recording a `filesystem` `ScopeBoundary` that does not claim to have
   left the host. `enter`, `jump` and `up` all go through the one function.
5. **§15.1 the path tree keeps its shape.** `parent_rules(Directory)` is now
   `[path.parent, mount.backs_directory]`, with `docs/spec/providers/linux-procfs.yaml` saying
   the same. §15.1 is unconditional, so the parent of `/mnt/backup` is `/mnt`; the mount is
   where the path tree runs out (`/` has no directory above it), which is where §15.2's
   MOUNTS -> DIRECTORY ROOTS meets the Unix tree. Recorded as a **spec deviation** in ADR-0187.
6. **§15.4 the directory place.** Children are hierarchy, not a relation (§3.4):
   `SpatialIndex::path_children` is the reverse of `set_path_parent` and the neighbourhood puts
   them first. The **read is whole and the view is bounded** — a 400-entry directory counts four
   hundred, shows eight and says "392 more not shown" (ADR-0188). `storage::observe_place_at` is
   the one seam every path spelling reaches: the object, the mount table, and the enclosing
   directory — which is what makes **`up` from a file** reach it, the gap S4c left open.

Green now, all previously `#[ignore]`d — 14 tests:

- `spatial_storage_missing` (3, the suite is now fully green):
  `should_show_the_source_device_and_filesystem_when_the_place_is_a_mount_boundary`,
  `should_record_the_boundary_crossing_when_traversing_from_the_root_into_a_mounted_directory`,
  `should_summarize_a_large_directory_instead_of_enumerating_it` — the last of which passed
  before because nothing was ever read, and passes now because a bound was applied.
- `spatial_contracts_missing` (7): `should_keep_the_typed_shell_working_when_the_spatial_layer_is_disabled`,
  `should_answer_repeated_looks_far_inside_the_look_budget`, and **five S4 tests that were
  delivered by S4b/S4c and left `#[ignore]`d by mistake** — `should_refuse_to_go_back_or_up_from_the_root_with_a_named_spatial_error`,
  `should_start_every_session_at_the_local_system_root`,
  `should_keep_a_scripts_navigation_out_of_the_callers_place`,
  `should_keep_the_trail_session_local_while_a_pin_survives_the_session`,
  `should_resolve_repeated_observations_of_one_object_to_the_same_spatial_id`.
- `spatial_navigation_missing` (2, the suite is now fully green):
  `should_stream_neighbors_that_compose_with_the_pipeline_when_near_runs_in_a_script`,
  `should_keep_running_external_commands_when_spatial_navigation_has_happened`.
- `spatial_topology_missing` (1, the suite is now fully green):
  `should_follow_the_parent_relation_from_a_discovered_process_to_its_spawner` — its `#[ignore]`
  said "un-ignored by the increment that delivers `trail`", and S4c delivered it.

Each of the eight late un-ignores was run twice on its own before the ignore was removed.

**Still ignored across the nine spatial suites at this commit — 6 tests, none of them S4's**
(S7's tombstones and S8's remote federation landed in the integration between S4d's work and its
rebase, and un-ignored the rest):

| Suite | Test | Owed by |
|---|---|---|
| contracts | `should_keep_a_package_relation_out_of_the_map_until_its_capability_is_granted` | S9 |
| contracts | `should_carry_the_contributing_package_as_the_origin_of_every_plugin_edge` | S9 |
| contracts | `should_reconcile_an_adapted_object_with_its_native_twin_into_one_place` | S10 |
| contracts | `should_never_let_raw_command_output_become_a_place` | S10 |
| identity | `should_resolve_the_adapter_view_and_the_native_view_of_one_process_to_one_spatial_id` | S10 |
| identity | `should_distinguish_a_tombstone_from_a_place_that_never_existed` | S7 |

`spatial_storage_missing`, `spatial_navigation_missing`, `spatial_topology_missing`,
`spatial_map_missing`, `spatial_relationships_missing`, `spatial_remote_missing` and
`spatial_interactive_missing` carry no `#[ignore]` at all.

Two of the S9/S10 tests pass when run with `--ignored`. They were left alone on purpose: a test
that passes because the condition it describes cannot arise yet is not delivered, and S9/S10
should be the increments that decide it.

**Found, not fixed, and deliberately outside this increment:**

- ~~`… | select <field> | to text` refuses with `Ono-Sendai-E0201`~~ — **fixed by S11a**
  (`fix(data)`): a record `select` has narrowed to one field is that field's line, and
  `get mount | select target | to text` prints one path per line. `--field` still projects a
  dotted path or one field out of a full record, and a record of several fields is refused
  exactly as before.
- `spatial_map_missing::should_only_remove_{edges,nodes}_…` failed about one gate run in three
  here, as S6 already recorded; they are green with `--test-threads=1`. **A gate run on this
  machine now needs `RUST_TEST_THREADS=1` to be reliable**, and that is a fixture problem in
  those two tests, not a harness one.
- **`ono-sendai:acceptance` is one image tag shared by every worktree.** A concurrent
  `scripts/acceptance.sh` in another worktree overwrites it, and a later `--no-build` run then
  tests the *other* agent's binary — which cost an hour here before it was spotted. Set
  `ONO_ACCEPTANCE_IMAGE=ono-sendai:acceptance-<agent>` while several agents share a machine.
- `options_and_selectors_missing::should_trace_nothing_else_when_no_connection_has_the_requested_remote`
  fails whenever *something else on the machine* holds a socket to 192.0.2.1 — a sibling
  worktree running `test port 192.0.2.1 443` does exactly that, and the connection stays
  `syn-sent` for two minutes. The test's premise ("this machine holds no connection to it") is
  the thing that broke, not the shell. It is green on an idle machine.
- §15.4's other optional neighbours are not delivered and say so rather than showing zero:
  `open-by processes` needs an `lsof`-shaped provider, `owned-by users` is an expensive relation
  nobody has asked to load, `changed recently` is a snapshot difference (§25.4).
- §8.2 clustering of directory entries — grouping them by kind or by name instead of counting
  them — is the next increment on top of ADR-0188; the field it would fill already exists on
  `ono.map-cluster/1`.
- An object place (a process, a socket, a directory) still expands its relationship providers on
  every `look` and honestly says `polled`. Caching relationship edges is a later increment with
  its own test; §34.1's background discovery needs the update channel S7 builds.

**S11 — release hardening: the ten §44 acceptance scenarios — is complete (2026-08-28, agent
`S11a`).** The ten `docker/acceptance/cases/09x-spatial-*.case.v04` files are renamed to `.case`
and the referee runs all 87 cases green, twice in a row. Ten commits, gate green:

| Commit | What it fixes | Proof |
|---|---|---|
| `fix(data)` | `… \| select f \| to text` refused a record `select` had narrowed to one field | `to_text` renders a one-field record; exit test `get mount \| select target \| to text` |
| `fix(spatial)` collections | a collection said `unsupported` while the index held its members (ADR-0197) | `spatial_contracts_missing::should_show_a_place_only_an_adapter_observed…` |
| `fix(spatial)` permission | a denied path was reported as missing, and an unreadable directory became the cwd (ADR-0198) | `spatial_identity_missing::should_refuse_a_path_this_user_may_not_read…` +2 |
| `fix(spatial)` paths | `enter /srv/app/..` made a cycle in the path tree and the next `look` overflowed the stack (ADR-0199) | `spatial_storage_missing::should_stand_in_the_directory_a_path_names…`, `ono-spatial-query::resolution::should_answer_a_place_path_rather_than_looping…` |
| `fix(spatial)` evidence | an edge said who observed it and never what they saw (ADR-0200) | `spatial_relationships_missing::should_carry_the_raw_evidence_of_an_edge…` |
| `fix(spatial)` find | `find place --where` read the providers and not the index (ADR-0201) | `spatial_contracts_missing::should_find_a_place_by_its_properties…` |
| `fix(spatial)` find record | a search result left `state` and the §24.1 summary null where `look` filled them | `::should_describe_a_search_result_and_a_place_view_with_the_same_record` |
| `fix(spatial)` relations | a relation §32.1 declined for cost was reported as one nobody serves | `spatial_relationships_missing::should_say_a_costly_relation_is_unknown…` |
| `fix(shell)` cwd | `cd` did not move the process, so `find file .` walked the launch directory | `builtins::should_change_the_directory_a_native_command_sees_when_cd_has_run` |
| `fix(spatial)` denial | a map of a denied place called itself `complete`; `find --near <path>` never reached the filesystem | `spatial_identity_missing::should_not_call_a_map_complete…`, `::should_refuse_a_search_anchored_on_a_path…` |
| `feat(spatial)` listeners | §13's `listeners` group was missing from a service place | `spatial_relationships_missing::should_offer_the_listeners_of_a_service…` |

ADRs: 0197 (a collection shows the places it holds), 0198 (denied is not missing), 0199 (one
directory however the path spells it), 0200 (an edge carries what the provider saw), 0201 (`find`
searches the index too).

**Found by S11a, not fixed, and recorded rather than faked:**

- **A tombstone's `replacement:` candidate (§10.3's example, §40's "actionable next steps") is
  never computed and answers `null`.** The field is on `ono.spatial-place/1`'s `tombstone` record
  and `Tombstone::replaced_by` exists; nothing calls it. It cannot be answered at the moment the
  old place ends: the source of the relation that reached it — the unit that controlled the
  process — has not been observed again, so the index holds no candidate to name. Two honest
  routes: re-observe that one source when a tombstone is rendered (a targeted query, not an
  enumeration), or fill the tombstone lazily when a later observation records an edge from the
  same source by the same relation to a live object of the same kind. Offer a candidate only when
  that source reaches **exactly one** such object — a choice among several is a guess, not a
  candidate (§2.17, §53). Exit test: after the §44.7 restart, `look --json` at the tombstone
  carries `tombstone.replacement` equal to the new process's `spatial_id` and
  `replacement_via` naming `service.controls_process`; `docker/acceptance/cases/096-…` `44.7e`
  then asserts it instead of what it asserts now.
- **`enter process <pid>` answers `spatial.not_found` for a process started with `setsid`.**
  **Corrected by S11b: this does not reproduce — the report's `$!` is `setsid`'s own pid, and
  `setsid` exits at once. See the S11b section below.** As reported:
  Reproducible: `setsid sleep 60 & ono -c "enter process $!"`. The same pid entered without
  `setsid` resolves. A session leader in its own session is an ordinary process and §12 makes it
  a place; the selector or the provider query is filtering on something it should not. Exit test:
  a `setsid`-started process is enterable by pid.
- **Two gate runs in a row went red on tests whose premise the machine broke, and green on the
  next.** `spatial_topology_missing::should_complete_the_relations_available_from_the_current_place_when_tab_follows_follow`
  waits 8 s for the completion after a walk it recognises by its own *echo*, so a busy host makes
  it wait for a walk that has not run yet — S6's note about it is exact and still true.
  `::should_show_the_mounts_the_mount_provider_answers_for_when_entering_storage_mounts` compares
  the mount table two `ono` processes saw, and the acceptance containers running beside it were
  mounting and unmounting overlayfs between the two. Both are green on an idle machine and both
  are premises about the host rather than claims about the shell. Neither is worth weakening; the
  first would be sound if it waited for the *place* rather than for the echo.
- **`ono.socket/1` gives a listener and its accepted connection the same `follow socket` word.**
  §12's "`follow socket :443` MUST traverse to the matching socket" is served, and bare
  `follow socket` on a process holding both is `spatial.ambiguous_selector` — correct, and worth
  knowing before writing a case that assumed one socket.

**S11b — the rest of v0.4 §52: budgets, evidence, the security review, dogfooding and the
checklist — is complete (2026-08-28, agent `S11b`).** Eight commits, gate green on each; the
container ran the new case on image `ono-sendai:acceptance-s11b`.

| Commit | What it delivers |
|---|---|
| `test(spatial)` | the racy half of `should_complete_the_relations_available…` — it read until `parent` was on screen and then asserted `user` in the same breath, which is why the board carried it as "fails under parallel load" |
| `fix(spatial)` | a map filter narrows the bounded map instead of re-selecting it (ADR-0202) |
| `test(spatial)` | the §43.5 renderer snapshots at 40/80/120/200 columns, and §34's 16 ms frame budget at a real PTY |
| `docs(decisions)` | ADR-0203, the spatial enumeration review: ADR-0015's table extended with seven rows, each naming a passing test |
| `test(xtask)` | `xtask/tests/spatial_evidence.rs`, the guard that keeps §4.7 from rotting |
| `test(acceptance)` | `docker/acceptance/cases/100-spatial-performance-budgets.case`, the §34 budgets at their real figures |
| `feat(help)` | `help spatial` (§38.1, a MUST that was missing), found by dogfooding |
| `docs` | `docs/dogfood/v0.4-2026-08-28.md`, and §4.7 ticked from the evidence |

**The §34 budgets, measured in the container on the §43.3 fixtures.** None is violated, so no ADR
documents a violation:

| Budget (§34) | Measured |
|---|---|
| interactive startup to usable prompt < 150 ms | 0 ms over `bash` under the same `script(1)` harness (272 ms both) |
| basic `look` local cached < 50 ms | 178 µs per repetition |
| `near` cached < 50 ms | 343 µs |
| map L0/L1 cached < 100 ms | 334 µs |
| map L2 ordinary host < 250 ms | 472 µs |
| search common indexed objects < 100 ms | 1 803 µs |
| focus/navigation in a rendered map < 16 ms/frame | 88 µs median at a real PTY (slowest 386 µs) |
| §34.1 discovery does not block the prompt | unchanged at 0 ms with 200 extra processes and a 20 000-entry directory |

The startup figure is measured **against a baseline of the same harness running `bash`**, because
`script(1)` costs about 270 ms of its own in that image — `bash` under it takes as long as `ono`
does, to the millisecond — so an absolute figure would be a measurement of the harness. A whole
non-interactive `ono -c true` run takes 18.5 ms there.

**Found by dogfooding (`docs/dogfood/v0.4-2026-08-28.md`), one fixed, the rest filed.**
The honest verdict on §52.3's statement is in that file: it holds for orientation and hierarchy
and breaks at the first permission boundary, because a group the provider answered `null` for is
rendered as `0` rather than as unknown.

**Two entries on this board are closed by S11b's own evidence:**

- **The bounded/filtered map defect is fixed** (ADR-0202). It now has a deterministic reproducer
  rather than a host-dependent one: `ono-spatial-query::properties::should_keep_every_node_and_edge_a_filter_left_alone_and_invent_none`
  is red at seed 1 on the old projection.
- **"`enter process <pid>` cannot reach a process started with `setsid`" does not reproduce.** Its
  reproducer — `setsid sleep 60 & ono -c "enter process $!"` — records `setsid`'s own pid, and
  `setsid` forks and exits immediately, so the pid looked for belongs to a process that is gone.
  Started properly (`setsid tail -f /dev/null &`, then the child's real pid) `find place --type
  process --where pid == <pid>` finds it and `enter <pid>` enters it. No defect; the entry is
  removed.

**One thing S11b made slightly worse and did not hide.** The interactive suite gained a
thirteenth PTY test (the frame budget), and one full-workspace gate run then failed
`should_preserve_the_current_place_when_the_terminal_is_resized_with_a_place_open`, whose 8 s
budget for closing a full-screen map of COMPUTE (500 processes) is tight when several PTY
sessions run beside it. The file is green four runs in a row on its own and green on the
following full gate. It belongs to the same family as the two host-premise flakes S11a recorded,
and the fix is theirs: wait for the *place*, not for a byte count.

**S11c measured that family and closed it.** The picker test joined it:
`spatial_interactive_missing.rs::should_open_a_picker_and_make_the_choice_current_when_a_selector_is_ambiguous`
failed roughly one run in four **with and without** that session's changes — four runs at
`079aa98` gave one failure, three runs with the working tree on top gave one — so it was a
premise about the host, not a claim about the shell, and two full gate runs in a row died on it
and on the resize test. A referee that fails one run in two is not a referee (AGENTS.md §14), so
the premise was fixed rather than the flake tolerated: `BUDGET` and `STARTUP` in that file are
**liveness bounds, not performance assertions** — they exist so a screen change that never comes
fails instead of hanging — and they are now 45 s and 60 s. No assertion changed, and the file
still finishes in 14.7 s, because a bound that is never reached costs nothing. The §34 figures
are asserted where they belong and are untouched:
`::should_repaint_a_focus_move_far_inside_the_frame_budget_when_the_map_is_open` (16 ms per
repaint) and `docker/acceptance/cases/100-spatial-performance-budgets.case`.

What that leaves standing is the observation underneath, which is about the shell and stays on
this board: **opening a full-screen map of COMPUTE on a 500-process host is unresponsive while
one whole projection is in flight**, which §34.2's view budget will eventually have to answer for.

A fourth member of the family, same treatment:
`spatial_remote_missing.rs::should_refuse_to_jump_to_a_hostname_that_is_not_a_known_link` gave the
run ten seconds to refuse `jump prod/web01.invalid`, and the refusal costs eight of CPU in a
debug build on this host — measured at 10.02 s on `76adb95` and 8.6 s with S11c's changes, so it
was the machine it raced, not a resolver. What proves nothing was dialled is the error name
(`spatial.not_found`, never a resolve or connect failure); the budget is only the hang guard, and
it is 60 s now.

A fifth member, found by CI on 2026-08-30 and fixed the same day (ADR-0417):
`spatial_navigation_missing.rs::should_stream_places_with_scope_and_provenance_when_find_searches_with_a_predicate`
spawned a `SleepChild` and then asserted that `find place --where state == "running"` streams at
least one place. The child it spawned sits in state S, and on an otherwise idle runner the
sampling instant found zero processes in state R: run 33318207211 (attempt 1, commit `1cee6cb`,
a README-only change) answered `[]`, attempt 2 of the same commit was green, and three local
runs in a row were green too. The shell answered correctly both times; the test claimed a host
premise it had never arranged. The premise is now established by the test itself — a
`BusyChild` burns CPU for the seconds the test holds it, so a runnable process exists whenever
the provider samples — and no assertion changed.


## Found, not yet filed

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
- **`docs/spec/schemas/limit.v1.yaml` is not embedded.** `ono_value::builtin_schemas()` lists
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

- **The persisted audit trail keeps one session's events and drops the next session's.** Three
  `ono -c 'load plugin dev.example.echo; echo:clock | count'` runs under one `XDG_STATE_HOME`,
  with `clock.read` granted `--duration always`, leave `<state>/ono/kuang/audit.jsonl` with one
  `clock.now` line, and `get audit --plugin dev.example.echo | where action == "clock.now" |
  count` answers `[1]`. The event ids are deterministic per process —
  `3a2ecab3-0000-4000-8000-000000000001` for the first event of a source in every session — and
  the flush appends only ids not yet on disk (`written_audit`), so the second session's first
  event is taken for the first session's and never written. Found 2026-09-03 while proving #5:
  acceptance case 211 had to read the trail in the session that wrote it. What closes it: an
  event id that is unique across sessions (a session nonce or the timestamp in the id), and a
  test that runs two sessions and counts two `clock.now` lines on disk.
- **A plugin command that fails after it started is an empty stream with exit status 0.** With
  the example package loaded and `filesystem.read` granted for `<dir>/allowed/**`, `echo:read-file
  --path <dir>/secret.txt | to json` prints `[]` and exits 0, while the supervisor's trail
  records the `capability.scope_violation` and the plugin's invocation ended `Failed`. A command
  refused *before* it starts — `echo:clock` with no `clock.read` grant — prints
  `Ono-Sendai-K11301` and exits 1, so the difference is where the failure happens: a
  `Outcome::Failed` the plugin returns mid-invocation reaches the shell as an end of stream, not
  as the pipeline's failure. Found 2026-09-03 while proving #5 (`echo:infer --provider
  <outside-scope>` behaves the same). What closes it: the invocation's terminal error becomes the
  pipeline's per-item failure (spec §16.5) and carries the exit status; a test through the binary
  asserting `K11304` on stderr and status 1 for the read-file case above.

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
one-element list where `docs/spec/commands/meta.yaml` declares `string`; `as_str()` fails, the
filter is skipped, and the reader who asked a narrower question receives the whole registry. §2.6
again — a filter that silently did not apply is worse than a refusal. The check belongs in the
binding layer beside the declared type rather than in each command. **Exit test:** a wrongly typed
option is refused by name.

**`enter process/1` was ambiguous on any host with an ssh login — fixed 2026-09-03 (`3278d58`).**
`argv[0]` is memory a process owns, and `sshd-session: william@pts/1` is a status line rather than
a path. Taking its last slash-separated segment as a program name gave that session the **exact**
alias `1`, beside pid 1. The gate went red between two runs an hour apart on an unchanged tree, the
moment a user logged in at 11:57, taking
`spatial_contracts::should_report_denied_information_as_denied_rather_than_as_an_empty_collection`
and `::should_serve_every_relation_it_declares_and_declare_every_relation_it_serves` with it. No
name is derived from a `command[0]` containing whitespace, and the test carries a
`CommandExt::arg0` decoy.

*A correction to this board.* An earlier version of this entry — written by the orchestrator on
the class-b report — read it as §27.1's **fuzzy** step matching `1293543` by substring, and
proposed rewriting the two tests to `find place … | take 1 | enter`. That was reasoning from the
shape of the escalation rather than from evidence, and it was wrong: `Resolution::Ambiguous` is by
construction the set of **exact** matches, the refusal named exactly two candidates where the fuzzy
step names about a hundred (`find place process/1` does), and removing the bogus alias makes
`enter process/1` resolve to pid 1 with a decoy present. It was a product defect, and changing the
tests would have hidden it.

**`cargo xtask perf` cannot adjudicate on a shared machine, and says nothing about that
(2026-09-03).** All eight Profile S benchmarks read three to five times their checked-in baseline —
`shell.cold_start` at 132 ms against 26 ms — while a second build tree held the load. Absolute
tolerance is right for release qualification (§32.4); what is missing is that the comparison
**reports a regression** where it should report that the environment was not the reference one.
`Comparison` already answers `ForeignEnvironment` for the wrong machine (ADR-0489); it needs the
same honesty for the right machine under the wrong conditions. **Exit test:** a benchmark run under
load reports that rather than a regression.

**`SECURITY.md`'s boundary table is a hand transcription and nothing compares it to the inventory
(2026-09-03).** `docs/spec/hardening/security_boundaries.yaml` exists and
`docs/reference/security-boundaries.md` is generated from it; `SECURITY.md`'s copy is still typed,
so a renamed boundary leaves it silently wrong. §4.8.12's box for #114 says so. **Exit test:** a
boundary renamed in the inventory turns the gate red where `SECURITY.md` disagrees.

**One machine-readable contract lives outside the indexed directory (2026-09-03).**
`docs/baselines/v0.4.1.json` is validated by `xtask::baseline::check` in `spec-check`, but
`registries.yaml` indexes `docs/spec/hardening/` only, so §52.3's "every contract is indexed"
property has a deliberate exception recorded in ADR-0548's *Consequences*. **Exit test:** either
the index reaches contracts outside that directory, or the snapshot moves into it.

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

**`ono-remote` declares its four ceilings as constants instead of reading the shared catalogue
(2026-09-02).** `limits.remote_*` exist with `enforced_by: pending`, and §52.2 wants one source.
This is **work for phase H3** (#54, one central `Limits` contract): if
`docs/spec/hardening/remote_limits.yaml` is still wanted, it should reference these keys rather
than restate the numbers. **Exit test:** changing a remote ceiling in the catalogue changes what
the listener enforces.

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
`raised: false` in `docs/spec/hardening/refusals.yaml` rather than quietly deleted. **Exit test:**
either a path raises it, or it leaves the taxonomy.

**`docs/spec/kuang/errors.v1.yaml:20` says "Nothing here is implemented" (2026-09-03).** Stale for
the K118xx block, which H4 delivered. One line. **Exit test:** the file describes what is there.

**Two worktrees share one acceptance image tag (2026-09-03).** `scripts/acceptance.sh` defaults
`IMAGE` to `ono-sendai:acceptance`, and a run finishing without `--keep-image` **deletes it out from
under a concurrent run in another worktree**, which then reports every remaining case as exit 125
`Unable to find image`. Observed: 16 spurious failures in a full run during the H11/H12 pair. The
run that produced the green result used `ONO_ACCEPTANCE_IMAGE=ono-sendai:acceptance-h11`. This is a
direct cost of running phases in parallel worktrees, and it fabricates failures rather than hiding
them. **Exit test:** two concurrent `scripts/acceptance.sh` runs in two worktrees both report their
own results — by deriving the default tag from the worktree, or by refusing to remove an image
another run is using.

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

**Acceptance case `181` (§59.6, private-key permissions) does not exist (2026-09-02).**
`docs/ACCEPTANCE.md` §4.8.2 names it in three boxes for #34 and #37, and §4.8.13's prose counts it
among the seven the tranche adds. Both H1 issues are closed, so the case was promised and not
written. **Exit test:** `181` exists and runs.

**Case 152's §34 budget sits one millisecond from its limit (2026-09-02).**
`docker/acceptance/cases/152-pathological-sockets.case` measures `get socket | take 1` against a
50 ms budget as the median of 20 runs. In a full 125-case run on a machine at load 6.8 it read
**51 ms** and `get connection | take 1` read **56 ms**, and both were reported OVER BUDGET. Run
alone at load 6.6 it passes; run beside case 151, which forks ten thousand processes, it passes;
and it passed on the GitHub runner. The baseline row on the ordinary host in the same failing run
was **48 ms** — two milliseconds of headroom on a figure the median is supposed to protect.

So this is not a regression and it is not the leak that was first suspected: case 151's children
block on a pipe and exit with their parent, and 151 followed by 152 is green. It is a budget with
no margin, measured on a machine the case does not own. It belongs beside the `ono_testkit` entry
above, and to **H8**. **Exit test:** the case states what it does when the machine cannot meet the
budget, rather than reading a number two milliseconds from the edge and calling it a defect.

**An acceptance case must build the population it needs, and two did not (2026-09-02).** The
GitHub pipeline was red for six pushes before anyone looked, and two of the four failing cases had
the same defect: `docker/acceptance/cases/190` set `limits.materialize_items = 2` and
`191` ran `get process | take 20`, both against the host's process table. The container is an
unprivileged user with **two or three** processes, so `count` answered `[2]` where `[20]` was
asserted, and `"consumed":3` was a coincidence rather than a fact. Both now create their own files
and read them with `get file <dir>/*`, which is the same number on every host. AGENTS.md §11
already says a test may not rely on the developer machine's processes unless the fixture creates
them; these two were written against a host with 900.

The orchestration lesson is separate and is mine: four agents in a row reported *"`scripts/acceptance.sh`
is owed"* and I integrated each of them on a green local gate. **`scripts/gate.sh` does not run the
container**, and AGENTS.md §10 says a capability without a passing acceptance case is not
delivered. Every phase from here runs the containerised suite before its commit is pushed.

**`ono_testkit`'s fixed wall-clock budgets fail under parallel load, and the failure reads like a
product defect (2026-09-02).** Two instances on one day, and both were first reported as hangs:

- `crates/ono-cli/tests/storage.rs::should_trace_a_mount_to_what_it_sits_on_and_who_uses_it`
  overran its 30 s budget while three cargo builds held the load average at 19–24. The command it
  runs, `trace mount / | to json`, answers in **2.79 s and 4.34 s** at load 1.97 — exit 0, 461 KB
  of JSON, 54 mounts and 342 processes.
- `crates/ono-cli/tests/processes.rs::should_trace_the_entered_process_without_a_selector` overran
  `ono_testkit`'s 20 s budget during a gate run that shared the machine with a worktree build. The
  whole suite passes in **1.70 s** at load 3.27, and the command answers in **1 s** in both a debug
  and a release build.

Neither command hangs. What fails is a **20 s or 30 s constant measured against wall clock on a
machine whose load the test does not control**, which is the trap ADR-0252 and issue #21 already
recorded for the completion budget — and which §38 calls a test that does not report execution
truth: a red result here means "the machine was busy", and nothing else.

This belongs to **H8** (#88, #89: three visible outcomes, and an unexpected skip fails the gate).
The two candidate answers are a budget that scales with observed load, or a `SKIP(reason)` when the
machine cannot meet it — §38.4's skip taxonomy exists for exactly this. **Exit test:** a full
`cargo test --workspace` under a load average above 15 produces no failure attributable to the
budget alone.

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

**`run_native_segment` now assembles, drives and drains, and duplicates its binding and assembly
with `stream_segment` (2026-09-02).** The seam is obvious — bind / assemble / drive / write-result
— and §65.12 forbids cutting it in the same work package as the semantic change that created it.
**This is work for H9 (#96)**, not a loose end. ADR-0480, ADR-0481.

**`set client-key --allow` takes a comma-separated word rather than a repeated option
(2026-09-02).** §9.7 writes `--allow <capability>...`, and `ono_command`'s binder keeps one value
per option while a bare comma ends a word, so the spelling that works is
`--allow "process.signal,service.manage"`. Closing it means a repeated-option form in
`crates/ono-command/src/contract.rs` (ADR-0468 §Alternatives). **Exit test:** `--allow a --allow b`
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

**The systemd test fixture leaks a follower process per run, and they accumulate for days
(2026-09-02).** 331 `/bin/sh /tmp/ono-test-<pid>-4/journalctl --output=json --no-pager --follow`
processes were alive on the development machine, the oldest **five days and two hours** old. Every
one of their `/tmp/ono-test-*` directories had already been removed, so each was a follower
pointing at a script that no longer exists, parked in `do_wait` with no child. They were killed by
hand on 2026-09-02; nothing in the repository would have done it.

This is the same defect class the spatial proof found while writing issue #22's fixture — two
`ono` shells holding a pipeline open for seven hours — and the answer there was
`support::run_bounded`, which kills *and reaps* at the deadline. The systemd fixture needs the
same: whatever spawns the `--follow` stub owns its lifetime. Look in `crates/ono-testkit` and
`crates/ono-provider-systemd/tests/` for the spawn site.

It belongs to **H8** (test truthfulness, #88–#94): a suite that leaves 331 processes behind over
five days is not reporting its own execution truthfully, whatever its exit code says. **Exit
test:** a full `cargo test --workspace` run leaves no `ono-test-*` process behind, asserted rather
than observed by hand.

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

---

*Empty when the tranche started.* The twenty-seven entries that stood here on 2026-08-31 were filed as issues **#1–#27** —
five class C, twenty-two class B — and what follows is the record of the triage that produced
them.

**Six of the twenty-seven were closed the same day as already done**, and the reason is the
argument for this whole arrangement: the entries described the tree at `b904327`, eighty commits
back, and said so in as many words while nobody re-read them. #1 (fuzz targets, `669c172`), #2
(agentless, `bbb29e4`), #4 (signature verification), #19 (`Origin::plugin`), #13 and #14 (the §34
cases had run at `41e9688`). #3 lost its isolation half to `1e76009` the same way. Every claim in
the remaining twenty was checked against HEAD — statically, or by running
`target/release/ono` — before it was left open.

---

## The 2026-08-29 triage, and what it closed

**Rebuilt from evidence on 2026-08-29 (agent `triage`), and reconciled with the tree at
`b904327` on 2026-08-29 (agent `STATE-recon`).** The triage pass was written against `ed923ee`.
The fifty-nine commits between `ed923ee` and `b904327` closed **fifty-five** of its boxes and were
never written back, because the agents that produced them were deliberately kept out of this file
so they would not collide in it. Everything they closed stands under *Done, reconciled* below
with the commit and the proof; everything a reproduction against the built binary still found
open at `b904327` was filed as an issue on 2026-08-31 and is no longer repeated here.

Method of the reconciliation, per box: find the evidence the entry names, check it exists at HEAD
and says what the entry claims, and — wherever the behaviour is observable from the command line —
run it against `target/debug/ono` and read the real answer. Nothing below was decided by reading
source alone. Two by-products of the sweep, both reported rather than fixed here:

- **Every test function named anywhere in this file exists in the tree** — 273 distinct names,
  cross-checked against the 2 607 `fn should_*` in `crates/` and `xtask/`. So does every test and
  every acceptance case named in `docs/ACCEPTANCE.md` (168 names, 143 ticked boxes, no dangling
  reference).
- **Eight acceptance cases named by ticked boxes in the §37 phase lists did not exist**, and never
  did: they are the pre-implementation planning names. `028-prompt-shows-context` is really `029`,
  and C2–C8's seven `0NN-<target>-provider` cases are really `037`–`046` under other names. The
  capabilities are all covered; the boxes now name the case that covers them.

Baseline: workspace suite at `ed923ee` — 2 572 passed, 1 failed, 0 ignored (that one failure is
B-prov-1, closed since by `4e53ee4`); 96 containerised cases in `docker/acceptance/cases/`;
`release-check: the shell is release-ready` last printed at `21b37d9`.

| Class | Meaning | Boxes | Where they went |
|---|---|---:|---|
| **C — large** | a spec requirement that is its own tranche | **7** | 2 closed below, 5 filed as issues |
| **B — small** | a concrete defect with a reproduction and an exit test | **27** | 5 closed below, 22 filed as issues |
| **closed since `ed923ee`** | listed under *Done, reconciled* with the commit that closed it | **55** | — |

The class B count grew from the 5 the triage found to 27 as later work turned up more: five while
recording the README figures on 2026-08-30, five more while fixing four of those on 2026-08-31,
and the rest from the class-C tranches. All of it is in the tracker now.

---

### Class C — the large tranches, and the two that closed

What remains here is the two entries the triage opened and later sessions closed. The five that
were still open on 2026-08-31 are issues #1–#5.

- [x] **C-1 — the generated provider conformance suite (spec §35.3, phase C9)** — done
  2026-08-29, `33b6e10` + `a595c4f` + `81edc7c`, ADR-0331 (supersedes ADR-0248's decision *not* to
  build the generator; its "a box claiming a generation must name a generated path" rule stays and
  now accepts this output). `cargo xtask conformance` generates
  `crates/ono-cli/tests/provider_conformance.rs` from `docs/spec/providers/*.yaml`,
  `schemas/*.v1.yaml`, `capabilities.yaml` and `commands/*.yaml`; `spec-check` regenerates and
  fails on drift. **87 generated tests over 18 provider entries, 30 schemas and 35 targets**,
  where 4 providers and 2 schemas were covered before. The assertions live beside the generated
  file in `tests/conformance_harness/mod.rs` — a 1600-line generated file with assertions inside
  is a file nobody reads. Generation **refuses rather than emitting a hole**: an unexercised
  target, an unknown exercise word, or a capability reaching neither a snapshot nor a command each
  stop it. The declarations gained a `conformance:` block, because how a bare snapshot behaves is
  not derivable from anything else. `docs/ACCEPTANCE.md` §4.1 C and D say "generated from" again,
  and it is true now. Four contract violations found by the new suite and fixed in `81edc7c`.

- [x] **C-7 — the §34 pathological fixtures, and the theme system of §44** — done 2026-08-29.
  **Themes** (`1c4866b`, ADR-0332): `theme.name` joins the ADR-0094 catalogue; resolution is
  built-in → `/etc/ono/themes/<name>.toml` → `<config dir>/themes/<name>.toml`; two themes ship
  (`ono`, `neon`); a theme file is TOML (`extends` + `[tokens]`) and is refused rather than
  half-applied on any unknown token, key or value. The session owns the theme and the sinks,
  reporter, REPL, live view and job output take it from there — which is what gives the setting an
  effect. §44's closing rule is mechanical in three parts: a theme is consulted only where there
  is colour, so every theme prints identical bytes under `NO_COLOR`, a pipe or a dumb terminal;
  markers carry no control character and at most 4 chars; `ui.danger`/`ui.warning`/`ui.success`
  keep distinct markers. 19 tests, acceptance case `150`.
  **§34's environments** (`f349971`, `43f35e8`, ADR-0333): `docker/acceptance/fixtures/perf/` and
  cases `151`–`154` — 10 000 forked processes; 5 000 listening unix sockets; 50 000 entries in one
  directory plus 200 levels plus 100 000 files over 1 000 directories; a tool on `PATH` that never
  answers, 100 MB of stdout, and `watch process`. Each measures §34's own figures, prints them
  pass or fail, reports the size it actually reached, and **fails if the environment is not
  pathological**. Two deviations under a `Spec deviation` heading: slow NSS and high-latency links
  are one environment (the container runs `--network=none`, and never-answering is stricter than
  slow); sockets reach thousands, not tens of thousands.


---

### Class B — the small defects, and the five that closed

Each line said what was wrong, how to reproduce it, and what closed it. Five stood open after the
2026-08-29 reconciliation and thirty-seven were already under *Done, reconciled*; later sessions
added more, and by 2026-08-31 twenty-two were open and became issues #6–#27. The five kept here
are the ones a session closed on its way past, and they are kept for the diagnosis each carries.

#### Interactive

- [x] **B-tui-1 — the map answered no key while it re-observed** — done 2026-08-31, ADR-0424.
  Not a flaky test: the hung `ono` was caught twice in `/proc` with its main thread in `ep_poll`
  and its CPU time unchanged across 24 seconds, so it was parked in an `.await` and reading the
  terminal not at all. A `Resize` re-observes, a re-observation asks every provider, and
  `ono-provider-systemd` alone allows ten seconds per bus call — under a full `-p ono-cli` run a
  dozen `ono` processes hit the same buses at once. `Esc` could not close the view, which v0.4
  §34 forbids ("MUST remain interactive ... rather than block unnecessarily"). Every await in the
  map loop now runs through `while_answering`, polling the terminal every 16 ms; keys typed
  during an observation queue and are answered in order, and leaving does not overtake a key
  pressed before it. Proven by `crates/ono-cli/src/spatial/interactive.rs`'s four unit tests —
  `::should_leave_the_view_when_the_closing_key_arrives_while_work_is_still_running` returns
  `Err(Elapsed)` against the old behaviour — and by four consecutive green
  `cargo test -p ono-cli --all-features --no-fail-fast` runs.

  Ruled out along the way, each by experiment: CPU load (passes under sixteen busy loops), the
  number of processes drawn (passes with 1 686 on the host), and crossterm's lone-`Esc`
  ambiguity (`input_available` is false for a one-byte read, so `Esc` is delivered at once).

#### Data and pipeline

- [x] **B-data-9 — bytes that cannot become objects are refused before the program runs** — done
  2026-08-29, `37ce5c3` on `close-last`, ADR-0376. It was a defect, and ADR-0028's reasoning held
  only in part. Three symptoms, one cause: `ls /etc | count` answered **`1`** (wrong, and
  plausible-looking), `seq 3 | take 1` answered one `VALUE` holding the whole listing as bytes,
  and `yes | take 1` never returned. The byte carry was wrapped into a single `Value::Bytes` and
  handed to stages declared over `stream<any>`, which counted the one value they were given; the
  hang is the same bug seen through an endless producer, because the wrap needs EOF.
  **Kept (ADR-0028 point 2):** the carry is one *document*, and there is no honest way to cut
  arbitrary bytes into values — newline-cutting is the implicit text parsing §50 forbids, and
  read-buffer-cutting makes a value whose content depends on scheduling. So `yes | from json`
  still runs to EOF, correctly, for the reason `jq` does. Streaming it would have returned a
  64 KiB slab of `y\n` and called it a stream.
  **Corrected:** ADR-0028 point 1's own rule ("the transform binds anyway and reports the type
  error when it runs") — the binding happened, the type error never did. The contract question is
  now asked at plan time: a consumer declared over objects, fed by an invocation no adapter
  decodes, is refused with `Ono-Sendai-E0911` **before the program is spawned**, so `yes` is never
  started and there is nothing to end. Tests in `crates/ono-cli/tests/native.rs`, incl. the exit
  test `should_answer_at_once_when_an_endless_program_feeds_a_stage_defined_over_objects` with a
  10 s liveness bound, and `should_still_carry_a_whole_document_across_the_boundary_into_a_parser`
  holding the buffering decision. **Not yet run in the container** — case `084-adapters-remote`
  is the one case of the affected shape and its `grep -c 'Ono-Sendai-E0'` still matches, but that
  is reasoning, not a passing case.

#### Found by the class-C tranches (2026-08-29), each its own increment

- [x] **A failed `enter` cost 23.9 s** — fixed 2026-08-29, `c46d524`, ADR-0416. The cause was
  `SpatialIndex::record_edge` recognising an edge by recomputing `edge_id()` — a SHA-256 — on both
  sides of a linear scan, so recording one edge cost two hashes per neighbour the place already
  had. Measured: `find place --type process | count` 6.16 s → 0.59 s, a failing `enter` 23.93 s →
  4.76 s (debug), 1.40 s in release with 0.27 s of that CPU;
  `spatial_contracts_missing.rs` 2 timeouts → 27/27 green. **The earlier entry mis-stated the
  budget**, and the correction matters: §34's 50 ms figures are for `look` and `near` *cached*
  (0.10 s and below here), and §34 says in as many words that "Cold provider discovery MAY exceed
  these targets". A one-shot `ono -c` is cold discovery.

- [x] **§34's socket budget is missed by ~20 %, measured — fixed 2026-08-30 (ADR-0418).**
  Acceptance case `152` ran for the first time on 2026-08-29 and failed honestly:

  ```text
  ordinary host (2 sockets)      first socket row      46 ms   (§34 budget 50 ms) within
  5002 listening unix sockets    first socket row      60 ms                      OVER
  5002 listening unix sockets    first connection row  61 ms                      OVER
  5002 listening unix sockets    whole socket table    67 ms   (budget 5000 ms)   within
  5002 listening unix sockets    cold start            21 ms                      within
  ```

  The shape said what was wrong: the whole table cost 67 ms and the *first row* cost 60 ms, so
  `take 1` paid for the enumeration of all 5002 before it answered. Both halves of that are gone.
  The decoders (`inet_sockets`, `unix_sockets`) are iterators that turn one netlink message into
  one record at a time, and `snapshot` hands objects over in batches of 64 through a channel that
  holds one batch, so a consumer that stops stops the reader — including the dumps it had not
  issued yet. The `connection` target no longer asks for the Unix table at all: every Unix socket
  carries `remote: null`, so none of them can be a connection.

  Measured in the container after the change, on a host whose baseline had drifted to 45 ms under
  load: first socket row **47 ms**, first connection row **43 ms**, cold start 20 ms, whole table
  78 ms. What the fixture costs above an ordinary host went from **+20 ms to +2 ms**. Case `152`
  is green, and with it the acceptance job. The whole table is ~24 ms dearer because every record
  now crosses a channel; §34 budgets the interactive path, and that is the one that got cheap.

  Tests: `kernel_providers::should_answer_exactly_the_bound_when_a_socket_query_asks_for_one` and
  `::should_answer_no_unix_socket_when_the_connection_target_is_asked`; the decoding fidelity of
  the new iterators is held unchanged by `socket_decoding` and `malformed_messages`.

Three subsections stood here — *Providers, remote and KUANG/11*, *Spatial*, and *Harness and
bookkeeping*. Every entry in all three was still open on 2026-08-31, so all of them are issues
and the headings are gone with them.

#### Found while recording the README figures (2026-08-30, agent `readme-demo`)

Five defects, each reproduced at `a057be1` — against `target/release/ono` on an ordinary host
(920 processes), or against the demo image `docker/demo.Dockerfile` (`ono-sendai:demo-recording`,
4 processes, nginx on 8080, redis on 6379). None was fixed there; §4 keeps the fix out of a
`docs:` commit. Three of the five were found by composing the recordings in `docs/assets/`, which
is the argument for keeping that harness: it types what a reader would type.

**Four of the five were closed on 2026-08-31** (`f8e0fb6`, `e8426e5`, `41e9688`;
ADR-0419/0420/0421) and are below. The fifth, the `map --live` hang, is issue #22. Fixing the four
turned up five more, in their own block below; two of them are halves of these boxes that were
reported as one defect and are not — `select`'s share of the float rendering (#23), and
`follow owner` (#25).

- [x] **A nested record renders as its schema id in a table** — fixed 2026-08-31, `f8e0fb6`,
  ADR-0419, in one increment with the float box below. One cause under both: `Renderer::text`
  matched on the value alone, and two of the things §13 prints are not in the value. `field_cell`
  now reads the field's declaration (§13.1 point 1) — a record-valued cell renders the record
  (`{address: 127.0.0.1, port: 631}`), a `ref<S>` renders what a person calls that object (`root`,
  or its identity where none resolved, §23.6), and `unit: percent` renders `2.1%`. Verified
  against `target/release/ono`: `get process 1` prints `2.1%` and `root` where it printed
  `2.085903083563699` and `ono.user/1 {uid: 0}`; `| to json` is byte-identical. Eleven tests in
  `crates/ono-render/tests/value_nested.rs`, two of which hold the line the other way (an
  undeclared float keeps its precision, a `unit: bytes` integer stays `65536`).
  **The README was left on `local.port` deliberately.** The line reads better than the restored
  `local` would (`{address: 0.0.0.0, port: 5432}` in a narrow block) and it shows field-path
  projection, which nothing else in that section does. The reason the box asked for the restore
  was that `local` was broken; it is not any more.

- [x] **`look` empties the neighbourhood it just described** — fixed 2026-08-31, `41e9688`,
  ADR-0421. **It reproduces on an ordinary host too**, against a listener the caller owns, so the
  demo image was not the condition. The diagnosis: §32.1 forbids a default `look` from paying for
  the `/proc/<pid>/fd` scan, so it records `process: unknown — available on request` (§32.2);
  `relation_summary` puts a refusal ahead of a count (§35.2, §42.4) and reads that record first;
  and the `near --type process` after it *does* pay for the scan and *does* record the owner edge,
  which nothing ever reached. The record was written once and never removed, so it outlived the
  statement it recorded. `SpatialIndex::clear_withheld` now forgets one exit's record, and
  `relations::observe` calls it for the labels of every provider it is about to consult, before
  consulting it. `relation_summary` is untouched — its rule was right, the record was stale.
  Exit test:
  `spatial_relationships_missing.rs::should_answer_the_same_neighbours_whether_or_not_a_look_came_first`,
  which compares the two answers rather than asserting either, so both being empty cannot satisfy
  it. **The `follow owner` half of the box was mis-attributed** and is now its own line below: on
  an ordinary host it answers E1009 with *and* without the `look`.
  The original report, kept for its reproduction:

  ```text
  ono -c 'enter 0.0.0.0:8080; near --type process'          → owner nginx, owner nginx
  ono -c 'enter 0.0.0.0:8080; look; near --type process'     → nothing at all
  ```

  With the `look` in between, `follow owner` also stops working: it answers `Ono-Sendai-E1009
  spatial.unsupported … available on request` where the same command without the `look` answers
  `Ono-Sendai-E1002 spatial.ambiguous_selector` and names both nginx lifetimes — the honest
  refusal §6.4 asks for. So `look` writes a place view whose expensive relations are recorded as
  *withheld*, and the later `near` reads that view instead of resolving them. Suspected site, not
  a diagnosis: the two places that mint `available on request`,
  `crates/ono-cli/src/spatial/view.rs:146` and `crates/ono-cli/src/spatial/relations.rs:534`.
  This is a correctness defect in the v0.4 surface — a reader who looks before they walk is told
  the place is empty. Exit test: a spatial test asserting `near --type process` answers the same
  rows with and without a preceding `look`, plus the acceptance case for the discovery scenario
  doing the `look` first.

- [x] **`trace --relations` restricts nothing** — fixed 2026-08-31, `e8426e5`, ADR-0420. The
  filter was never at fault: `TraceOptions::wants` is correct, and `--relations child`, written as
  a word, restricted the walk all along. A bracketed list is an *expression*, ADR-0009 keeps it
  unevaluated, and both `BoundArguments::option` and `CommandContract::query` answer for value
  bindings only — by design, and documented. `trace` read the absence as "not written" and walked
  unrestricted in silence. It now evaluates its bound arguments against the invocation's scope
  first (ADR-0219, as `mutate` already did), so `--relations ["child"]` restricts and
  `--relations [child]` refuses — `child` names no variable. `trace process $p` narrows at the
  provider as a by-product. Three tests in `crates/ono-cli/tests/native.rs`; the list one derives
  its precondition from the unrestricted answer, so two empty answers cannot satisfy it.

- [x] **A float field renders at full precision beside a formatted one** — fixed 2026-08-31,
  `f8e0fb6`, ADR-0419, the same increment as the record-cell box above. `get process` and
  `watch process` print `2.1%`. **`select` still does not**, and that half is its own line below:
  the projection erases the declaration the renderer now reads. The original report:
  `get process | where
  cpu > 1 | take 3 | select pid name cpu` prints `2.4491293271514514`, in the same table where
  `memory` prints `11.60 MiB`. `watch process` shows it too, which is where it hurts — it is the
  one view a reader watches rather than reads. The JSON is right (`{"cpu":2.4491289426573135}`),
  so this is the renderer again, and it belongs to the same increment as the record-cell defect
  above. Exit test: a render test fixing the human rendering of a percentage-typed field, with the
  serialisation left untouched.

**`scripts/acceptance.sh` stands at 107 passed, 0 failed at `41e9688`** (2026-08-31), and
`scripts/gate.sh` printed `gate: green` on the same tree. One measurement is worth recording
beside it: case `152` **failed** on a first run of the same tree and passed on a second. It was
not the tree. The first run went out while a `cargo test --workspace` and a container build were
on the same eight cores at load 18–68, and the case reported its *baseline* — the ordinary host,
two sockets — at 52 ms against §34's 50 ms budget, with the pathological host only 6 ms above it.
A budget measured under that load says nothing, which is what the §34 box said all along. The
second run, on a quiet machine, was green. Read it as one more reason issue #13 stays open: these
five cases need a machine nobody else is using.

#### Found while fixing three of the README-figure defects (2026-08-31)

Five defects, each reproduced against `target/debug/ono` or `target/release/ono` on an ordinary
host at `41e9688`, and each deliberately left out of the fix that found it (AGENTS.md §4). All
five were still open when the board was filed and are issues now.

---

### Done, reconciled with the tree at `b904327` (2026-08-29, agent `STATE-recon`)

**Fifty-five boxes closed.** Every line names the commit that closed it and the evidence that
proves it; where the behaviour is observable from the command line, the answer quoted is the one
`target/debug/ono` gave during this reconciliation. `[run]` marks a line settled by running the
shell, `[test]` one settled by reading the named proof at HEAD.

Nothing in this section was fixed by this agent. It writes back what fifty-nine commits between
`ed923ee` and `b904327` had already delivered, and which no board update had recorded.

#### Data and pipeline — nineteen of twenty

| Was | Closed by | Proof |
|---|---|---|
| **B-data-1** a dotted path into an error value yielded the error, not the field | `c354dd2` (ADR-0215) | `[run]` `unmount filesystem / \| select error.name \| to json` → `[{"name":"io.permission_denied"}]` |
| **B-data-2** `diff` reported `changed` for two byte-identical records — provenance carried the instant of observation | `79f58df` (ADR-0229) | `[run]` `get user root \| diff (get user root) \| to json` → `[]`; `data_missing.rs::should_report_two_fresh_snapshots_of_one_object_as_unchanged` |
| **B-data-3** `@` was null inside an `each` block whose body was a native command | `0820686` (ADR-0219) | `[run]` `get process \| where pid == 1 \| each { get process @.pid \| count }` → `1`; `language_missing.rs::should_bind_the_iterated_item_for_a_native_stage_inside_an_each_block` |
| **B-data-4** `to bytes --field <name>` refused a record | `df54ffc` (ADR-0223) | `[test]` `ono-value/tests/text_and_bytes_codecs.rs::should_write_the_named_field_of_each_record_as_raw_bytes`, `adapters.rs::should_write_one_field_s_bytes_verbatim_when_to_bytes_names_it` |
| **B-data-5** a `SIGPIPE`d stdout was reported as `io.permission_denied` | `85d2983` (ADR-0220) | `[run]` `get process \| to json` into a closed reader → stderr empty; `cli.rs::should_stop_quietly_when_the_reader_of_its_output_goes_away`, case `034` |
| **B-data-6** JSON object key order was alphabetical, not schema order | `320387a` (ADR-0228) | `[run]` `echo '{"zebra":1,"alpha":2}' \| ono -c 'from json \| to json'` → `[{"zebra":1,"alpha":2}]` |
| **B-data-7** `--name=value` was a parse error in expression mode | `1b4f16d` (ADR-0227) | `[run]` `from json \| reduce $acc + @ --initial=10` over `[1,2,3]` → `16`; `parse_expressions.rs::should_read_the_equals_spelling_of_a_long_option_when_the_mode_is_expression` |
| **B-data-8** the pipeline's `Diagnostics` counters never reached the user | `3221510` (ADR-0261) | `[test]` `data_missing.rs::should_say_how_many_values_a_condition_could_not_be_decided_on`, `::should_say_how_many_unknown_values_an_aggregate_skipped`, `::should_say_nothing_when_the_pipeline_dropped_nothing` |
| **B-data-10** `sort` after an external head could not reach GNU sort's flags | `78f5935` (ADR-0260) | `[run]` `printf 'b\na\nc\n' \| ono -c 'sort -r'` → `c b a`; `builtins.rs::should_run_the_program_with_its_flags_when_a_native_head_is_reached_by_bytes`. Residue on record in ADR-0260: `sort -k1,2` is still a parse error, `sort '-k1,2'` works |
| **B-data-11** `get log`'s `level` compared alphabetically, not by severity | `ae60d8c` (ADR-0222) | `[test]` `expressions.rs::should_order_an_enum_field_by_its_declared_values_when_compared`, `services_logs_missing.rs::should_order_the_level_by_severity_rather_than_by_spelling`. **The old entry's reproduction is obsolete**: `from json \| where level >= "error"` still keeps `warning`, and that is now correct — the decision orders a field the *schema* declares an enum, and a schemaless string field still compares as text (ADR-0222, deliberately) |
| **B-data-12** `get process <gone-pid> \| count` printed an error and a value and exited 0 | `ca0efde` (ADR-0221) | `[run]` `get process 999999 \| count` → the `io.not_found` refusal, no `VALUE 0`, **status 1**; `processes_missing.rs::should_fail_the_run_when_a_named_process_is_not_there_and_a_count_follows` |
| **B-data-13** the E0701 bulk-guard message carried runs of spaces | `f784422` | `[run]` `get process \| stop process` → one clean line, "…more than the bulk threshold of 100" |
| **B-data-14** two label rules for one object | `733af7b` (ADR-0224) then `9130d29` (ADR-0226, superseding it) | `[run]` `unmount filesystem /` and `get mount / \| unmount filesystem` both render `ono.mount/1[/]`; `storage_missing.rs::should_label_the_unmounted_mount_the_same_way_whichever_spelling_named_it` |
| **B-data-15** `get config --problems \| select code` failed the pre-flight check | `5708edb` (ADR-0218) | `[run]` no E0202; `meta_config_missing.rs` (the `--problems \| select code name` assertion) and `::should_still_refuse_a_field_neither_side_of_the_declared_union_has` |
| **B-data-16** `trace group root` reported "command not found: trace" | `83dedd1` (ADR-0217) | `[run]` → `Ono-Sendai-E0102 resolve.target_not_found \`trace\` has no target \`group\``; `meta_config_missing.rs::should_name_the_target_when_a_known_verb_was_given_one_it_does_not_have` |
| **B-data-17** `get interface lo \| stop interface` refused the piped record | `05910b8` (ADR-0216) | `[run]` → one `failed` ActionResult naming `lo`, no type error; `network_missing.rs::should_act_on_the_piped_interface_when_a_record_arrives_instead_of_a_selector` |
| **B-data-18** `explain` inside a frame did not print the narrowed spelling | `a419137` (ADR-0225) | `[run]` `enter process 1; explain get process` → `narrowed  get process 1`; `context.rs::should_print_the_narrowed_spelling_when_explaining_inside_an_object_frame` and three siblings |
| **B-data-19** `watch` narrowed at the query level rather than on the argument seam | `fd06ebe` (pin), `1ea33fb` (the `--service` prerequisite), `a3ec71a` (the refactor) | `[test]` `watch.rs::should_narrow_a_watch_to_the_entered_object_rather_than_the_whole_machine`, `::should_refuse_a_watch_the_context_cannot_narrow_rather_than_widening` — pinned first, unchanged across the refactor |
| **B-data-20** §17.5 redaction did not reach §20.2's retention | `76d7eed` (ADR-0262) | `[test]` `native.rs::should_keep_a_secret_out_of_the_retained_result_as_well_as_out_of_history` and two siblings |

#### Providers — all twelve

| Was | Closed by | Proof |
|---|---|---|
| **B-prov-1** `get process \| count` exited 1 on a churning host | `4e53ee4` (ADR-0230) | `ESRCH` from a `/proc/<pid>/stat` read is `io.not_found`, not `provider.unavailable`; `common::tests::should_read_esrch_as_the_object_being_gone_when_a_procfs_read_fails`. 0 failures in 60 runs under four forking shells, against 2 in 40 before. **The shell was wrong and the three tests were right** — none of them changed |
| **B-prov-2** `cpu` was `null` in every one-shot run | `dfa66eb` (ADR-0232) | `ono.process/1` gains `cpu_window`; a single read answers the lifetime share, `--sample <duration>` buys the rate. `process.rs::should_report_the_share_over_the_process_lifetime_when_nothing_earlier_was_observed`, case `120-process-cpu-share` |
| **B-prov-3** two TIME_WAIT sockets were one place | `79d6a9c` (ADR-0231) | a record supplying none of its identity components states no identity; `ono-provider-api/tests/contract.rs::should_refuse_to_identify_a_record_whose_every_identity_component_is_null`, `::should_not_make_two_records_the_same_object_because_both_have_no_identity` |
| **B-prov-4** `trace service` was registered and executed by nothing | `d1a6e5f` | `services_logs_missing.rs::should_trace_a_service_to_the_processes_it_owns`, `::should_refuse_to_trace_a_service_that_does_not_exist`. No production code changed |
| **B-prov-5** `service.depends_on` had no provider evidence | `bf25291` (ADR-0239) | `GetAll` already carried `Requires`/`Requisite`/`BindsTo`/`Wants`; `ono.service/1` now has `dependencies`. `relationships.rs::should_link_a_service_to_the_units_it_requires` and four siblings |
| **B-prov-6** `trace mount` had no propagation peers | `bf3e8ee` (ADR-0236) | `ono.mount/1` gains `peer_group` from `mountinfo(5)`; `relationships.rs::should_link_a_mount_to_the_other_mounts_of_its_propagation_peer_group`, case `122-mount-propagation-peers` under `CAP_SYS_ADMIN` |
| **B-prov-7 / B-prov-8** `watch` polled everywhere; no watch reported `source: subscription` | `cc0ca32` (ADR-0235), `1789185` (ADR-0241) | `watch file` on inotify, `watch interface`/`route` on the rtnetlink multicast groups, `tail --follow` waits on the file. `files_missing.rs::should_report_a_created_file_before_the_next_poll_would_have_come`, `network_missing.rs::should_watch_interfaces_through_the_kernel_rather_than_by_asking_it_again` and its route sibling |
| **B-prov-9** `--preserve` did not restore timestamps on a copied directory | `8dccec2` (ADR-0234) | `utimensat(2)` by path reaches a directory, a read-only file and a symlink, and its failure is reported. `files_missing.rs::should_preserve_the_timestamps_of_a_copied_tree_when_preserve_is_given`, case `121-copy-preserves-a-tree` |
| **B-prov-10** nothing made ignoring a declared option impossible | `e0c6eec` (ADR-0233), with `e8ca19d` fixing the three that were being ignored | `xtask/tests/contracts.rs::should_report_a_declared_option_no_implementation_names`, `::should_not_accept_an_option_named_only_by_a_test` |
| **B-prov-11** network write paths had no privileged conformance | `a96b337` (ADR-0237), `6218424` (ADR-0238), `bb5dca3` | a case may now name `capability:`, `user:` and `security:`; case `123-privileged-network-writes` runs all nine mutations against a live kernel under `CAP_NET_ADMIN`, `stop socket` included |
| **B-prov-12** `resolve dns --server <ip>` was refused as `provider.unsupported` | `3e19bc3` (ADR-0240) | `ono-provider-net` gains an RFC 1035 client (A, AAAA, PTR; UDP with a TCP retry; every length bounded). `network_missing.rs::should_answer_from_the_nameserver_that_was_named_rather_than_from_the_system_resolver` and three siblings, case `124-dns-named-server` |

#### Remote, KUANG/11 and spatial — eleven

| Was | Closed by | Proof |
|---|---|---|
| **B-remote-1** inside a link frame, `get link` was sent to the other side | `2d80a5c` (ADR-0269, superseding ADR-0103's note) | `[run]` `link host testbox --transport local; enter link testbox; get link \| count \| to json` → `[1]`; `remote.rs::should_answer_for_this_sessions_links_and_jobs_from_inside_a_link_frame`, `::should_detach_the_link_it_is_standing_in_when_the_link_table_feeds_the_mutation`, case `044` |
| **B-kuang-1** `grant capability --scope`/`--duration` were declared and ignored | `a5be21b` (ADR-0264) | `--scope <key>=<value>` validated against the capability's declared keys, `--duration` making a lease the broker checks; `plugins_missing.rs::should_record_the_scope_the_operator_named_on_the_grant`, `::should_refuse_a_scope_key_the_capability_does_not_declare`, `::should_make_a_lease_that_expires_when_a_grant_is_given_a_span` |
| **B-kuang-2** grants, leases and the audit trail did not survive a session | `a5be21b` (ADR-0265) | `always` grants to `<config>/kuang/policy.yaml`, the trail appended to `<state>/kuang/audit.jsonl`; `plugins_missing.rs::should_read_back_an_always_grant_in_a_later_session`, `::should_forget_a_stored_grant_when_it_is_revoked_in_a_later_session`, `::should_keep_the_audit_trail_across_sessions`, case `125-kuang-capability-policy` |
| **B-kuang-4** the negotiated `OverflowPolicy` was never enforced | `1665e1e` (ADR-0267) | `host_emit` consults it; `fail-stream` raises K11206, which nothing raised before. `ono-kuang-sdk/tests/conformance.rs::should_end_the_stream_and_keep_the_instance_when_the_negotiated_overflow_fails_the_stream`, `::should_keep_the_oldest_values_and_drop_the_rest_when_the_overflow_drops_the_newest` |
| **B-kuang-5** `contributions.views`/`annotations` were listed and never loaded | `c111f91` (ADR-0268) | the box's second permitted outcome: an annotation key outside the package namespace is `package.invalid` at parse time, a view contribution is `package.incompatible` naming the `view_protocol` dimension. `manifest_validation.rs::should_refuse_an_annotation_key_outside_the_packages_namespace` and two siblings. Registering a view stays a tranche inside C-4 |
| **B-kuang-6** the seven `docs/spec/kuang/*.v1.yaml` contracts had no drift check | `122dcea` (ADR-0266) | `check_kuang_contracts` holds four of the seven against `crates/ono-kuang-*` both ways; the manifest half asks the parser rather than mirroring it. `xtask/tests/contracts.rs::should_match_the_kuang_contracts_against_the_runtime_that_serves_them` and two siblings |
| **B-spat-1** every read-only mount at 100 % was a "storage pressure" landmark | `13b6157` (ADR-0270) | `[run]` `enter storage; look` lists exits and no snap landmark; `ono-spatial-query/tests/landmarks.rs::should_not_promote_a_read_only_filesystem_that_is_full_as_storage_pressure`, `::should_still_promote_a_writable_filesystem_above_the_threshold`, `::should_still_promote_a_full_filesystem_that_does_not_say_whether_it_is_writable` |
| **B-spat-2** a map of an object was a flat list with unusable rows | `c064639` (ADR-0272) | `[run]` `enter process 1; map` — every row reads `— process.parent_of`, and `containerd-shim (pid 171616)` tells four namesakes apart; `ono-spatial-render/tests/object_map.rs::should_name_the_relation_every_neighbour_of_an_object_stands_in`, `::should_tell_two_neighbours_sharing_a_display_name_apart`, case `105` s5ae |
| **B-spat-3 / the S11c `help here` line** `help here` did not exist (v0.4 §38.2) | `13b6157` (ADR-0271) | `[run]` `enter process 1; help here` names every exit with what is behind it, the spelling that traverses it, and `permission_denied` where the provider gave one; `ono-cli/tests/spatial_help.rs` (three cases), `ono-command/tests/completion.rs` (two topic cases) |
| **B-spat-4** `near --relation` did not name the positional spelling; an empty `near` was silent | `13b6157` (ADR-0271), corrected by `b904327` (ADR-0275) | `[run]` `near --relation process` → "`relation` is a positional selector: write `near <relation>`"; `enter process 1; near socket` → `Ono-Sendai-E1008 spatial.permission_denied … /proc/1/fd`. `spatial_navigation_missing.rs`' three near cases, `ono-spatial-query/tests/neighborhood.rs::should_keep_the_exit_at_this_end_of_a_relation_rather_than_the_one_at_the_other`, case `102` s4w/s4y/s4z |
| **B-spat-5** a tombstone never named its replacement candidate | `4e1f23f` (ADR-0273) | a tombstone keeps at most eight sources with their relations, captured before `forget_edges`, and asks each once when it is rendered; a source reaching several live objects is discarded. `ono-spatial-core/tests/trail.rs::should_keep_the_source_that_reached_a_place_so_a_candidate_can_be_asked_for_later`, `::should_name_the_replacement_once_one_has_been_identified`, `::should_keep_the_first_candidate_rather_than_revising_it`, case `096` `44.7e` — which is this box's own exit test |

#### Harness, bookkeeping and the split remainders — thirteen

| Was | Closed by | Proof |
|---|---|---|
| **B-harn-1** `check_commands` skipped its cross-checks for a command omitting the field | `25b6139` (ADR-0246) | `verb`, `target` and `argument_mode` are required keys; `target: null` stays a valid declaration. `xtask/tests/contracts.rs::should_reject_a_command_that_declares_no_verb` and three siblings |
| **B-harn-2 / B-harn-3** the unfinished-work scan excused all of `xtask/tests/`, and its walk of `tests/`, `fuzz/` and `examples/` was unproven | `80c8cb7` | the excuse is now two files; the walk is pinned by a fixture per tree, plus a guard that reports a top-level Rust tree the list does not walk. `xtask/tests/scan.rs::should_reject_a_placeholder_in_an_xtask_test_that_is_not_the_scanners_own`, `::should_scan_every_rust_tree_the_repository_layout_allows`, `::should_report_a_rust_tree_the_scan_does_not_walk` |
| **B-harn-4** `docs/ACCEPTANCE.md` claimed a generation that does not exist | `42713e4` (ADR-0248) | §4.1 D and §4.7.4 now describe the drift check they really have; the boxes stay ticked because the drift check is real and green. `xtask/tests/reference.rs`' three tests, one of which failed on exactly those two sentences |
| **B-harn-5** ADR-0015's threat-model table named intentions, not tests | `9d7f16a` (ADR-0245, superseding ADR-0015) | same fifteen threats, same mitigations, a *Proven by* column naming test functions. `xtask/tests/spatial_evidence.rs::should_find_every_test_the_threat_model_names` turns the gate red when a named proof goes missing — verified by pointing T1 at a test nobody wrote |
| **B-harn-6** five behaviours verified by running the shell had no regression test | `5f683e0` | one outcome test each: `read file` on 20 MB as one value, `explain remove file *.txt` naming the glob and removing nothing, `--user root` leaving out what root does not own, `trace connection --remote` over a loopback connection the test opens itself, and `unmount filesystem /etc`'s own sentence |
| **B-harn-7** §27.2's binding check was never run against the real table | `42713e4` (ADR-0247) | `xtask::bindings::check_bindings` runs `unbound_stable_commands` over the embedded registry and `builtin_commands`, with `BOUND_ELSEWHERE` naming what binds each of the fifty-two; both directions fail. `xtask/tests/bindings.rs`, three tests |
| **B-split-B8** the §12.3 refusal was proven only in the container | `73aa3a0` | one workspace test each way, including a child process's invalid-UTF-8 stdout reaching a pipeline value losslessly (§12.2) |
| **B-split-D4** (first half) completion answered only static registry metadata | `4a91ec8` (ADR-0252) | `ProviderValues` fills the `ValueCompleter` seam; `completion.rs::should_offer_this_machines_users_when_completing_a_user_selector`, `::should_answer_a_completion_that_no_provider_can_serve_without_waiting_for_one`, case `044`. **The budget half stays open above** |
| **B-split-E3** no `vcs` prompt segment, and the object-context segment was unasserted | `7cad7d6` (ADR-0250) | case `113-prompt-segments` |
| **B-split-E5** the *bounded* half of §20.2's retention was untested | `e6094eb` (ADR-0249) | the seventeenth result evicts the first, and a result over the value bound is retained truncated and says so |
| **B-split-J1 / B-split-J5** `view tree` was exercised by nothing, and `get link \| view table` was proven only in halves | `365ac0a` | `view.rs::should_open_the_tree_view_over_a_graph_and_leave_the_pick_behind` drives a real PTY; cases `111-view-tree-navigation` and `112-link-overview` |
| **the acceptance image was grading the wrong program** — not a box, and it outranked every box (AGENTS.md §14) | `9169db9` (ADR-0251) | `COPY` preserves mtimes and the Dockerfile caches `target/`, so cargo declared crates fresh and the image was built around a previous binary while carrying the new source. Observed, not imagined. The build now stamps `crates/`, `xtask/` and `docs/spec/` before compiling; `xtask/tests/packaging.rs::should_stamp_the_workspace_before_building_when_the_image_caches_its_target_directory` |
| **a journal ordering test compared timestamps as text**, and a dry-run case read a record in the old key order | `d245b96`, `db27dd1` | both were tests wrong about data, not the shell wrong about behaviour (AGENTS.md §11) |

---
### Done, verified today (2026-08-29, agent `triage`)

Forty-eight boxes moved. The §37 phase lists below are now ticked with the proof beside each. The
ten most consequential, with the evidence that decided them:

1. **E1 — the context stack** — `ono-cli/tests/context.rs::should_enter_a_directory_and_leave_back_out_of_it`,
   `::should_show_the_stack_when_asked_for_the_context`, `::should_stay_on_the_ground_when_there_is_nothing_to_leave`;
   case `045-context-and-reuse.case` (`stdout-contains: /etc`, `filesystem`, `nothing to leave`).
2. **G3 — `trace`** — `ono-command/tests/trace.rs::should_answer_a_trace_with_a_graph_rooted_at_the_named_object`,
   nine walk tests in `ono-graph/tests/trace.rs`, case `047-relationship-graph.case`
   (`+-- child ->`). Split closed since, by `d1a6e5f`: `trace service` is driven by
   `services_logs_missing.rs::should_trace_a_service_to_the_processes_it_owns`.
3. **H1 — `ono-protocol`** — `framing.rs::should_refuse_a_frame_claiming_more_than_the_limit_before_allocating`,
   `handshake.rs::should_prefer_the_highest_version_both_ends_speak`, `streams.rs`'s four
   multiplexing tests, seventeen `messages.rs` round trips; case `049-remote-link.case`.
4. **I2 — `ono-kuang-protocol`** — `frame.rs`, `message.rs`, `version.rs`, `error.rs`
   (`should_expose_all_27_codes_of_spec_31_79_when_enumerated`), `lifecycle.rs`, `capability.rs`
   (`should_carry_all_29_families_of_the_registry_when_enumerated`), and the wire proof
   `ono-kuang-sdk/tests/conformance.rs::should_quarantine_a_plugin_that_breaks_framing`.
5. **I10 — the plugin conformance suite** — `ono-kuang-sdk/tests/conformance.rs`, seventeen tests
   over every §31.74 area: manifest validation, the four denial paths, cancellation, backpressure
   both directions, quota exhaustion, four protocol-violation quarantines; case `050`.
6. **B10 — `ActionResult` and partial failure** —
   `ono-command/tests/mutations.rs::should_keep_a_mixed_result_apart_rather_than_collapsing_it`
   (the exact `[Success, Failed, Success]` sequence) and `::should_answer_with_one_outcome_per_object_that_arrived`;
   case `037-files-read-write-remove.case` (`[{"status":"success"},{"status":"success"}]`).
7. **D6 — `explain`** — `ono-command/tests/explain.rs::should_render_a_plan_in_the_shape_of_spec_42_1`
   plus six siblings; case `043-discoverable-from-the-shell.case` proves non-execution with
   `stdout-contains: explain-never-ran` **and** `stdout-not-contains: RAN`.
8. **D5 — `type` and `inspect`, causal chain included** —
   `ono-command/tests/meta.rs::should_show_the_causal_chain_of_an_error` asserts `chain.len() == 1`
   with the nested `io.permission_denied` ("spec §16.2: the whole causal chain, not only the
   top"), beside `::should_show_every_field_with_how_it_was_known_and_where_it_came_from`.
9. **F1/F2 — watch and in-place rendering** —
   `ono-command/tests/watch.rs::should_begin_a_watch_with_the_current_state_and_then_the_changes`
   (snapshot first, `changed` naming its field, `source: "poll"`), `ono-cli/src/live.rs`'s
   `should_report_no_change_when_an_event_repeats_the_shown_state`, and
   `watch_live.rs::should_render_in_place_at_a_terminal_and_stop_on_ctrl_c`; case `046`.
10. **I8 — contributed relations reach the spatial map** — the one that surprised the audit:
    `docker/acceptance/cases/110-spatial-contributions.case` `s9-a` … `s9-g` prove no edges
    without the grant, the edge present with it, `"provider":"dev.example.echo"` and
    `"confidence":"strong"` — never `exact` — plus
    `ono-kuang-testhost/tests/spatial_package.rs::should_refuse_a_package_that_declares_relations_without_asking_for_the_capability`.

Eight non-phase entries moved with them, each verified by running the shell today: keyless `sort`
(`language_missing.rs::should_sort_scalars_by_themselves_when_no_key_is_given`, and the string and
descending forms); `kill %N` on a native job
(`::should_stop_a_native_job_when_kill_names_it_by_job_number`); backgrounding a pipeline with
native stages (`jobs_native.rs::should_finish_a_bounded_background_pipeline_and_say_so`); the
`container`, `link` and `host` `*-event/1` schemas, all three written and watched
(`containers_packages_missing.rs:697`, `remote_missing.rs:458`, `:485`); `read file` on a 20 MB
file as one value; `get process --user root` narrowing; `unmount filesystem /etc` reaching the
provider's own wording; and `trace connection --remote <ip>`. The last five had no regression
test at the time; `5f683e0` gave each one, so **B-harn-6 is closed**.

---

Phase A is decomposed to increment level. Later phases are listed at their coarse shape and are
decomposed by the agent that starts them — decomposing early would invent detail the spec does
not fix yet.

## Phase checklists (spec §37)

The ten phases of spec §37, each box naming the automated proof that ticks it. All ten are
complete and tagged `phase-a` … `phase-j`; the boxes that remain open name the issue that
carries them.

### Phase A — Language and Unix shell foundation

**Phase A is complete.** Its exit criterion from spec §37 is proven by the acceptance case
`010-replaces-bash-for-ordinary-work`, and `docs/ACCEPTANCE.md` §4.1 A is ticked. The performance
budgets of §34 are tracked under *Cross-cutting*, not here.

- [x] A1 — Lexer: tokens, spans, quoting and escaping corpus — spec sections 6, 26 —
      exit test: `crates/ono-parser/tests/lexer.rs` golden corpus
- [x] A2 — Parser and AST with recoverable errors and precise spans — spec sections 24.4, 26 —
      exit test: golden AST snapshots + diagnostics snapshots
- [x] A3 — Incremental/partial parse for a line being typed — spec section 24.4 —
      exit test: partial-input parse tests
- [x] A4 — Evaluator skeleton: run an external command, propagate exit status — spec section 29 —
      exit test: acceptance `020-runs-external-commands`
- [x] A5 — Environment variables, `cd` and working directory — spec section 19 —
      exit test: acceptance `021-cwd-and-environment`
- [x] A6 — Redirection: `>`, `>>`, `<`, fd duplication, deterministic non-TTY behaviour —
      spec sections 12, 29 — exit test: acceptance `022-redirection`
- [x] A7 — External pipelines and exit status of a pipeline — spec section 11 —
      exit test: acceptance `023-external-pipelines`
- [x] A8 — PTY execution for full-screen programs — spec section 29 —
      exit test: acceptance `024-pty-applications`
- [x] A9 — Signals, process groups and foreground/background job control — spec section 18 —
      exit test: acceptance `025-job-control`
- [x] A10 — Line editor: keymap, editing, syntax highlight from the incremental parse —
      spec section 24.1 — exit test: editor behaviour tests + latency budget
- [x] A11 — History persistence and recall — spec section 20 — library done
      (`crates/ono-history/tests/history.rs`); wiring and acceptance case
      `026-history-survives-restart` land with the REPL
- [x] A12 — Configuration loading, with no eager plugin load and no network at startup —
      spec section 30 — exit test: acceptance `027-startup-is-quiet`
- [x] A13 — Prompt with location URI and privilege indication — spec sections 4, 17 —
      exit test: acceptance `029-prompt-shows-context.case` (the box named `028`, which is
      `028-config-is-restricted`; corrected 2026-08-29), and `113-prompt-segments.case` since
      `7cad7d6`
- [x] A14 — Structured error model and exit-status contract — spec sections 16, 43 —
      exit test: error taxonomy tests
- [x] A15 — Phase A gate: `ono` as a login shell doing a real working session —
      exit test: acceptance `010-replaces-bash-for-ordinary-work` — **Phase A complete**

### Phase B — Value system and native pipelines (spec §10, §11, §12, §13, §25)

- [x] B1 — Value model: scalars, semantic scalars, units, `Record`, `Map`, `List`, provenance —
      `crates/ono-value/tests/` — ADR-0016 — commit d020129
- [x] B2 — Schema model and registry, the canonical schemas of spec §28, compatibility rules —
      `crates/ono-value/tests/{builtin_schemas,schema_compatibility}.rs` — commit d020129
- [x] B3 — Stream engine: bounded channels, backpressure, cancellation, the streaming/blocking
      distinction — `crates/ono-pipeline/tests/{backpressure,boundedness,cancellation}.rs`
- [x] B4 — Transforms `where`, `select`, `take`, `skip`, `each` (streaming) — spec §53 —
      `crates/ono-pipeline/tests/streaming_transforms.rs`, `crates/ono-command/tests/transforms.rs`
- [x] B5 — Transforms `sort`, `group`, `count`, `measure`, `reduce`, `join`, `diff` (bounded) —
      `crates/ono-pipeline/tests/blocking_transforms.rs`
- [x] B6 — Conversion `to`/`from` json, yaml, csv, text, bytes — `crates/ono-value/tests/`
- [x] B7 — Renderer separated from data: table, stacked, list, tree, raw, hex; width-aware
      layout; visible truncation; semantic theme tokens — `crates/ono-render/tests/`
- [x] B8 — Object-to-external and external-to-object boundaries — spec §12.2, §12.3 —
      `docker/acceptance/cases/040-object-pipeline.case` (`boundary-refused`: an object aimed at a
      raw program is refused naming `to json`), `crates/ono-value/tests/roundtrip.rs:190`
      ("undecodable bytes must never be lost") and
      `codec_properties.rs::should_round_trip_every_generated_byte_string_through_the_raw_form`.
      **Split closed** by `73aa3a0`: `ono-cli/tests/native.rs` now asserts both in the
      workspace — the §12.3 refusal, and a child process's invalid-UTF-8 stdout reaching a
      pipeline value losslessly (§12.2).
- [x] B9 — Pipeline type-checking before execution — spec §11.3 —
      `crates/ono-command/tests/expressions.rs::should_report_an_unknown_field_with_a_suggestion_before_the_pipeline_runs`,
      `pipeline.rs::should_report_the_typo_and_run_nothing_at_all`,
      `crates/ono-cli/tests/native.rs::should_reject_a_misspelled_field_before_anything_runs`,
      case `042-inspection-without-text-parsing.case` (`perhaps: cpu`)
- [x] B10 — `ActionResult` and partial failure — spec §11.5, §16.5 —
      `crates/ono-command/tests/mutations.rs::should_keep_a_mixed_result_apart_rather_than_collapsing_it`,
      `::should_answer_with_one_outcome_per_object_that_arrived`,
      `crates/ono-provider-api/tests/contract.rs::should_report_what_it_did_to_each_target_rather_than_one_boolean`,
      case `037-files-read-write-remove.case`

### Phase C — Linux core providers (spec §23, §28, §35.3)

Every provider answers from the kernel, systemd or NSS — never by parsing unstable human text
(spec §50, AGENTS.md §6). Every provider ships its conformance case in the same increment.

- [x] C1 — `ono-provider-api`: the provider trait, capability declarations, and the
      `snapshot` / `subscribe` / `watch` triple with the `ObjectEvent` envelope of spec §31.14
**The seven case names these boxes carried — `040-process-provider` … `046-service-provider` —
never existed**: they are the pre-implementation planning names, and the 2026-08-29 reconciliation
replaced each with the case that actually covers the capability.

- [x] C2 — `process` from procfs — spec §23.1, §28.1 —
      acceptance `040-processes-inspect-jobs-signals.case`, `120-process-cpu-share.case`
- [x] C3 — `file`/`dir` — spec §23.4, §28.2 —
      acceptance `037-files-read-write-remove.case`, `121-copy-preserves-a-tree.case`
- [x] C4 — `user`/`group` from NSS, `env` — spec §23.6, §28.7 —
      acceptance `043-identity-sessions-and-accounts.case`, `021-cwd-and-environment.case`
- [x] C5 — `mount`/`filesystem` — spec §23.5, §28.6 —
      acceptance `042-storage-devices-and-mounts.case`, `122-mount-propagation-peers.case`
- [x] C6 — `interface`/`route`/`neighbor` over netlink — spec §23.2, §28.5 —
      acceptance `039-network-dns-port-mutations.case`, `123-privileged-network-writes.case`
      (nine write paths against a live kernel under `CAP_NET_ADMIN`)
- [x] C7 — `socket`/`connection` over netlink sock_diag — spec §23.2, §28.4 —
      acceptance `039-network-dns-port-mutations.case`, `123` (`stop socket` over
      `NETLINK_SOCK_DIAG`), `047-relationship-graph.case`
- [x] C8 — `service` over the systemd D-Bus API — spec §23.3, §28.3 —
      acceptance `038-services-set-and-journal.case`;
      `services_logs_missing.rs::should_trace_a_service_to_the_processes_it_owns`
- [x] C9 — Generated provider conformance suite from `docs/spec/providers/*.yaml` — spec §35.3 —
      done 2026-08-29, `33b6e10` + `a595c4f` + `81edc7c`, ADR-0331. `cargo xtask conformance`
      generates `crates/ono-cli/tests/provider_conformance.rs` from the declarations and
      `spec-check` fails on drift: **87 generated tests over 18 provider entries, 30 schemas and
      35 targets**, where 4 providers and 2 schemas were covered before. Generation refuses rather
      than emitting a hole, and found four contract violations that `81edc7c` fixed.

### Phase D — Language consistency and discoverability (spec §15, §27, §36, §47)

- [x] D0 — The registries themselves — ADR-0012 — commit 6b107d0
- [x] D1 — `xtask spec-check` validates the registries and cross-checks them against the
      implementation — spec §36.5
- [x] D2 — The command registry drives dispatch — spec §27.2 —
      `crates/ono-command/tests/registry.rs::should_load_every_command_the_contract_files_declare`,
      `::should_find_a_command_by_verb_and_target`,
      `implementations.rs::should_hand_the_bound_arguments_and_the_input_to_the_implementation`,
      `crates/ono-cli/tests/builtins.rs::should_dispatch_set_of_a_system_target_through_the_registry_rather_than_the_builtin`;
      id uniqueness enforced by `xtask/src/contracts.rs:494`. **Split closed** by `42713e4`
      (ADR-0247): `xtask::bindings::check_bindings` runs §27.2's binding check over the embedded
      registry and `builtin_commands` in the gate, both directions —
      `xtask/tests/bindings.rs`, three tests.
- [x] D3 — `help` generated from metadata for every command, target and topic — spec §15.2 —
      `crates/ono-command/tests/help.rs::should_generate_complete_help_for_every_command_in_the_registry`
      (iterates the registry and fails on any missing summary, example or field doc),
      `::should_generate_help_for_a_verb`, `::should_generate_help_for_a_target`,
      `::should_generate_help_for_a_topic`; case `043-discoverable-from-the-shell.case`
- [x] D4 — Completion from metadata — spec §15.1 —
      `crates/ono-command/tests/completion.rs` (six position tests),
      `completion_missing.rs` (schema-field positions), case `044-semantic-completion.case` on a
      real terminal; provider-backed live values since `4a91ec8` (ADR-0252) —
      `completion.rs::should_offer_this_machines_users_when_completing_a_user_selector`.
      **Split, halved:** the < 50 ms first-result budget is still an in-process proxy —
      B-split-D4.
- [x] D5 — `type` and `inspect`, showing schema, provenance and the causal chain — spec §15.2 —
      `crates/ono-command/tests/meta.rs::should_report_what_a_pipeline_would_produce_without_running_it`,
      `::should_show_every_field_with_how_it_was_known_and_where_it_came_from`,
      `::should_show_the_causal_chain_of_an_error`; case `043`
- [x] D6 — `explain` — spec §15.3 —
      `crates/ono-command/tests/explain.rs::should_render_a_plan_in_the_shape_of_spec_42_1` and six
      siblings, `crates/ono-cli/tests/builtins.rs::should_explain_without_running_anything`;
      case `043` (`explain-never-ran` with `stdout-not-contains: RAN`)
- [x] D7 — Fuzzy command discovery and the `resolve.command_not_found` suggestion path —
      spec §15.4 —
      `crates/ono-cli/tests/meta_config_missing.rs::should_report_command_not_found_with_suggestions_when_no_stage_answers`,
      `crates/ono-command/tests/meta.rs::should_find_a_command_by_what_it_does_rather_than_by_its_name`,
      `help.rs::should_suggest_a_near_miss_for_an_unknown_topic`, `suggest.rs`'s unit tests
- [x] D8 — Generated documentation under `docs/reference/` — spec §36.2, §46

### Phase E — Contextual systems interface (spec §14, §20)

- [x] E1 — Context stack, `enter`/`leave`, filesystem and object contexts — spec §14.1–§14.3 —
      `crates/ono-cli/tests/context.rs::should_enter_a_directory_and_leave_back_out_of_it`,
      `::should_show_the_stack_when_asked_for_the_context`,
      `::should_stay_on_the_ground_when_there_is_nothing_to_leave`,
      `::should_refuse_to_enter_an_object_that_does_not_exist`,
      `::should_leave_every_frame_at_once_when_asked`; case `045-context-and-reuse.case`
- [x] E2 — Implicit selectors from context — spec §14.3 —
      `crates/ono-command/tests/producers.rs::should_narrow_a_producer_with_the_ambient_selector_of_a_context_frame`,
      `::should_refuse_a_query_the_context_cannot_narrow_rather_than_widening`, and one per target
      in `processes_missing.rs`, `storage_missing.rs`, `network_missing.rs`, `identity_missing.rs`
- [x] E3 — Prompt as a HUD: link, privilege, context, path, jobs — spec §4.2 —
      case `029-prompt-shows-context.case`, `049-remote-link.case` (`testbox://`),
      `crates/ono-cli/tests/signals.rs::should_make_an_elevated_prompt_impossible_to_miss`,
      case `025-job-control.case` (`+1 >`). **Split closed** by `7cad7d6` (ADR-0250): the `vcs`
      segment exists and the object-context segment is asserted — case `113-prompt-segments.case`.
- [x] E4 — Interactive selection over rendered collections, never altering pipeline data —
      spec §13.5 —
      `crates/ono-cli/tests/view.rs::should_pick_a_row_and_leave_it_addressable_as_the_current_value`
      (a real PTY: arrow, Enter, `q`, then `@ | to json`), `::should_fall_back_to_plain_rendering_when_nobody_is_watching`;
      `ono.data.view` is registered as a pass-through stage (ADR-0050)
- [x] E5 — Semantic history and structured result retention; `@`, `@-1`, `@3` —
      spec §20.1, §20.2, §6.4 —
      `crates/ono-cli/tests/native.rs::should_reuse_the_previous_result_without_rerunning_it`,
      `::should_pick_one_item_of_the_current_result_by_position`,
      `::should_say_there_is_nothing_to_reuse_when_no_result_was_retained`,
      `crates/ono-history/tests/history.rs::should_record_where_and_how_a_command_ran_rather_than_only_its_text`;
      case `045`. **Split closed** by `e6094eb` (ADR-0249): the seventeenth result evicts the
      first, and a result over the value bound is retained truncated and says so.

### Phase F — Live system semantics (spec §18)

- [x] F1 — `watch` over a query, event/snapshot model, explicit polling metadata — §18.2 —
      `crates/ono-command/tests/watch.rs::should_begin_a_watch_with_the_current_state_and_then_the_changes`
      (snapshot first, `changed` naming its field, `source: "poll"`),
      `::should_emit_an_empty_snapshot_when_the_watched_listing_has_nothing_in_it`;
      case `046-live-system-semantics.case`
- [x] F2 — In-place rendering keyed by stable object identity — §18.3 —
      `crates/ono-cli/src/live.rs::tests::should_report_no_change_when_an_event_repeats_the_shown_state`,
      `crates/ono-cli/tests/watch_live.rs::should_render_in_place_at_a_terminal_and_stop_on_ctrl_c`
- [x] F3 — Native background jobs, `get job`, the prompt's job segment — §18.4 —
      `crates/ono-cli/tests/jobs_native.rs::should_background_a_watch_as_a_job_the_shell_lists`,
      `::should_share_one_number_space_between_native_and_external_jobs`,
      `::should_finish_a_bounded_background_pipeline_and_say_so`;
      `processes_missing.rs::should_list_a_detached_live_view_as_a_native_job`; case `046`
- [x] F4 — Cancellation through native pipelines and into external processes — §18.5 —
      `crates/ono-cli/tests/signals.rs::should_interrupt_a_native_pipeline_and_leave_the_prompt_standing`,
      `::should_report_a_command_the_terminal_interrupted_as_128_plus_sigint`,
      `watch_live.rs::should_reattach_a_backgrounded_watch_and_end_it_with_ctrl_c`;
      case `030-signals-and-process-groups.case`

### Phase G — Relationship graph (spec §22)

- [x] G1 — Graph value type with provenance and confidence — §22.1, §22.2 —
      `crates/ono-graph/tests/graph.rs::should_describe_itself_as_a_record_of_the_graph_contract`
      (validates `ono.graph/1`, asserts `confidence` and `provider` on every edge),
      `::should_travel_a_pipeline_and_survive_being_serialized_as_json`; case `047`
- [x] G2 — Exact relationship providers — §22.3 — `crates/ono-graph/tests/relationships.rs`:
      `::should_link_a_process_to_its_parent_and_to_its_children`,
      `::should_link_a_process_to_the_socket_it_holds_by_inode`,
      `::should_link_a_service_to_its_main_process_and_to_its_cgroup_members`,
      `::should_link_a_mount_to_the_device_backing_it`, plus the inference discipline in
      `::should_mark_a_reverse_resolved_host_as_inferred_and_keep_its_evidence`
- [x] G3 — `trace` for process, service and socket — §22.3 —
      `crates/ono-command/tests/trace.rs::should_answer_a_trace_with_a_graph_rooted_at_the_named_object`,
      `::should_report_a_trace_of_nothing_rather_than_an_empty_graph`, nine walk tests in
      `crates/ono-graph/tests/trace.rs`, case `047` for `process`,
      `options_and_selectors_missing.rs::should_trace_the_socket_on_the_requested_port_when_port_is_given`
      for `socket`. **Split closed** by `d1a6e5f`:
      `services_logs_missing.rs::should_trace_a_service_to_the_processes_it_owns` and
      `::should_refuse_to_trace_a_service_that_does_not_exist` drive the third stated target.
- [x] G4 — Tree and ASCII graph renderers; the graph view never fabricates edges — §22.4 —
      `crates/ono-graph/tests/render.rs::should_draw_the_tree_of_the_specification`,
      `::should_draw_an_inferred_edge_differently_from_an_observed_one`,
      `crates/ono-render/tests/tree_layout.rs`,
      `relationships.rs::should_not_invent_a_device_for_a_filesystem_that_has_none`,
      `::should_not_invent_a_gateway_neighbour_the_kernel_has_not_resolved`;
      `crates/ono-cli/tests/native.rs::should_draw_a_trace_as_a_tree_rather_than_a_table`

### Phase H — Remote links (spec §21)

- [x] H1 — `ono-protocol`: typed transport, framing, versioning, multiplexed streams — §21.2 —
      `crates/ono-protocol/tests/framing.rs::should_refuse_a_frame_claiming_more_than_the_limit_before_allocating`,
      `handshake.rs::should_prefer_the_highest_version_both_ends_speak`,
      `streams.rs::should_keep_concurrent_streams_apart_when_both_are_open`,
      `::should_leave_the_other_streams_running_when_one_is_cancelled`,
      `::should_bound_a_fast_remote_producer_when_the_local_consumer_is_slow`, `messages.rs`
- [x] H2 — the remote endpoint — §21.4 — there is no `ono-agent` crate; the endpoint is
      `crates/ono-remote/src/agent.rs` reached as `ono --agent` (ADR-0036 §1) —
      `crates/ono-remote/tests/agent.rs::should_negotiate_the_registry_providers_with_their_availability`,
      `tests/subprocess.rs::should_run_a_query_against_an_agent_in_a_child_process`; case `049`
- [ ] H3 — Agentless SSH fallback — §21.3 — **open: issue #2.** `--agentless` is parsed and
      visible; `context.rs:615` still says the fallback does not exist, and ADR-0037 §6 agrees.
- [x] H4 — Provider negotiation and capability discovery — §21.2 —
      `crates/ono-remote/tests/agent.rs::should_announce_capabilities_with_the_risk_the_provider_declares`,
      `crates/ono-protocol/tests/handshake.rs::should_negotiate_the_intersection_of_the_two_capability_sets`,
      `crates/ono-remote/tests/provider.rs::should_mount_one_provider_per_negotiated_target`;
      case `044-remote-links-as-objects.case`
- [x] H5 — Security model: host key pinning, `remote.host_key_changed` — §21.5, §49 —
      `crates/ono-protocol/tests/trust.rs` (five tests: pin on first contact, refuse a changed
      key, refuse an unknown key under a required policy, deliberate replacement, a readable
      store), `crates/ono-remote/tests/trust.rs::should_refuse_a_changed_host_key_with_the_stable_safety_code`
      (E0603, non-retryable). **Split:** the store is still not consulted on either production
      transport and no case asserts E0603 — B-remote-2, which absorbs F12. `593baee` (ADR-0274)
      records what must exist first: a transport that certifies its peer to this process.
- [x] H6 — Remote context and prompt — §14.4, §4.2 — case `049-remote-link.case` (`testbox://`
      in the prompt), `044` (`testbox (remote)`, `risk mutate + remote`),
      `crates/ono-cli/tests/remote_missing.rs::should_enter_the_remote_context_when_connecting_to_a_host`,
      `::should_pop_the_link_frame_when_detaching`

### Phase I — KUANG/11 extension runtime (spec §31)

- [x] I1 — `docs/spec/kuang/` contracts — §31.78 — all seven exist and are consumed:
      `crates/ono-kuang-protocol/tests/manifest_validation.rs` (14 tests),
      `src/error.rs::should_expose_all_27_codes_of_spec_31_79_when_enumerated`,
      `src/capability.rs::should_carry_all_29_families_of_the_registry_when_enumerated`
      (ADR-0022). **Split closed** by `122dcea` (ADR-0266): `check_kuang_contracts` holds four of
      the seven against `crates/ono-kuang-*` both ways, the manifest half by asking the parser
      rather than mirroring it — `xtask/tests/contracts.rs`, three tests.
- [x] I2 — `ono-kuang-protocol` — §31.12 — `src/{frame,message,version,error,lifecycle,capability}.rs`
      unit suites, and the wire proof
      `crates/ono-kuang-sdk/tests/conformance.rs::should_quarantine_a_plugin_that_breaks_framing`
- [x] I3 — Package identity, layout, manifest validation — §31.5–§31.7 —
      `manifest_validation.rs::should_refuse_a_third_party_claim_on_the_ono_namespace`,
      `::should_fail_closed_on_an_unknown_key_in_a_closed_section`,
      `::should_refuse_a_scope_key_the_capability_does_not_declare`;
      `crates/ono-cli/tests/plugins_missing.rs::should_install_a_package_from_a_path_reference_when_confirmed`;
      cases `045`, `050`. **Split:** signature verification (§31.9) still does not exist — C-5.
- [x] I4 — Supervisor: install/enable/load/run states and lifecycle — §31.8 —
      `crates/ono-kuang-protocol/src/lifecycle.rs::should_walk_the_main_path_of_spec_31_8_when_each_step_is_legal`
      and five siblings (ADR-0041); `plugins_missing.rs::should_persist_enablement_across_sessions`,
      `::should_withdraw_contributions_when_a_package_is_unloaded`;
      `conformance.rs::should_quarantine_a_plugin_that_emits_beyond_credit`; case `045`.
      **Split:** §31.10 isolation is still capability brokering only, with no sandbox — C-4(a).
- [x] I5 — Capability broker, scopes, policy, audit — §31.16–§31.19, §31.33 —
      `crates/ono-kuang-supervisor/src/policy.rs::should_deny_by_default_when_no_rule_matches`,
      `::should_let_a_system_deny_override_a_grant`,
      `::should_allow_a_path_inside_the_granted_scope_and_refuse_one_outside`; the four denial
      paths in `conformance.rs`; `plugins_missing.rs::should_record_a_denied_capability_use_in_the_audit_trail`;
      case `045`. **Split closed** by `a5be21b` (ADR-0264, ADR-0265): `--scope` is validated
      against the capability's declared keys and `--duration` makes a lease the broker checks;
      `always` grants reach `<config>/kuang/policy.yaml` and the trail
      `<state>/kuang/audit.jsonl`. `plugins_missing.rs`' eight broker tests, case
      `125-kuang-capability-policy`.
- [x] I6 — Host API domains — §31.12 — six of sixteen are implemented and proven:
      streams (emit/close), filesystem (read), state (get/set/delete), audit, clock (now),
      capabilities (check/request) — `conformance.rs::should_stream_typed_values_for_a_contributed_command`,
      `::should_refuse_a_state_write_beyond_quota_and_keep_state_intact`,
      `::should_audit_a_granted_call_with_the_virtual_clock`,
      `::should_refuse_and_audit_a_path_outside_the_granted_scope`.
      **Split:** the other ten domains are still absent — C-4(c).
- [x] I7 — Backpressure and quotas — §31.15 —
      `conformance.rs::should_deliver_everything_under_a_small_credit_window`,
      `::should_quarantine_a_plugin_that_emits_beyond_credit`,
      `::should_stop_cleanly_when_the_host_cancels_a_stream`,
      `crates/ono-kuang-supervisor/src/state.rs::should_refuse_a_write_beyond_quota_and_keep_existing_keys_intact`.
      **Split closed** by `1665e1e` (ADR-0267): `host_emit` consults the negotiated policy, and
      `fail-stream` raises K11206, which nothing raised before —
      `conformance.rs::should_end_the_stream_and_keep_the_instance_when_the_negotiated_overflow_fails_the_stream`.
- [x] I8 — Contribution model: commands, targets, schemas, relations, adapters —
      §31.22–§31.27 — `conformance.rs::should_surface_contract_shaped_contribution_tables`,
      `::should_close_the_stream_when_output_leaves_the_declared_schema`;
      `crates/ono-cli/tests/plugins.rs::should_load_a_package_and_run_its_contributed_command`,
      `::should_adapt_through_a_third_party_pack_once_its_grant_is_explicit`;
      relations end to end in `docker/acceptance/cases/110-spatial-contributions.case`
      (`s9-a` … `s9-g`) and
      `crates/ono-kuang-testhost/tests/spatial_package.rs::should_refuse_a_package_that_declares_relations_without_asking_for_the_capability`
      (ADR-0194). **Split closed** by `c111f91` (ADR-0268), by the second of the two outcomes the
      box allowed: an annotation key outside the package's namespace is `package.invalid` and a
      view contribution is `package.incompatible` naming the `view_protocol` dimension, so neither
      is listed and ignored. Registering a view stays a tranche inside C-4.
- [x] I9 — `ono-kuang-sdk` and the deterministic test host — §31.73 —
      `conformance.rs::should_audit_a_granted_call_with_the_virtual_clock` asserts the exact
      virtual timestamp; `crates/ono-kuang-testhost` is the real supervisor on a fixed clock
      (ADR-0040 §1); the example plugin ships into the container
- [x] I10 — Plugin conformance suite — §31.74 — `crates/ono-kuang-sdk/tests/conformance.rs`,
      seventeen tests over every area ADR-0040 enumerates; case `050-kuang-plugin.case`
- [ ] I11 — `ono-model-broker` — §31.12 — **open: issue #5.** The crate does not exist;
      `model_broker` is a manifest field nothing reads and `Capability::ModelInfer` is a
      capability nothing checks. The design is in the issue body; no file is written.

### Phase J — Advanced TUI views (spec §37 Phase J, §13.6, ADR-0050)

ADR-0050 collapses Phase J into one verb, `view`, on §37 J's own "deliver only where semantics
justify them", and records what it declines and why.

- [x] J1 — Navigable graph view — §22.5 — `view tree` renders graph values navigably
      (`crates/ono-cli/src/view.rs:25-34`); the tree rendering is proven by
      `crates/ono-graph/tests/render.rs` and `crates/ono-render/tests/tree_layout.rs`.
      **Split closed** by `365ac0a`:
      `view.rs::should_open_the_tree_view_over_a_graph_and_leave_the_pick_behind` drives a real
      PTY — open on `trace process 1`, a carriage return opens the inspect pane, `q` leaves the
      graph addressable as `@` — and case `111-view-tree-navigation` runs it in the container.
- [x] J2 — Multi-pane inspect — §37 —
      `crates/ono-cli/tests/view.rs::should_pick_a_row_and_leave_it_addressable_as_the_current_value`
      asserts the `--- inspect` pane opens beside the collection. The multi-pane *watch* half is
      declined by ADR-0050 ("arrangement, not semantics").
- [x] J4 — Object pickers — §13.5 — the same test: a real PTY picks a row and bare `@` then names
      it; `::should_fall_back_to_plain_rendering_when_nobody_is_watching` proves §17.4 off-terminal
- [x] J5 — Remote link overview — §37 — `get link | view table` (ADR-0050): `get link` proven by
      case `044-remote-links-as-objects.case` and `crates/ono-cli/tests/remote.rs`, `view table`
      by `view.rs`. **Split closed** by `365ac0a`: case `112-link-overview` makes two links
      against this binary over a pipe pair, so it needs no network, and browses them in the view.

J3 (timeline/history exploration, §20.3) is **not built, deliberately.** ADR-0050: §20.3 is a MAY,
Ctrl-R and `history` already carry the semantics, and a timeline adds presentation over the same
records. It was removed from this board rather than carried as an open box.

### Cross-cutting, tracked to the release checklist

- [x] Performance budgets of spec §34 measured in the container —
      `docker/acceptance/cases/060-performance-budgets.case` (cold start, bare start, parse,
      first process row) and `100-spatial-performance-budgets.case` (the eight v0.4 budgets, none
      violated); `crates/ono-editor/tests/latency.rs` for the keystroke budget.
      **Split:** four of spec §34's five pathological environments are still absent — C-7, which
      an agent holds (see *In progress*).
- [ ] Fuzzers over parser, serializers, remote protocol, plugin protocol, procfs/netlink
      decoders — spec §35.6 — **open: issue #1.** No `fuzz/` directory; since `80c8cb7` the
      unfinished-work scan is pinned to walk one the day it appears.
- [x] A test for each risk in the threat model of spec §49 — every T1–T15 row of ADR-0015 has a
      passing test, enumerated in `docs/ACCEPTANCE.md` §4.4's final bullet; ADR-0203 adds seven
      spatial rows the same way. **Split closed** by `9d7f16a`: **ADR-0245 supersedes ADR-0015**
      with the same fifteen threats and a *Proven by* column that names test functions, and
      `xtask/tests/spatial_evidence.rs::should_find_every_test_the_threat_model_names` turns the
      gate red when a named proof goes missing.
- [x] Theme and semantic visual tokens — spec §44 — the 24 tokens are delivered and fully tested
      (`crates/ono-render/tests/presentation.rs`), and a theme is loadable since 2026-08-29,
      `1c4866b`, ADR-0332: `theme.name` joins the ADR-0094 catalogue, resolution is built-in →
      `/etc/ono/themes/<name>.toml` → `<config dir>/themes/<name>.toml`, two themes ship (`ono`,
      `neon`), and an unknown token, key or value is refused rather than half-applied. 19 tests,
      acceptance case `150`.
- [x] The per-capability quality bar of spec §50 for every advertised command —
      `docs/ACCEPTANCE.md` §4.2, nine boxes, each proven by a registry-wide sweep rather than a
      sample: `ono-command/tests/help.rs` iterates every command, completion candidates are
      registry lookups, the provider conformance suites validate every emitted record, case
      `034-redirected-output-is-deterministic` requires terminal, file and pipe to be
      byte-identical, and case `033-errors-are-structured` requires an `Ono-Sendai-ENNNN` code on
      every failure. **Split:** the first-completion budget is still a proxy — B-split-D4. Since
      `e0c6eec` (ADR-0233) `spec-check` also holds every declared option against the sources, so a
      command can no longer advertise an option no code reads.

---

## Done

**A one-shot command no longer pays for a shell it does not use (2026-09-03, ADR-0571).**
`ono -c 'echo ready'` took 26,8 ms first / 30,1 ms p95 where `bash -c` takes 4 ms on the same
machine, and `ono --version` 0,9 ms — so the binary was not the cost, the first pipeline was.
Measured in-process: 15 ms parsing 465 KB of embedded YAML (command families, ninety schema
contracts, adapter packs), 7 ms connecting to systemd and logind over D-Bus, 1 ms for a tokio
thread pool, all of it for a pipeline with nothing native in it. Three increments, one kind
each: (1) `perf(contracts)` — the three crates that embed `docs/spec/` documents transcode them to
JSON in a `build.rs` and read that, with a fidelity test per crate against the YAML on disk;
26,8 → 13,8 ms. (2) `perf(cli)` — the §11.3 pre-flight check resolves the stage heads first and
plans against the providers only when one is native; 13,8 → 4,8 ms, and an external-only
pipeline now starts no runtime (`tests/one_shot_startup.rs`). (3) `perf(providers)` — the two
D-Bus connections are opened side by side with `tokio::join!`, which is what a native pipeline
still pays. Found on the way and recorded above: `limit.v1.yaml` is not embedded, and two
timing-sensitive tests fail under a concurrent release build. The `-4`/`-6` flags of two adapter
packs are quoted now, in a `spec` commit ahead of the rest, because JSON does not read an
integer into a string field the way YAML did.

**A skip is visible or it is not a skip (2026-09-01, ADR-0428).** Eight hand-written
`eprintln!("skipped: …")` lines in six suites, in eight formats, each followed by an early return.
`cargo test` has no outcome for "could not run here", so all of them counted as `ok` — around
thirty tests on a host that cannot meet their preconditions (no second mount, running as root, no
`git` on `PATH`). They now go through `ono_testkit::skipped(reason)`, which prints one marker
naming the test and the reason, so `cargo test 2>&1 | grep -c SKIPPED` answers how much of a run
was real. `xtask spec-check` refuses any other spelling
(`scan::check_silent_skips`), and checks the whole repository for it, so a new silent skip fails
the gate where it lands. The rule catches the *announcement*, never the early return itself: a
precondition guard and an ordinary `return` are the same Rust, and a check that guessed would
either cry wolf or teach people to phrase guards so it looks away.

**One helper per behaviour, not per file (2026-09-01, ADR-0427).** `fn ono` was declared by hand
in 24 suites — in eleven different implementations; `rows` in fifteen suites and thirteen
implementations. Same name, different behaviour, which is how two tests of one contract come to
disagree about it. Every byte-for-byte identical helper moved to the nearest shared home
(`ono_testkit::{ono, ono_within}`, and a `tests/support/mod.rs` for `ono-cli` and `ono-command`
beside the ten crates that already had one); every genuine variant stayed where it was, because
unifying it would change which implementation a test runs (AGENTS.md §11). Net −97 lines, and the
identical-helper duplication drops from 394 lines to 152.


**The RED suites are named for their subject (2026-09-01, ADR-0426).** Twenty-three suites still
carried the `_missing` names and the RED-phase prose they were written with on 2026-08-27 —
21 231 lines and 597 tests, 69 % of the `ono-cli` test code, asserting in the present tense that
the shell cannot do what it does. The debt was recorded under *Deferred* on 2026-08-29 and is now
paid. Twenty suites drop the suffix; three that would have collided with an existing plain name
are named for what distinguishes them (`plugin_commands.rs`, `remote_commands.rs`,
`completion_fields.rs`) rather than merged into it, because merging two same-named local helpers
would change which implementation a test runs (AGENTS.md §11). Module documentation moved to the
present tense. The evidence tables of `docs/ACCEPTANCE.md` §4.7, ADR-0203 and ADR-0245 are live
indexes the gate resolves, so their pointer cells were rewritten with the rename, exactly as
`xtask/tests/spatial_evidence.rs` instructs; the other 121 ADRs and the session records above keep
the names they used, because they record what was true when they were written and the test
function names they cite are unchanged. No test body, assertion or helper changed —
`cargo test` runs the same 2 729 tests it ran before.

**The rpm database answers too (2026-08-31, agent `rpm`, ADR-0422).** `get/find/add/remove/set
package` worked on Debian and refused honestly everywhere else, which is half the Linux machines
in the world answering E0401. `linux.packages.rpm` is now registered beside `linux.packages` and
claims the same target, so the registry's existing rule decides which answers: dpkg where
`dpkg-query` is on `PATH`, rpm where `rpm` is, and where neither is present the two refusals name
both databases. It is one provider per *database* rather than per distribution, because
`ono.package/1` is identified by `provider + name` and Fedora, RHEL, openSUSE and SLES keep one
database between them — every record it makes says `provider: rpm`. The front end is whichever of
`zypper`, `dnf` and `yum` is on `PATH`, in that order: a machine carrying zypper is a SUSE machine
whatever else it has, while dnf installs anywhere. Machine formats only (AGENTS.md §6):
`rpm -qa --queryformat`, `dnf repoquery --queryformat`, and `zypper --xmlout search`, whose
`solvable` elements are read with the workspace's first XML reader (`quick-xml`) because
`--xmlout` is the machine interface zypper documents. `rpm` alone is enough to be available — the
listing is complete without a front end — and `find`/`add`/`remove`/`set` then refuse with E0402
naming the three programs looked for. `--purge` is E0402 on rpm rather than a quiet ordinary
removal, because rpm has no purge. Fifteen outcome tests
(`crates/ono-cli/tests/packages_rpm.rs`), seven decoder unit tests, the generated conformance
cases and acceptance case `046-rpm-packages` (three faked machines, and dpkg still answering on
the Debian container) prove it. One defect fell out of having two providers for one target and is
fixed in the same increment: `explain` named the first provider *claiming* a target, which on a
Red Hat machine is a plan for a different machine; it now names the first *available* one.

**The orphaned-shell leak is fixed (2026-08-28, agent `leak`, ADR-0160).** The 160 shells were
not spinning and not deadlocked on a lock: every one of them held the *master* side of its own
controlling terminal. `nix::pty::openpty` is glibc's `openpty(3)`, which opens `/dev/ptmx` and
the slave without `O_CLOEXEC`, and `PtySession::start` passed them straight to `spawn`, so every
program the shell started under a terminal inherited that terminal's master —
`/proc/<pid>/fdinfo/4` said `tty-index: 29` while `/proc/<pid>/fd/0` was `/dev/pts/29`. The last
reference to the master was therefore held by the shell reading from it, closing it in the caller
could never produce end of file, and the shell waited in `ep_poll` for a byte nobody could send.
Marking both descriptors `FD_CLOEXEC` in `PtySession::start` is the whole fix; the child still
gets the terminal as the three `dup2` duplicates `plan::prepare_pty` makes. `pgrep -c -x ono`
after a full `scripts/gate.sh` run is now 0, where it used to grow by a shell per PTY test.
Proven by `crates/ono-cli/tests/session_lifetime.rs`
(`should_exit_when_the_terminal_it_was_given_goes_away`,
`should_not_hold_the_terminal_that_drives_it`), both RED before the fix.



- [x] `fix(remote)` a shell ends the agent processes it started (ADR-0161): `link host` spawns
  `ono --agent` (or `ssh … ono --agent`) as its own child, and nothing waited for it — the shell
  exited first and the agent reparented to whatever init the machine runs. Measured from a
  process with `PR_SET_CHILD_SUBREAPER`: the shell was reaped, and a second, still-running
  process reparented onto the subreaper in the same millisecond, every run. In the container
  that init is `script` (`bash -lc 'script …'` execs it), whose `SIGCHLD` reaping took the
  orphan for its own child and hung up the `bash` under it — case 049's exit 129. Now
  `Session::hang_up` says the goodbye explicitly (`Link::hangup`) and waits for the process
  through a `ChildProcess` handle that outlives the transport, escalating `SIGTERM`/`SIGKILL`
  only after a 2 s grace it never reaches; `impl Drop for Session` does it for every link still
  held, before the runtime field is dropped. Every teardown path — `remove link`, `detach link`
  of a one-shot, `leave` of a one-shot frame, `add link` replacing a name, a handshake that
  failed after the child was spawned — goes through it. RED first in
  `crates/ono-cli/tests/session_lifetime.rs::should_end_the_agent_it_started_before_it_exits`.
  Proof: 20 consecutive `scripts/acceptance.sh --keep-image remote-link` runs green while a full
  69-case suite ran beside them; the subreaper probe sees no orphan; a linked `ono -c` costs the
  same as before (10 runs, 1.64 s, unchanged)

- [x] `fix(process)` an interactive `ono` no longer outlives the terminal it was given
  (ADR-0160): `PtySession::start` marks the `openpty` master and slave `FD_CLOEXEC`, so a program
  the shell starts under a terminal no longer inherits that terminal's master and end of file on
  the shell's input becomes possible at all. RED first in
  `crates/ono-cli/tests/session_lifetime.rs`; `pgrep -c -x ono` after a full gate run went from
  "one per PTY test, forever" to 0. Case 049's exit 129 was a second, separate leak of the same
  kind — the link's agent, not the shell — and is fixed by ADR-0161 below

- [x] installable `.deb`/`.rpm` for x86_64 and aarch64 (docs/ACCEPTANCE.md §4.5, ADR-0121,
  ADR-0122, ADR-0123): package metadata and maintainer scripts in `crates/ono-cli/Cargo.toml`
  + `crates/ono-cli/packaging/deb/`, shape pinned by `xtask/tests/packaging.rs` — commit
  cbc7612; `scripts/package.sh` (container builds via `cross`, `dist/ono_<v>_<arch>.deb`,
  `dist/ono-<v>-1.<arch>.rpm`, reproducible bytes) — commit 8608c1c;
  `scripts/package-check.sh` (install/run/login-shell/remove in fresh `debian:bookworm` and
  `fedora:latest`, structural check for a foreign arch) — commit a16633e; release workflow on
  `v*` tags with native x86_64 and aarch64 runners plus a `packaging` job in `ci.yml` — commit
  e6f10f1; the §4.5 box, `scripts/release-check.sh` running both scripts, README install
  section. Local aarch64 packages are structural proof only; their runtime proof is the release
  workflow on `ubuntu-24.04-arm`.
- [x] wiki-verification defect (1): piped forms of shell-answered commands answered by their
  seams, `input: null` refused with the head form named (ADR-0118) — commit 1e98be0
- [x] wiki-verification defect (2): `get env` reads the session's live environment — commit 8ca9aa7
- [x] wiki-verification defect (3): a watch over an empty listing begins with its snapshot — commit ed75190
- [x] wiki-verification defect (4): `let` in a block rebinds the enclosing binding (ADR-0119) — commit cc339ee
- [x] CI-red symlink walk: `--follow-symlinks` lists a directory under every name that reaches it;
  a cycle is an ancestor on the walk path (ADR-0120) — commit 25c1985

### The RED-suite run (2026-08-27): per-family notes, kept for their open items

- [identity | 2026-08-27] **identity family remainder** — **all 25 tests of
  `crates/ono-cli/tests/identity_missing.rs` green and un-ignored** on branch
  `implementation-identity` (ADR-0100–0102). Acceptance case
  `docker/acceptance/cases/043-identity-sessions-and-accounts.case` is written and dry-run
  against the binary; **the integrator runs it in the container** when merging. Left open (not
  in the RED suite): a privileged conformance run of the account tools (the workspace's tests
  never change the developer's accounts); `select error.code` on an ActionResult row projects
  the whole error under `code` instead of its field.
- [remote | 2026-08-27] **remote family** (`crates/ono-cli/tests/remote_missing.rs`) on branch
  `implementation-remote` — **all 36 tests green and un-ignored**; the gate is green at every
  commit. Delivered: `link`/`host` as tables of the session provider, `ono.host/1` and its
  three sources (ADR-0103); `add/set/rename/remove/detach link`, `connect host`, `test host`
  and `add/set/remove host` (ADR-0104); `watch`/`trace` for link and host with
  `ono.link-event/1`, `ono.host-event/1`, `ono.provider/1` (ADR-0105); `--agentless` recorded
  and visible, `explain`'s EXECUTION CONTEXT and MUTATION blocks (ADR-0106). Acceptance case
  `docker/acceptance/cases/044-remote-links-as-objects.case` is written and dry-run against the
  binary; **the integrator runs it in the container** when merging the branch. Case 049 now
  matches the typed `get link` table by regular expression. Left open (not in the RED suite):
  the piped forms `get link | remove link` / `… | detach link`; the agentless provider set of
  ADR-0037 §6 (today `mode: agentless` is recorded and the agent answers, visibly); a
  `watch host` that probes reachability; the multiplexed streams of `trace link`; the
  execution context in the `ono.execution-plan/1` value.
- [containers | 2026-08-27] **container and package families** (`crates/ono-cli/tests/containers_packages_missing.rs`)
  on branch `implementation-containers` — files: `crates/ono-provider-container/`,
  `crates/ono-provider-linux/src/packages.rs`, `crates/ono-graph/src/kernel/container.rs`,
  `crates/ono-command/src/impls/{mod,meta}.rs`, `crates/ono-cli/src/providers.rs`,
  `docs/spec/commands/{container,package}.yaml`, `docs/spec/schemas/{container,image,package,container-event}.v1.yaml`,
  `docs/spec/providers/{container-engine,linux-packages}.yaml`, acceptance case
  `046-containers-and-packages`. ADR-0112–0115.
  Increment 1 done: `ono-provider-container` — the engine API over the runtime's Unix socket,
  `get container`/`get image`, E0401 naming the sockets tried (4 tests green, ADR-0112).
  Increment 2 done: start/stop/restart/remove/set container as engine requests, the engine's
  status as the per-target outcome (8 tests green, ADR-0113). Increment 3 done: `enter
  container` as a `container` frame, `watch container` over the engine listing, `trace
  container` with the exact `image` edge (4 tests green, ADR-0114). Increment 4 done:
  `linux.packages` — `get package`/`find package` from `dpkg-query -W -f` and `apt-cache
  search`, E0401 naming dpkg and rpm, E0403 for a listing outside the machine format (5 tests
  green, ADR-0115). Increment 5 done: `add`/`remove`/`set package` through `apt-get` and
  `apt-mark`, the unprivileged refusal as a failed E0302 row before anything runs (4 tests
  green, ADR-0115 §5). **All 25 tests green and un-ignored**; the gate is green at every
  commit. Acceptance case `docker/acceptance/cases/046-containers-and-packages.case` is
  written and dry-run against the binary; **the integrator runs it in the container** when
  merging the branch. Left open (not in the RED suite): an rpm/dnf package provider (the
  refusal names it); `trace container` edges to namespaces, cgroups, mounts and processes
  (need `State.Pid` joined to procfs); `watch container` over the engine's `/events` instead
  of polling; `enter container` as an execution context (`container.exec`); a root acceptance
  case for the package mutations' success path.

- [plugins | 2026-08-27] **plugins family** (`crates/ono-cli/tests/plugins_missing.rs`) on
  branch `implementation-plugins` — **all 32 tests green and un-ignored**; the gate is green at
  every commit. Delivered: `ono.plugin/1` records from the session provider `ono.shell`
  (ADR-0107); `verify`/`inspect`/`find plugin` and the K11 family folded into
  `ono_core::ErrorCode` (ADR-0108); `install`/`remove plugin` (ADR-0109); `unload`/`set plugin`,
  enablement on disk, hot reload (ADR-0110); `get/grant/revoke capability`, `get audit`, and the
  typed empty `assistant`/`model`/`finding` tables (ADR-0111). Acceptance case
  `docker/acceptance/cases/045-plugins-lifecycle.case` is written and dry-run against the
  binary; **the integrator runs it in the container** when merging the branch. Left open (not in
  the RED suite): `always` grants and leases on disk (spec §31.19), `--scope`/`--duration` on
  `grant capability`, `capability_grants` inside `inspect plugin`, instance memory/cpu figures
  (null today), the interactive install prompt under a PTY case.
- [meta | 2026-08-27] **meta family** (`crates/ono-cli/tests/meta_config_missing.rs`, plus the
  `--human` and uid/gid cases of `options_and_selectors_missing.rs`) on branch
  `implementation-meta` — files: `crates/ono-cli/src/{meta,resolve,settings,config,eval,native}.rs`,
  `crates/ono-command/src/impls/{meta,convert}.rs`, `docs/spec/schemas/command.v1.yaml`,
  `docs/spec/commands/identity.yaml`, acceptance case `041-config-and-resolve`. ADR-0093–0095.
  Increment 1 done: `resolve command` (6 tests green, ADR-0093). Increment 2 done: the typed
  settings catalogue, `get config` with layers/source/line/`--overridden`/`--problems`, typed
  `set config` with E0202/E0201 and its ActionResult (15 tests green, ADR-0094). Increment 3
  done: `render.table.max_rows` reaches the sink, redirected output and `format table`
  (3 tests green; the file has no `#[ignore` left). Increments 4–5 done: `--human` reaches
  record fields (2 tests), `uid`/`gid` declared before `name` so numeric selectors bind
  (2 tests, ADR-0095). Acceptance case `041-config-and-resolve` written and dry-run against the
  binary; **the integrator runs it in the container** when merging.

- [language | 2026-08-27] **language family** (`crates/ono-cli/tests/language_missing.rs`) on
  branch `implementation-language` — **all 31 tests green and un-ignored**; the gate is green
  at every commit. Delivered: `let`/`( … )`/`$( … )` capture (ADR-0069); callable functions and
  `alias` (ADR-0070); `now()`, the RFC 3339 timestamp literal, prefix assignment
  `NAME=value cmd`, `each { … }` blocks, string `+`, keyless `sort`, `kill %N` (ADR-0071).
  Acceptance case `docker/acceptance/cases/035-scripting-language.case` is written and
  dry-run against the binary; **the integrator runs it in the container** when merging the
  branch. Left open (not in the RED suite): `explain` of a `NAME=value cmd` stage, functions
  and aliases in completion candidates, a function in a non-head pipeline position.
- [watch | 2026-08-27] **`watch`/`trace` for the declared-but-unbound targets** — done for
  file, user, group, interface, route and mount (ADR-0078..0080; commits on
  `implementation-watch`). Left ignored: the remote five in `remote_missing.rs` (`watch
  link|host`, `trace link|host`) — they need `link`/`host` as provider-backed records first
  (context.rs `get_link` renders by hand); the remote family picks them up.

- [processes | 2026-08-27] **process family remainder** (`crates/ono-cli/tests/processes_missing.rs`;
  `--tree`/`--user` in `options_and_selectors_missing.rs`) on branch `implementation-processes`
  — **all 18 process tests and the 3 option tests green and un-ignored**; the gate is green at
  every commit. Delivered: `get job` from the session provider `ono.shell` (ADR-0090);
  `inspect process` → `ono.process-detail/1` and `get process --tree` (ADR-0091); `set process
  --priority` via setpriority(2) and `send signal` as the pipeline spelling of a signal
  (ADR-0092). Acceptance case `docker/acceptance/cases/040-processes-inspect-jobs-signals.case`
  is written and dry-run against the binary; **the integrator runs it in the container** when
  merging the branch. Left open (not in the RED suite): a tree renderer for `--tree` at the
  terminal (the table shows the roots' columns; spec §22.4's tree view is the graph family's);
  `link`/`host` rows in `SessionTables` (remote family, ADR-0090 §3).

- [agent | 2026-08-27] **RED suites for everything v0.2 declares but does not build** (user
  request; wiki pages "Command Index" and "What Is Not Built Yet"). 329 outcome tests, every
  one `#[ignore = "REASON: …"]` (AGENTS.md §7) so the tree stays green; **the increment that
  delivers a family removes the ignore lines of its tests in the same commit** — a family is
  done when its file has no `#[ignore` left and the gate is green. Work order: cross-cutting
  seams first (registry-dispatched `set`/`remove`, ActionResult exit status and error shape,
  generic `enter`/`watch`/`trace` for object targets), then the families. Each file is one
  family; each test asserts the behaviour the contract promises, never mere presence:
  - `crates/ono-cli/tests/files_missing.rs` (34) — read/write/copy/move/remove/set/open/tail/
    watch/trace/enter file, remove/set dir, globs for native selectors — **done** by
    [files | 2026-08-27] on branch `implementation-files` (ADR-0081–0083) for everything
    except the four watch/trace tests, which stay `#[ignore` for the watch/trace family;
    the four `find file` option tests of `options_and_selectors_missing.rs` are green too.
    Acceptance: `docker/acceptance/cases/037-files-read-write-remove.case` (written, not yet
    run in the container by this agent)
  - `crates/ono-cli/tests/language_missing.rs` (31) — `let` capturing a pipeline, `$(…)`/`(…)`
    values, callable `fn`, `alias`, `now()`, timestamp literals, `FOO=bar cmd`, `each { … }`,
    string `+`, keyless `sort`, `kill %N`
  - `crates/ono-cli/tests/options_and_selectors_missing.rs` (15) — `--user/--tree`, `find file`
    options (**done**, files family), `--mounted`, `trace socket --port`, `--human`,
    `get user 0`, `where local.port`
  - `crates/ono-cli/tests/meta_config_missing.rs` (24) — `resolve command`, `get config` layers/
    source/line, `set config` typed + effective (`render.table.max_rows`)
  - `crates/ono-cli/tests/processes_missing.rs` (18) — `inspect process`, `get job`, `enter
    process`, `set process --priority`, `send signal`, failed ActionResult ⇒ exit 1 (ADR-0006)
  - `crates/ono-cli/tests/identity_missing.rs` (25) — `get session`, user/group mutations,
    watch/trace/enter user|group
  - `crates/ono-cli/tests/network_missing.rs` (31) — `resolve dns`, `test port`, watch/trace/
    enter interface|route|socket, route/interface/socket mutations
  - `crates/ono-cli/tests/services_logs_missing.rs` (15) — `set service`, `get journal`,
    `tail journal`, `get log` — **done, 15/15** by [services | 2026-08-27] on branch
    `implementation-services` (ADR-0084–0086, ADR-0096, case 038)
  - `crates/ono-cli/tests/storage_missing.rs` (22) — `get device`, mount/unmount, mount verbs,
    watch/trace/enter mount — **done** by [storage | 2026-08-27] on branch
    `implementation-storage` (ADR-0097–0099, case 042-storage-devices-and-mounts); nothing left
    ignored. `should_return_only_unmounted_filesystems_when_mounted_is_false` in
    `options_and_selectors_missing.rs` is green on the same branch.
  - `crates/ono-cli/tests/data_missing.rs` (15) + `crates/ono-command/tests/completion_missing.rs`
    (6) — `tail`, `join`, `diff`, stacked records on narrow terminals, fields after `where`
    — **done** by [data | 2026-08-27] on branch `implementation-data` (ADR-0072–0074); no
    `#[ignore` left in either file
  - `crates/ono-cli/tests/remote_missing.rs` (36) — `get link` as data, host commands, link
    definitions, detach/rename, agentless visibility, mutations across a link — **done** by
    [remote | 2026-08-27] on branch `implementation-remote` (ADR-0103–0106, case 044); no
    `#[ignore` left
  - `crates/ono-cli/tests/plugins_missing.rs` (32) — `ono.plugin/1` records, inspect/find/
    verify/install/unload/set/remove plugin, capabilities, audit, reload, assistants/models
  - `crates/ono-cli/tests/containers_packages_missing.rs` (25) — a fake engine-API socket and
    fake package managers on PATH; E0401 when none answers

  Wiki claims found stale while writing them (already work, no test added): `get route
  --table/--family`, `format --max-rows`, backgrounding native stages, `let i = $i + 1`.

  Contract gaps the suites had to resolve by reading — each needs an ADR (or a registry change)
  before its GREEN increment: `alias` statement syntax (grammar.ebnf/language.yaml have none);
  `ono.command/1` resolution `kind` field; `set config` unknown key ⇒ E0202; `ono.device/1`
  shape (path/kind/major/minor); `ono.session/1` fields; `ono.link/1` lacks a `host` field;
  `ono.container/1`, `ono.image/1`, `ono.package/1` schemas and the runtime knobs
  (`DOCKER_HOST`/`CONTAINER_HOST`, managers found on PATH); `get journal`/`get log` referenced
  `ono.log-record/1` which neither existed nor was deferred (resolved: ADR-0085/0086);
  `join`/`diff` output shape and
  `--identity [pid]` spelling; failed ActionResult rows nest the error as
  `error.error.code = "io.permission_denied"` instead of `error.code = "Ono-Sendai-E…"`, and
  `operation` carries the bare verb instead of the command id; K11 codes not folded into
  `Ono-Sendai-K11xxx`; `--agentless` is accepted and ignored by `context.rs::link`.

### Everything else

- [x] remote family — `get link`/`get host` from the session provider (ADR-0103) — commit
  19dce98; link definitions add/set/rename/remove/detach (ADR-0104) — commit fb10641;
  `connect host`, `test host`, ssh `-F` — commit 89879f3; watch/trace link and host
  (ADR-0105) — commit 5ced427; `--agentless` visible, `explain` context and mutation blocks
  (ADR-0106) — commit 91539b5; `add/set/remove host` and acceptance case 044 — see the
  `implementation-remote` log; `remote_missing.rs` (36) un-ignored
- [x] plugins family — `get plugin` records (ADR-0107) — commit f7a487a; verify/inspect/find and
  the K11 fold (ADR-0108) — commit 2cababb; install/remove (ADR-0109) — commit ca68ab0;
  unload/set/enablement (ADR-0110) — commit 7757006; hot reload — commit 4835eae;
  capabilities and audit (ADR-0111) — commit de2f831; assistants/models/findings and case 045
  — this commit; `plugins_missing.rs` (32) un-ignored
- [x] process family — `get job` (session provider, ADR-0090) — commit 0cc0730; `inspect process`
  (`ono.process-detail/1`, ADR-0091) — commit d512f03; `get process --tree` — commit b3f91a4;
  `set process --priority` (ADR-0092) — commit 730b1a3; `send signal` — commit d9cd7f8;
  `processes_missing.rs` (18) and the three `--user`/`--tree` tests un-ignored
- [x] File family — globs for native selectors, `read`/`write`/`copy`/`move`/`remove`/`set`/
  `open`/`tail file`, `remove`/`set dir`, `find file --name/--depth/--kind/--follow-symlinks`
  (ADR-0081, ADR-0082, ADR-0083) — branch `implementation-files`, commits b27b0a5, 7c41a09,
  a9b1f2f, c7e0e15, c26466f and the find-options commit after them

- [x] network family — `resolve dns` (system resolver, `ono-provider-net`, ADR-0087), `test port`
  (probe result, ADR-0087 §3), the nine route/interface/socket mutations over rtnetlink and
  sock_diag with the unresolved-target and `confirmation: always` seams (ADR-0088), null
  through a schema-known field and port/int comparability (ADR-0089), `--remote` on
  `trace connection`; a serializer no longer writes `[]` for a stream that only failed
  (ADR-0028) — commits 24f7968, baf53e2, e1d5a73, 3c54b30 and the two fixes after it;
  `network_missing.rs` (17 tests), `options_and_selectors_missing.rs` (3 tests),
  `docker/acceptance/cases/039-network-dns-port-mutations.case`. The eight watch/trace tests of
  `network_missing.rs` belong to another agent.
- [x] storage 1 — `get device` from /dev + sysfs, `ono.device/1` written — commit 0f9a36a
  (ADR-0097; `storage_missing.rs` ×4)
- [x] storage 2 — `get filesystem --mounted`, unmounted filesystems from udev's probe — commit
  2e588f4 (ADR-0097 §3; `options_and_selectors_missing.rs` ×1, provider fixture ×3)
- [x] storage 3 — `mount`/`unmount filesystem` through mount(2)/umount2(2); creating verbs name
  their object — commits e818770 (test form), 5a90ea8 (ADR-0098; `storage_missing.rs` ×5)
- [x] storage 4 — `set`/`add`/`remove`/`start`/`stop mount`: remount, fstab definitions, systemd
  mount units — commit e2e2f03 (ADR-0099; `storage_missing.rs` ×5, provider fixture ×5)
- [x] identity 1 — `get session` from systemd-logind over D-Bus, `ono.session/1` written,
  `--user` filter, E0401 where no login manager answers — ADR-0100;
  `identity_missing.rs` (2 tests) un-ignored, `crates/ono-provider-systemd/tests/session.rs` (4)
- [x] identity 2 — `add`/`remove`/`set user` through shadow-utils by exit status, E0302 from the
  euid check before any tool runs; `add` acts unresolved and an ambiguous name is narrowed by
  the input type — ADR-0101, ADR-0102; `identity_missing.rs` (6 tests) un-ignored
- [x] identity 3 — `add`/`remove`/`set group` and `--member` membership through
  `groupadd`/`groupdel`/`groupmod`/`gpasswd`, same privilege gate — ADR-0101;
  `identity_missing.rs` (5 tests) un-ignored; the file has no `#[ignore` left; acceptance case
  `043-identity-sessions-and-accounts` written
- [x] seams 1 — `set`/`remove` of system targets dispatch through the registry — commit 7ec0d83
  (ADR-0068 §1; `crates/ono-cli/tests/builtins.rs`)
- [x] seams 2 — ActionResult contract: a failed row exits 1, a missing target is an E0301 row,
  `operation` is the command id, `error` is a flat `ono.error/1` — ADR-0068 §2;
  `processes_missing.rs` (3 tests), `remote_missing.rs` (2 tests) un-ignored
- [x] seams 3 — a mutating verb binds when a provider advertises its capability
  (`builtin_commands_for`, ADR-0068 §3); `crates/ono-command/tests/mutations.rs` (4 tests).
  Families deliver a mutation by advertising the capability and answering the verb in `act`:
  `set service` now reaches the systemd provider, which reports it has no `set` operation
  (E0402 row) until the services family maps `--enabled` onto enable/disable and reports a
  missing property as E0201 naming `--enabled`
  (`services_logs_missing.rs::should_refuse_set_service_without_a_property…` stays ignored).

- [x] services 1 — `set service <unit> --enabled true|false` reaches the systemd provider as the
  `set` operation with the property as an argument (EnableUnitFiles/DisableUnitFiles); a `set`
  with no property is E0201 naming `--enabled` before anything is resolved (ADR-0084);
  `services_logs_missing.rs` (4 `set service` tests un-ignored),
  `ono-provider-systemd/tests/service.rs` (2 tests)
- [x] services 2 — `get journal [--since --boot]` and `tail journal [--lines]` as
  `ono.journal-event/1` through `journalctl --output=json` and the systemd adapter pack's
  decoder (ADR-0085); a provider-kind stream failure exits 1; `StreamSink::closed()` lets a
  following producer stop when `take` is satisfied; the decoder reads journalctl's byte-array
  and multi-valued strings (fix); `services_logs_missing.rs` (6 journal tests un-ignored)
- [x] services 4 — expression-valued options reach the provider query (`--since (now() - 1h)`
  evaluated in the producer, fix); a bare word compared with an enum field is that field's
  value — `where state == failed`, `where level >= error` run (ADR-0096); `services_logs_missing.rs`
  15/15, `ono-command/tests/expressions.rs`, `ono-cli/tests/native.rs`
- [x] services 3 — `get log [--service <ref>] [--level <name>] [--since --until]` as
  `ono.log-record/1` (journal-event plus `level`, the severity name) from the same journal
  provider (ADR-0086); case 038
- [x] data family (ADR-0072) — `tail N [--follow]` (commit 0f68fe0), `join <right> --on key
  --kind inner|left|right|outer` with `$variables` and pre-run `(pipelines)` visible to native
  stages (1616fe1), `diff <right> [--identity [fields]]` by schema identity (1761cc9),
  stacked records once a cut column would drop below eight cells (ADR-0073, 98437d4),
  schema fields with their docs after `where`/`select` (ADR-0074) —
  `crates/ono-cli/tests/data_missing.rs` (15/15), `crates/ono-command/tests/
  completion_missing.rs` (6/6), case 036
- [x] the context stack for every object target — `enter` of process/user/group/interface/
  socket/mount/file by word or by pipe (`get socket 443 | enter socket`), frames narrowing
  every later command at the command-table seam (`pid 1`, `--user root`, `--interface lo`,
  `--port 443`), `--user`/`--group` honoured by the procfs provider, `--interface` declared on
  `get route`, `--port` honoured by `trace socket` — ADR-0075, ADR-0076 — 24 tests un-ignored
  in `crates/ono-cli/tests/{processes,identity,network,storage,files}_missing.rs`

- [x] v0.3 step 1 — ADAPT-001 OutputDemand computed backwards from the consumer, reported
  by `explain` (ADR-0052) — cases 070, 071
- [x] v0.3 step 2 — the `adapter.*` error family E0901–E0911 in `docs/spec/errors.yaml` and
  `ono_core::ErrorCode` (ADR-0053) — `error_taxonomy.rs`; the box in ACCEPTANCE §4.6.2 stays
  open until an adapter emits one with the §1.65 payload
- [x] v0.3 step 3 — ADAPT-003 the `raw` keyword; `adapt` spelled for §1.18 (ADR-0054) —
  case 072
- [x] v0.3 step 4 — ADAPT-009 the declarative adapter contract, the util-linux pack with
  fixtures, `ono.block-device/1` and `ono.namespace/1`, the validator and the spec-check rule
  (ADR-0055) — `ono-adapter/tests/contracts.rs`, `xtask/tests/contracts.rs`
- [x] v0.3 step 5 — ADAPT-002 registry, negotiation states, identity pinning, conflict
  resolution, the probe cache of ADAPT-006, and `explain`'s `adaptation`/`argv`/`candidates`
  rows (ADR-0056) — `ono-adapter/tests/negotiation.rs`, case 073
- [x] v0.3 step 6 — ADAPT-004/007/010 and COMPAT-LSBLK/FINDMNT/LSNS: adapted execution
  through `ono-process`, the json decoder, adapter provenance in `inspect`, the fixture
  harness in `spec-check`, util-linux end to end (ADR-0057) — `ono-cli/tests/adapters.rs`,
  cases 074, 075. ADAPT-005's streaming half waits for the first line-protocol tool.
- [x] v0.3 step 7 — COMPAT-IP: the iproute2 pack, `ono.interface-address/1`, the field-map
  derivations children/template/first/infer/literals/require (ADR-0058) — case 076
- [x] v0.3 step 8 — ADAPT-005 streamed adaptation (`Decoding`, `Output::Pipe`,
  `start_piped`/`finish_foreground`, cancellation to the producer), the systemd pack
  (journalctl jsonl, systemctl list-units/show with the `properties` decoder),
  `ono.journal-event/1`, the live view absorbing plain records; the image gains git, curl,
  lsof (ADR-0059) — case 077
- [x] v0.3 step 9 — COMPAT-PS: the procps pack, whitespace columns, `first` on strings,
  `program-name`/`started-from-elapsed` inferences, streaming `lines` (ADR-0060) — case 078
- [x] v0.3 step 10 — COMPAT-STAT/DF/FIND: the coreutils and findutils packs, trailing argv,
  header lines, basename, NUL records with the path last, typed-order pass-through
  (ADR-0061) — case 079
- [x] v0.3 step 11 — COMPAT-GIT/LSOF: builtin decoders `git-status-v2` and `lsof-fields-v1`,
  `ono.git-status-entry/1`, `ono.commit/1`, `ono.open-file/1`, hex escapes (ADR-0062) — case 080
- [x] v0.3 step 12 — COMPAT-SS: combined flags, nested record coercion, the `ss-text-v6`
  version-constrained parser, required flags as specificity (ADR-0063) — case 081
- [x] v0.3 step 13 — the `adapt` keyword of §1.18 (E0911 when nothing answers) and
  COMPAT-CURL: `ono.http-exchange/1`, the `curl-exchange-v1` decoder with the body kept as
  exact bytes, secrets never adapting (ADR-0064) — case 082
- [x] v0.3 step 14 — ADAPT-008: `contributions.adapters`, the `executables`/`argv_policy`
  scope of `process.exec`, packs loaded disabled under default deny and enabled by
  `--grant process.exec` (experimental packs by `--allow-experimental` besides), the test
  host's `check_adapter_package`, the SDK's example package (ADR-0065) — case 083
- [x] v0.3 step 15 — ADAPT-011: the `start-adapt` frame, the agent negotiating, running and
  decoding on its side, records marked with the host, `explain … on <host>`, visible
  degradation (ADR-0066) — case 084
- [x] v0.3 step 16 — integration surfaces: adapted stages are producers for the pre-flight
  check, `type`, completion and history; text tools pinned raw; the §1.71 session, script
  determinism and the muscle-memory diff as cases (ADR-0067) — cases 085, 086, 087
- [x] v0.3 step 17 — release evidence: generated adapter reference pages and the compatibility
  matrix, live conformance for every first-party adapter (case 088), measured overhead
  (case 089), the README section with examples that parse and run under xtask — all §4.6
  boxes ticked

- [x] `get service <name>` reaches unloaded on-disk units, and the listing no longer reports
      `not-found` stubs. Investigation showed the by-name path already resolved through
      `LoadUnit`; the real defect behind the CI flake was the inverse — `ListUnits` enumerates a
      stub for a referenced unit whose file is gone, and the enumeration reported it as a
      service the by-name path then rightly denied. Both paths now agree — tests:
      `should_find_a_unit_on_disk_when_systemd_has_not_loaded_it`,
      `should_report_no_service_when_a_listed_unit_is_only_a_dangling_reference`
- [x] Bootstrap: Cargo workspace (`ono-cli`, `ono-core`, `ono-testkit`, `xtask`), pinned
      toolchain, lint configuration, first outcome tests — ADR-0001
- [x] Quality gate `scripts/gate.sh` and contract check `cargo xtask spec-check` — ADR-0001
- [x] Containerised acceptance harness: `docker/Dockerfile`, `docker/acceptance/cases/`,
      `scripts/acceptance.sh`, verified green with four cases — ADR-0002
- [x] Release gate `scripts/release-check.sh` and the stopping rule in `docs/ACCEPTANCE.md` —
      ADR-0002
- [x] CI running the gate and the acceptance suite on every push — ADR-0002
- [x] Specification immutability enforced by checksum in `cargo xtask spec-check` — ADR-0003
- [x] Branch policy: implementation on a disposable `implementation` branch, guarded in
      `scripts/gate.sh` — ADR-0004
- [x] Acceptance harness extended: `|` block scripts, stdin, `pty:`, `columns:`/`lines:`, `env:`,
      `timeout:` and repeatable assertions, with a self-test case — commit 036f89c
- [x] The gate refuses untracked unfinished work: `todo!()`, `unimplemented!()`, untracked
      `TODO`/`FIXME`, `#[ignore]` without a reason — `xtask/tests/scan.rs` — commit 6f7c308
- [x] `ono-testkit`: real-binary runs with a deadline, scratch directories, and a reproducible
      generator for fuzz-style tests — commits b2a0d2d, a20056c
- [x] `ono-render`: width-aware table and stacked-record layout, semantic theme tokens, the
      presentation contract of spec §4.6, and the ASCII tree of §22.4 — commits bb2d825,
      a3d3fac, 37f78de
- [x] `ono-history`: semantic entries, restart survival, secret policy — commit 0b1def8
- [x] A0 — Shared vocabulary in `ono-core`: `Span`, the complete error taxonomy of spec §43,
      the exit-status contract — ADR-0005/0006/0008 — commit 5551654 —
      tests `crates/ono-core/tests/{error_taxonomy,exit_status,span}.rs`
- [x] A0 — The concrete grammar: ADR-0009 and `docs/spec/grammar.ebnf`, resolving the
      command/expression ambiguity of spec §26.1 with the two argument modes

---

## Known defects (found by adversarial review, 2026-08-26)

Two independent reviewers were asked to falsify the implementation rather than describe it, as
AUTONOMOUS_IMPLEMENTATION.md §18 requires. Between them they found 27 things, each with a
reproduction they ran.

**Everything release-blocking is fixed**, each with a regression test that fails without the fix
(commits 0742918, aeae961). A ticked box below means fixed *and* guarded. What remains unticked is
should-fix or unbuilt, and each entry says which.

- [x] **R1 — nested blocks overflow the stack.** `if true { if true { … } }` nested about 2000
      deep aborts the process with SIGABRT. `MAX_DEPTH` in `crates/ono-parser/src/parser.rs` is
      consulted in `parse_stage` and in the expression parser but not in `parse_block`, so
      statement recursion is unguarded. The parser claims never to panic and always to return a
      tree, and it runs on every keystroke in the editor — one pasted line kills a login shell.
      `crates/ono-parser/tests/robustness.rs` has a test named for this that repeats `{` 2000
      times, which never enters block recursion: it passes while the thing it names is broken.
- [x] **R2 — `exit` in a configuration file hijacks the whole session.** `config::load` runs the
      config in the same `Session`, so `exit 3` there sets `session.leaving`, which is never
      cleared. Every later statement short-circuits and every command's status is replaced.
      Breaks ADR-0008 ("an external command's status is passed through unchanged") and ADR-0010
      ("a bad setting never stops the shell from starting").
- [x] **R3 — configuration mode stops external commands only.** The single-builtin fast path in
      `crates/ono-cli/src/eval.rs` returns before the `Mode::Config` check, so `cd`, `remove env`,
      `help`, `jobs`, `fg`, `bg` and `exit` all run from a config file. The error text the code
      itself prints says configuration "runs nothing". `028-config-is-restricted` only tries
      `touch`, so it does not prove what it claims.
- [x] **R4 — a builtin ignores its redirections and cannot be piped.** Verified fixed on
      2026-08-29 by the triage pass, though not the way the report imagined: both spellings are
      structured refusals naming the alternative rather than silent losses.
      `ono -c 'help > out.txt'` answers `Ono-Sendai-E0201 type.mismatch` — "`help` runs in the
      shell itself and cannot be redirected … Send it through a command that does:
      `help | to text > file`. The redirection at 5..14 was not applied" — writing no file *and
      saying so*; `ono -c 'help | cat'` answers "`help` runs in the shell itself and cannot be a
      pipeline stage", never `resolve.command_not_found`, and the run fails.
      Previously: **`help > out.txt` printed to stdout and wrote no file; `help | cat` reported
      `resolve.command_not_found` for `help` and then reported success.**
- [x] **R5 — an unterminated `${` eats the rest of the word.** `printf '[%s]' a${HOMEb` yields
      `[a$]`. `crates/ono-cli/src/expand.rs` drains the iterator looking for `}` and drops what it
      consumed, while its own comment says the text is kept as typed. Silent data loss inside an
      argument, which is the class of surprise ADR-0019 exists to remove.
- [x] **R6 — background children are only reaped when `jobs`/`fg`/`bg` runs.** A script that
      backgrounds 100 commands leaves 100 zombies, because `poll_jobs` is called only from the
      interactive loop and from the `jobs` builtin.
- [x] **R7 — a bad shebang reports 127 rather than 126.** `crates/ono-process/src/spawn.rs` maps
      every `ENOENT` from `exec` to `NOT_FOUND` without distinguishing the program from its
      interpreter. ADR-0008's table and every other shell say 126.
- [x] **R8 — a parse error echoes the whole source line.** A 100 000-character line produces a
      98 KB error message; the shown line needs a budget and an ellipsis.

What the review tried hard to break and could not, which is worth keeping: ADR-0019's rule that a
value's content never becomes a command's structure held under filenames containing spaces,
newlines, quotes, `$`, `*`, backslashes and raw escape bytes; file-descriptor hygiene is correct
including the fd-shuffle most hand-written shells get wrong; and the `pre_exec` SAFETY claim of
ADR-0007 is accurate as written.

### From the security review (ADR-0015 checklist)

Each was reproduced by the reviewer against the built binary. The release-blocking ones are fixed
and guarded; the rest stay open with their reproduction.

- [x] **F1 — `explain` prints attacker-controlled escape sequences raw.** A program name on `PATH`
      containing an OSC sequence retitles the terminal when `explain` reports it, and the bytes
      survive redirection into a file. `crates/ono-cli/src/builtin.rs` and
      `crates/ono-command/src/explain.rs` echo stage source and resolved paths without sanitising.
      ADR-0015 T1/T9/T11. The row's named acceptance case uses the benign name `ls`.
- [x] **F2 — structured error messages are not sanitised.** Only the code and the help line are
      painted through the theme; `error.message()` is written raw
      (`crates/ono-cli/src/report.rs`). `cd` into a directory whose name carries an OSC sequence
      retitles the window. ADR-0015 T1.
- [x] **F3 — a parse diagnostic sanitises the echoed line but not its own message.**
      `crates/ono-cli/src/report.rs`. ADR-0015 T1.
- [x] **F4 — `sanitise` lets `\n` and `\t` through, so a value forges a table row.** A cell
      containing `"evil\nroot      1"` renders as two terminal lines, the second indistinguishable
      from a real row. Widths are also measured on unsanitised text, so escapes misalign columns.
      `crates/ono-render/src/theme.rs`. ADR-0015 T1.
- [x] **F6 — resolution and execution disagree about a relative `PATH` entry.** `explain` stats a
      relative entry against the *process* working directory while the command runs with the
      *session's*, so `explain foo` reports one binary and `foo` runs another after a `cd`.
      `crates/ono-cli/src/resolve.rs` versus `crates/ono-cli/src/eval.rs`. ADR-0015 T10/T11 — it
      defeats that row's only stated mitigation.
- [x] **F7 — the history file is world-readable and ships with no redaction patterns.** Created at
      the ambient umask (0644, in a 0755 directory), and `Policy::default()` has an empty pattern
      list, so `deploy --password=hunter2` is stored verbatim. ADR-0015 T8; the row's named test
      supplies its own pattern, so it proves the mechanism rather than the product.
      `crates/ono-history/src/{store,policy}.rs`, `crates/ono-cli/src/repl.rs`.

Should-fix:

- [x] **F9 — fixed.** The prompt derives elevation from the kernel's effective uid: a root shell
      shows ` root` in `ui.prompt.root` and prompts with `#` (spec §17.2). Pinned from both
      sides in `ono-cli/tests/signals.rs::should_make_an_elevated_prompt_impossible_to_miss`.
- [x] **F10 — fixed** (as a side effect of the depth-guarded block recovery landed in the
      security sweep): every hostile wall — parens, brackets, blocks, `if`-chains — now parses
      20 000 deep in under 40 ms debug. The regression guard is
      `ono-parser/tests/robustness.rs::should_stay_linear_on_a_wall_of_unbalanced_parentheses`.
      Previously: **quadratic on unbalanced nesting** (24.8 s at 20 000).
- [x] **F11 — fixed.** The frontier holds paths, not descriptors: each directory re-opens from
      the held root through `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)`, so at most two
      descriptors are ever open and the T14 no-redirect property survives the change — a
      swapped component fails loudly instead of being followed. Pinned under a real 64-fd limit
      in `ono-cli/tests/native.rs::should_walk_a_wide_tree_without_hoarding_descriptors`.
      Previously: **one open descriptor per pending directory.**
- [ ] **F12 — the trust store's default policy is trust-on-first-use**, which contradicts ADR-0015
      T5's "an unknown key is refused, not prompted past". `crates/ono-protocol/src/trust.rs`.
      Either the ADR or the default has to move. Tracked as **issue #18**, and
      **ADR-0274 (`593baee`, 2026-08-29) now records why it cannot be settled yet**: both `ssh`
      and `local` go through `SubprocessTransport`, whose `peer_key` is truthfully `None`, so the
      pin store is never consulted in production and which default is right depends on whether
      first contact can be verified out of band. Copying `known_hosts` into the store is ruled
      out — it would assert a verification this process did not perform (ADR-0037 §4, §2.17).
- [x] **S1 (F13) — fixed.** `ProviderMutation` refuses a selection over the bulk threshold (10,
      a constant until configuration reaches invocations) with `safety.confirmation_required`
      naming the scope, before the first action; `--confirm` proceeds. `stop process` declares
      the option too. Pinned in `ono-command/tests/mutations.rs`. Previously: **the contract
      advertised a bulk-mutation guard nothing implements.** Four command
      contracts (`docs/spec/commands/file.yaml` twice, `network.yaml`, `kuang.yaml`) declare a
      `confirm` option documented as "without it, a selection over the configured threshold fails
      with `safety.confirmation_required` in a script (spec §11.6, §17.4)".
      `ProviderMutation::run` in `crates/ono-command/src/impls/mutate.rs` forwards it verbatim as
      an opaque argument and contains no threshold and no `safety.confirmation_required` path. A
      documented safety guard that does not exist is worse than no guard, because someone will
      rely on it. This is why `docs/ACCEPTANCE.md` §4.4's "destructive operations show scope
      before acting" cannot be ticked.
- [x] **S2 (F8) — fixed.** The systemd dry-run branches now answer `skipped` with what would
      have happened — the contract `ono-provider-linux` always kept — and the test that asserted
      a claimed change asserts the report; a declared `--dry-run` option travels in the action's
      own field rather than as an ignorable argument. Previously: **`Action::as_dry_run()` was
      unreachable, and one test encoded the wrong contract.**
      Nothing constructs a dry run: both call sites in `crates/ono-command/src/impls/mutate.rs`
      leave it false, no contract declares the option, and the `is_dry_run()` branches in
      `crates/ono-provider-systemd/src/provider.rs` are dead. Latent rather than live — but
      declaring `--dry-run` on a contract would make the flag arrive as an ordinary argument and
      the mutation would *run*. The systemd branches also report a completed change rather than
      `skipped`, and `crates/ono-provider-systemd/tests/service.rs` asserts that, so the wrong
      behaviour is currently guarded by a passing test. `ono-provider-linux` does it correctly.

Accepted for now, with the reason recorded so the decision is not re-made by accident:

- **F14** — bidirectional and other format characters pass the sanitiser, because
  `char::is_control()` covers only the `Cc` category. Trojan-Source display spoofing of a
  filename. Proposed as an extension of T1.
- **F15** — an empty `PATH` element resolves to the working directory. Deliberate, matches every
  other shell, and `explain` prints the absolute path it reached.
- **F16** — the history and trust-store temporary files are predictable and opened without
  `O_EXCL`. Only reachable in a directory another user can write, which F7 makes likelier than it
  should be; fix alongside F7.
- **F17** — a residual TOCTOU window remains between confirming a process's identity and
  signalling it. `pidfd_open`/`pidfd_send_signal` would close it; T13 claims only "re-read before
  signalling", which the code does.
- **F18** — `O_NOFOLLOW` does not stop `openat` descending into a bind mount;
  `openat2(RESOLVE_NO_XDEV)` would. T14 claims only that the walk cannot leave the tree *by name*,
  which holds.
- **F19** — `is_executable_file` tests `mode & 0o111` rather than `access(X_OK)`.
- **F20** — `FdPlan::normalise` opens `/dev/null` in a loop up to the target descriptor, so
  `9999>file` costs ten thousand opens. Self-inflicted.

### What the security review attacked and could not defeat

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

This section holds two kinds of entry, and both are tracked rather than silenced — `cargo xtask
spec-check`'s unfinished-work scan refuses an `#[ignore]`d test that no entry here names, and
refuses an entry that names no ADR.

**Blocked on something outside this repository.** Work that is merely unfinished is an issue, and
the 2026-08-29 triage moved everything that was really work out of here. One entry, and it is
blocked on the kernel.

**Red by design: the v0.4.1 phase H0 failure proofs (issue #31).** v0.4.1 §57 is explicit that no
production fix lands before the corresponding failure proof, so from 2026-09-02 the workspace
carries proofs that fail at HEAD *because the defect is real*. AGENTS.md §7 forbids committing a
failing test, so each one lands `#[ignore]`d with a `// REASON:` and an entry below naming the
issue that un-ignores it. Each entry states the assertion that may not be weakened when that
happens — a proof that is edited to fit the fix has stopped proving anything. These are the
opposite of a silenced requirement: they are the requirement, written down before the fix.

- **`socket.accepts_connection` cannot be observed.** It is declared in
  `docs/spec/spatial/relations.yaml`, claimed by no provider, and produces no edges (ADR-0135).
  Neither `sock_diag` nor procfs relates an accepted connection to the listener it came from, and
  matching by local port would be a guess v0.4 §11.5 has no value for. Unblocked only by a kernel
  interface that supplies the link; until then the relation is declared and honestly empty rather
  than faked or removed. **Exit test:** none can be written today, which is the point.

- **#71's measured cancellation distribution.** The p95 < 100 ms / p99 < 250 ms half of issue #71
  needs §37.2's *named reference environment*, which issue #84 delivers; asserting a wall clock on
  shared hardware is the trap ADR-0252 and issue #21 already recorded. The deterministic half is
  green and un-ignored — `crates/ono-pipeline/tests/cancellation.rs::should_stop_a_capture_growing_when_the_scope_is_cancelled`
  reads the source's counter after the operation unwinds, waits, and reads it again. The
  100-sample latency run was written, executed (100 cancellations in 0.07 s total, two orders of
  magnitude inside the target) and removed from the tree rather than left ignored — ADR-0459
  carries the measurement. **No ignored test exists for this.** Owed by **#83** and **#84**;
  §4.8.6's box stays unticked and says so.

- **§47.3's end-to-end signature proof needs a tag push.** ADR-0529 implements keyless Sigstore
  signing, self-verification and identity-constrained verification; `scripts/sign-release.sh`
  refuses without an OIDC identity and `scripts/verify-release.sh` fails closed. What the gate
  cannot do is make or check a *real* signature: that needs a Fulcio/Rekor round trip and a token
  that exists only inside a release run, and §40.2 denies the acceptance container a network. The
  two tests own the verification path against a stand-in `cosign`, which is honest about the
  outside world (AGENTS.md §11) and is not the proof. **§4.8.11's box for #107 is deliberately
  open**, with the reason written into the box. **Exit test:** the first `v*` tag's `publish` job
  signs and verifies.

- **The local rebuild comparison packages one binary twice.** ADR-0527. A second release *compile*
  needs a second target directory this machine cannot afford, so `rebuild-check.sh` locally proves
  the packaging layer is deterministic and not the compiler. In the release workflow the two builds
  are **two runners** per architecture, so there the binary is compiled twice and the comparison
  covers the whole chain — same script, both places. **Exit test:** a machine with the disk runs
  `rebuild-check.sh` over two independent compiles.

The v0.4.1 checklist's own guards, red by design (ADR-0575):

- **`docs/ACCEPTANCE.md` §4.8 names proofs that do not exist**, because the checklist was written
  before the tranche and guessed at the test names its increments would use. `§4.8.2`'s nine boxes
  are the bulk of it: they name `authentication.rs::…`, `link_identity.rs::…`, `agent_startup.rs`
  and `handshake.rs`, and the mutual-authentication work landed as
  `crates/ono-remote/tests/client_authentication.rs`, `peer_identity.rs`, `downgrade_resistance.rs`
  and `crates/ono-cli/tests/listening_agent.rs`.
  `xtask/tests/hardening_evidence.rs::should_find_every_test_the_v041_checklist_names_as_a_proof`
  is `#[ignore]`d and red, and the report it prints is the reconciling increment's worklist.
  ADR-0575. Un-ignored by that increment.
- **Forty boxes of §4.8 are open**, of which fourteen are P0 and twelve P1, so
  `xtask/tests/hardening_evidence.rs::should_find_every_p0_and_p1_box_of_the_v041_checklist_ticked`
  and `::should_find_a_dated_adr_for_every_box_the_checklist_leaves_open` are `#[ignore]`d and red.
  §66.9 allows an open box only as a P2/P3 exclusion recorded in an ADR dated before the
  release-candidate freeze, and §4.8.14 now states that freeze as 2026-09-04. ADR-0575.
  Un-ignored by the increment that ticks the last mandatory box.

The H0 failure proofs, red by design (issue #31, ADR-0430):

- **An orientation query asks a provider for every object when it needs a bounded view.**
  `crates/ono-cli/tests/spatial_first_output.rs::should_hold_every_time_to_first_result_target_of_the_reference_targets_table`
  is `#[ignore]`d and red: three of §33.2's four targets are missed — 595 ms against 150 ms,
  805 ms against 500 ms, 3 514 ms against 1 500 ms — and after H7 the remaining cause is a single
  one. `enter compute; look --json` pays 405 ms for 569 systemd units on **every** orientation, and
  `enter network; look --json` pays 3.4 s for 100 000 socket records. Neither is cardinality in the
  profile's sense; both are §34.4's second half. The fix is a bounded observation that **still
  reports the true count**, because an answer that lies about how much it left out trades a latency
  defect for an honesty defect (§2.6) — so it changes the observation contract and is its own
  increment. ADR-0496. Un-ignored by that increment.

Two entries left this section on 2026-08-29:

- **`service.depends_on`** moved onto the board as **B-prov-5**, and was **delivered the same day**
  by `bf25291` (ADR-0239). It was never blocked: the provider already called
  `GetAll(org.freedesktop.systemd1.Unit)` for every unit it emitted, and that reply carries
  `Requires`, `Requisite`, `BindsTo` and `Wants`. `ono.service/1` now has `dependencies`;
  `enter service systemd-journald.service; look` answers `dependencies available 4` where it
  answered nothing. Ordering (`After`, `Before`) is deliberately excluded: it says when, not
  whether.
- **The v0.4 RED suites** are delivered and green — the nine
  `crates/ono-cli/tests/spatial_*_missing.rs` files (175 tests) and the ten
  `docker/acceptance/cases/09x-spatial-*.case` scenarios (139 assertions), with
  `xtask/tests/spatial_evidence.rs` failing the gate if a `*.case.v04` file returns or if a test
  `docs/ACCEPTANCE.md` §4.7 names as a proof goes missing or ignored. The files keep their
  `_missing` names because renaming them would rename 113 proofs the checklist points at, which is
  a `refactor` of its own. The three questions those suites could not settle are fixed contracts
  now: **ADR-0124** (spatial verbs take the bare name; `find place` beside `find file`, so bare
  `find` stays findutils and case `087` stays green; `look` shadows util-linux `look`),
  **ADR-0125** (the fourteen §40 conditions are the family `spatial`, `Ono-Sendai-E1001`–`E1014`,
  in `docs/spec/errors.yaml` — one taxonomy in one file), and **ADR-0126** (the registry is
  `docs/spec/spatial/{spatial,spaces,relations,landmarks}.yaml`).

---

## Notes for whoever starts phase A

- Switch to `implementation` before your first edit. The gate refuses to run on `main`.
- The workspace is green as delivered. Confirm it (`scripts/gate.sh`) before your first edit, so
  a later red gate is unambiguously yours.
- `crates/ono-cli/src/main.rs` is scaffolding: it answers `--version` and `--help` and refuses
  everything else. Replacing its argument handling with the real interpreter is expected and
  needs no ADR; the three acceptance cases guarding it must keep passing.
- Crate names not yet created (`ono-parser`, `ono-value`, `ono-pipeline`, …) come from spec
  section 24.2 with the `ono-` prefix. Create them as the phase needs them, not upfront.
- Add the acceptance case in the same increment as the capability. A feature without a case in
  the container does not count as delivered (`docs/ACCEPTANCE.md` section 2).
- The specification is read-only and checksum-enforced. When it is ambiguous, wrong or in your
  way, write an ADR with a `Spec deviation` heading and implement your decision — never edit the
  spec (AGENTS.md section 5.1, ADR-0003).
