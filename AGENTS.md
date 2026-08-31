# AGENTS.md — Operating Instructions for Autonomous Agents

> This file is the **single source of truth** for how any AI agent (Codex, Claude Code, or any
> other) works in this repository. `CLAUDE.md` is a thin Claude-specific layer that points here.
> Read this file completely before your first action in a session.

---

## 1. Prime Directive

Build **Ono-Sendai**, the shell specified in `docs/ono_sendai_shell_spec_v0.2.md`, to completion —
**test-driven, autonomously, without asking the user for input.**

Four rules override everything else:

1. **Do not block on the user.** Every decision that is not explicitly fixed in the spec
   is yours to make. Decide, record the decision (§8), continue.
2. **Tests are the referee.** A (sub)goal counts as reached only when its tests are green
   and the full quality gate (§10) passes. Only then start the next step.
3. **No test, no code.** Production code is written *after* a failing test exists for it.
4. **Be pragmatic and stay on task.** Solve the problem in front of you, nothing else (§4).
5. **Do not stop early.** The run ends when `scripts/release-check.sh` passes and not before
   (§15). There is no MVP exit and no proof-of-concept exit.
6. **Never commit to `main`.** All implementation happens on the `implementation` branch, so the
   whole run stays disposable (§12.1).

If you catch yourself writing "should I…?", "which option do you prefer?", or "let me know
how to proceed" — stop, pick the option best aligned with the spec, write an ADR, proceed.

---

## 2. Repository Layout

**Everything an agent needs to read lives under `docs/`.** No `spec/`, `adr/`, `rfc/` or similar
directory at the top level.

```
ono-sendai/
├── AGENTS.md                     these instructions (authoritative)
├── CLAUDE.md                     thin pointer + Claude-specific notes
├── README.md
├── Cargo.toml                    workspace root
├── crates/                       implementation, `ono-*` crates (see spec §24.2)
├── tests/                        cross-crate integration tests
├── examples/
├── xtask/                        spec validation, generators, gates
├── scripts/                      gate.sh, acceptance.sh, release-check.sh
├── docker/                       Dockerfile + acceptance/cases/ (the referee, §10)
└── docs/
    ├── ono_sendai_shell_spec_v0.2.md
    │                             base narrative spec (normative)
    ├── ono_sendai_shell_spec_v0.3_external_command_adapters.md
    │                             enhancement spec, layered on the base (§5.2)
    ├── ono_sendai_shell_spec_v0.4_spatial_systems_interface.md
    │                             enhancement spec, layered on the base (§5.2)
    ├── ono_sendai_shell_spec_v0.5_temporal_causal_systems_interface.md
    │                             enhancement spec, layered on the base (§5.2)
    ├── ono_sendai_shell_spec_v0.6_prospective_change_protection_recovery.md
    │                             enhancement spec, layered on the base (§5.2)
    ├── STATE.md                  progress board (§9)
    ├── ACCEPTANCE.md             definition of release-ready + stopping rule (§15)
    ├── decisions/ADR-*.md        recorded agent decisions (§8)
    ├── spec/                     machine-readable contracts
    │   ├── language.yaml
    │   ├── grammar.ebnf
    │   ├── verbs.yaml
    │   ├── targets.yaml
    │   ├── errors.yaml
    │   ├── capabilities.yaml
    │   ├── commands/*.yaml
    │   ├── schemas/*.v1.yaml
    │   ├── providers/*.yaml
    │   └── kuang/                KUANG/11 contracts (spec §31.78)
    └── reference/                generated docs — never hand-edited
```

This deviates deliberately from the top-level `spec/` sketched in spec §24.2, §47 and §31.78:
**the directory is `docs/spec/`.** Read every `spec/...` path in the narrative spec as
`docs/spec/...`, including `spec/kuang/...` as `docs/spec/kuang/...`. Only build artifacts,
source code and tooling belong at the top level.

---

## 3. Naming

Two names coexist deliberately, and they are not interchangeable (spec §0, §31):

| Thing | Name |
|---|---|
| Project / product / full name | **Ono-Sendai** |
| Short name, binary, command, prose default | **`ono`** — the form to prefer everywhere |
| Cargo crates | `ono-*` — spec §24.2 (`ono-cli`, `ono-core`, `ono-parser`, `ono-value`, …) |
| Config paths, env vars, protocol ids | derived from `ono`: `~/.config/ono/`, `ONO_*` |
| Reserved ID namespace | `ono.*` — only the Ono project may claim it (spec §31.5) |
| Extension runtime / plugin system | **KUANG/11** — spec §31, nothing else |
| KUANG/11 crates and contracts | `ono-kuang-supervisor`, `ono-kuang-protocol`, `ono-kuang-sdk`, `docs/spec/kuang/` |

> **Ono is the deck; KUANG/11 is the software that can be loaded into the deck** (spec §0).

Consequences for anything you write:

- Prose about the shell itself says *Ono* (or *Ono-Sendai* where the full name reads better),
  never "KUANG" — the old project name is gone.
- `kuang` appears **only** in KUANG/11 context: the `kuang` engine component (spec §24.1), the
  `ono-kuang-*` crates, the `docs/spec/kuang/` contracts, the `kuang_api` manifest field.
  Anywhere else it is a leftover of the old name and MUST be renamed.
- Third-party plugin IDs, command IDs, schema IDs and capability IDs are publisher-namespaced
  (`dev.example.packet-eye.…`) and MUST NOT claim `ono.*` (spec §31.5).
- If you touch a file that still carries an old name, rename it as part of that increment; do
  not do a repository-wide sweep inside an unrelated change (§4). The narrative specification is
  exempt: it is never renamed or edited, whatever it says inside (§5.1).

---

## 4. Pragmatism and Separation of Concerns

**Code and reviews are pragmatic and problem-oriented.** Write the straightforward thing that
solves the actual requirement. No speculative generality, no abstraction for a second use case
that does not exist, no framework where a function suffices, no premature optimisation.
"Might be useful later" is not a reason; a test that demands it is.

**Improvements are strictly separated from fixes.** These are different kinds of work and MUST
NOT share a commit or a change:

| Kind | What it does | Rule |
|---|---|---|
| `feat` | new behaviour | driven by a new test (§7) |
| `fix` | wrong behaviour → correct behaviour | starts with a test that reproduces the bug |
| `refactor` | same behaviour, better structure | **no test may change** (§11) |
| `perf` | same behaviour, measurably faster | needs a benchmark showing the gain |
| `test` | test coverage only | no production code changes |

Concretely:

- Fixing a bug? Fix **that** bug. Do not reformat the file, rename neighbours, tidy imports or
  "improve while you're there". Note improvement ideas in `docs/STATE.md` → *Next up*.
- Refactoring? Change no behaviour. If the tests need editing, it is not a refactor — split it.
- Found a real problem outside your current increment? Write it into `docs/STATE.md` (or add a
  failing, `#[ignore]`d test with a `// REASON:` comment) and continue your task. Fix it in its
  own increment.
- Reviews follow the same discipline: report defects as defects and preferences as preferences,
  and state which of the categories above a finding belongs to. A stylistic preference never
  blocks a correct, tested change.

---

## 5. Authority Order (what wins when sources disagree)

```
0. docs/ono_sendai_shell_spec_v0.6_prospective_change_protection_recovery.md
0. docs/ono_sendai_shell_spec_v0.5_temporal_causal_systems_interface.md
0. docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md
0. docs/ono_sendai_shell_spec_v0.3_external_command_adapters.md
                                   the enhancement specs share level 0 (IMMUTABLE, read-only);
                                   where two of them overlap, the later version wins — §5.2
1. docs/ono_sendai_shell_spec_v0.2.md   base narrative spec — intent & semantics (IMMUTABLE, read-only)
2. docs/spec/*.yaml, grammar.ebnf  machine-readable contracts (public API surface)
3. docs/decisions/ADR-*.md         recorded agent decisions (fill gaps in 1 & 2)
4. docs/ACCEPTANCE.md              what "finished" means, in checkable boxes
5. tests/ + docker/acceptance/     executable behaviour contract
6. crates/                         implementation
7. docs/reference/, generated code derived artifacts — never hand-edited
```

Lower levels must never silently contradict higher ones. If implementation reality forces a
change at level 2 or below, **change it explicitly** in the same commit and note it in the
commit body. Level 1 is the exception, and the exception is absolute:

### 5.2 The specification is a base plus enhancements

`docs/ono_sendai_shell_spec_v0.2.md` is the **base**. The user may add further narrative
specifications beside it — any `docs/ono_sendai_*spec_v<version>*.md` — and each is an
**enhancement layered on top of the base**, not a replacement for it. The base still governs
everything the enhancement does not speak about; where they overlap, **the later version wins**,
and the ADR implementing that part cites both.

The user names these files, and the name is not a shape to rely on. v0.4 and v0.5 each arrived
without the `shell_spec` infix the harness matched on, and each was renamed afterwards to restore
it — a correction only the user may make (§5.1) and one no gate can require. What every one of
them carries is the product name and a version, so that is what discovery matches on and how the
base is identified — the lowest version, never whatever sorts first (ADR-0423).

Every one of them is immutable under §5.1, without exception, and `spec-check` enforces two
things on every gate run (ADR-0026):

- each narrative specification has a `sha256sum` line in `docs/spec.sha256`, so none of them can
  be edited unnoticed;
- this file enumerates each enhancement by name, so no enhancement can sit in `docs/` unread.

The enhancements present, newest first:

- `docs/ono_sendai_shell_spec_v0.6_prospective_change_protection_recovery.md` — Prospective
  Change, Protection & Recovery: the `ChangePlan` as a first-class object, so a mutation can be
  inspected for its consequences and its recoverability before it is made real. Added 2026-08-31,
  **not implemented**; `docs/STATE.md` records it behind v0.5.
- `docs/ono_sendai_shell_spec_v0.5_temporal_causal_systems_interface.md` — the Temporal & Causal
  Systems Interface: time as a coordinate, an evidence-backed event ledger, state reconstruction,
  timelines and causal explanation. Added 2026-08-31, **not implemented**; `docs/STATE.md`
  records it as the next tranche.
- `docs/ono_sendai_shell_spec_v0.4_spatial_systems_interface.md` — the Spatial Systems
  Interface. Added 2026-08-27, implemented 2026-08-28/29 (ADR-0124 … ADR-0402) and released as
  0.4.0; `docs/ACCEPTANCE.md` §4.7 holds its checklist.
- `docs/ono_sendai_shell_spec_v0.3_external_command_adapters.md` — the External Command
  Adaptation Layer. Implemented (ADR-0052 … ADR-0067).

Adding an enhancement is the user's action. Reconciling the code, the contracts and the ADRs with
it is the agent's, and it is ordinary work in the loop of §7 — not a reason to stop and ask.

### 5.1 The initial specification is immutable

**`docs/ono_sendai_shell_spec_v0.2.md` MUST NOT be edited, amended, reformatted, renamed,
regenerated or replaced.** Not to fix a typo, not to reflow a paragraph, not to update a name,
not to correct something you believe is wrong, not to record a decision, not "while you are in
there". No agent may write to that file for any reason. It is the fixed reference every later
artifact is measured against, and it stops being that the moment anyone edits it.

This holds even when the spec is:

- **ambiguous** — resolve it in an ADR (§8) and implement to your ADR;
- **silent** — decide, write an ADR, proceed;
- **internally inconsistent** — write an ADR that states both readings, chooses one and says
  why; implement the chosen one;
- **apparently wrong or unimplementable** — write an ADR recording the finding, the evidence
  and the deviation you implement instead. Deviating from the spec is allowed; rewriting it to
  match your deviation is not;
- **out of date with the code** — the code is what changes, or an ADR records the divergence.

**ADRs are the only mechanism** for resolving anything the spec leaves open or gets wrong. An
ADR that supersedes spec text MUST cite the exact section (`spec §N.M`), quote the sentence it
departs from, and state the rule that replaces it. Later readers then find the spec intact and
the deviations enumerated beside it.

Only the user changes the specification, and only by replacing it deliberately as a new version.
If that happens, the agent's job is to reconcile the code and the ADRs with the new text — never
the other way around.

The narrative spec is explorative in tone. Where it says "plausible", "suggested", "MAY" or
leaves an Open Design Question (spec §39), **you decide** — see §8. Deciding means writing an
ADR, never annotating the spec.

---

## 6. Non-Negotiable Constraints

Fixed by the spec; not open for agent re-decision:

- Language: **Rust**, stable toolchain, 2021 edition or newer.
- Cargo **workspace** with `crates/*` as sketched in spec §24.2. Deviate only with an ADR.
- Machine-readable `docs/spec/` registries are the **public contract**; commands, schemas, verbs
  and errors are defined there first, implemented second (spec §27, §36, §47).
- **Structured values, not text parsing.** Never parse unstable human-readable output of
  external tools in a provider unless documented as an explicit adapter fallback (spec §50).
- Unknown data is `null`, never fabricated or zero (spec §35.3).
- Repository language is **English**: code, comments, identifiers, tests, docs, commit
  messages, ADRs. (Conversation with the user may be German.)
- Non-goals in spec §38 stay non-goals.
- The narrative specification is **read-only** for every agent (§5.1). Ambiguities and
  implementation decisions are resolved through ADRs, never by touching the spec.

---

## 7. The TDD Loop (mandatory working rhythm)

Work in **small, individually shippable increments**. One increment = one loop:

```
0. SELECT     pick the next task (§9). Write it into docs/STATE.md as in-progress.
1. CONTRACT   if the increment adds public surface: update/create the docs/spec/*.yaml entry first.
2. RED        write the test(s) that express the desired observable behaviour. Run them.
              They MUST fail, and fail for the right reason. A test that passes immediately is
              a broken test — fix the test, not the assertion count.
3. GREEN      write the minimum implementation that makes them pass. No speculative features.
4. REFACTOR   clean up names, duplication, module boundaries — with the tests untouched (§11).
5. GATE       run the full quality gate (§10). Everything green.
6. RECORD     ADR if a non-trivial decision was made; update docs/STATE.md; commit (§12).
7. LOOP       go to 0.
```

Rules for the loop:

- **Never** commit with a failing or ignored test. `#[ignore]` requires a `// REASON:` comment
  and an entry in `docs/STATE.md` under *Deferred*.
- If a step is blocked (missing dependency, unclear semantics), do **not** stop. Split the
  task: implement what is decidable, write the blocked part as an `#[ignore]`d test plus an ADR
  stating the assumption, and move on.
- If green cannot be reached after **three** honest attempts, revert the increment
  (`git restore` / reset to the last green commit), record what failed in `docs/STATE.md`, and
  choose a smaller increment. Never leave the tree broken.
- Never weaken or delete a test to make the suite pass. Tests only change when the *contract*
  changes — and then the contract (`docs/spec/`) or an ADR changes in the same commit. The
  narrative spec never changes (§5.1).

---

## 8. Autonomous Decisions and ADRs

You are explicitly authorised to decide, without asking:

- crate boundaries, module layout, internal APIs, data structures;
- library choices (parser generator, async runtime, CLI, serde formats, test tooling);
- error handling strategy, trait design, generics vs. dyn, sync vs. async;
- naming of internals, test organisation, fixture strategy;
- resolution of spec Open Design Questions (§39) and every "plausible"/"suggested"/"MAY";
- refactors of your own earlier code, including reversing an earlier ADR.

**Record it** as an ADR whenever the decision is architectural, cross-cutting, hard to reverse,
resolves a spec ambiguity, or picks between real alternatives. Trivial local choices need no ADR.

Format — `docs/decisions/ADR-NNNN-kebab-title.md` (NNNN = zero-padded, monotonic):

```markdown
# ADR-0007: Parser library choice

- Status: accepted            # proposed | accepted | superseded by ADR-XXXX
- Date: 2026-08-26
- Spec refs: §24.4, §26
- Decided by: agent (autonomous)

## Context
What forced a decision; which spec text is silent or ambiguous.

## Decision
The choice, stated as a rule future agents can follow.

## Consequences
What becomes easy, what becomes hard, what must be revisited, which tests encode it.

## Alternatives considered
Option — why rejected.
```

When the ADR departs from something the specification actually says, add one more heading:

```markdown
## Spec deviation
- Section: spec §31.12
- Text: "<the sentence being departed from, quoted>"
- Instead: <the rule that applies now>
- Why: <evidence — what made the specified behaviour wrong or unimplementable>
```

This is the only place a deviation may be recorded. The spec itself stays untouched (§5.1), so
the set of ADRs carrying a `Spec deviation` heading is the complete, greppable list of every
point where the product differs from its specification.

Superseding is allowed and expected: write a new ADR, set the old one to
`superseded by ADR-XXXX`, never edit the history of accepted ADRs.

**Escalate to the user only if** an action would be destructive outside the repository, requires
credentials or network access you do not have, has legal/licensing implications, or the spec is
self-contradictory in a way that makes both readings produce user-visible wrong behaviour.
Even then: state your chosen default, proceed with it, and flag it — do not idle.

---

## 9. Task Selection and Progress State

`docs/STATE.md` is the shared, machine- and human-readable progress board. **Read it first,
update it last, in every session.** If it does not exist, create it from this template:

```markdown
# STATE

Current phase: A — Language and Unix shell foundation (spec §37)
Phase exit criterion: <copied from spec §37>

## In progress
- [agent-id | timestamp] <task> — files: <paths>

## Next up (ordered)
- [ ] <task> — spec §X — exit test: <test name/path>

## Done
- [x] <task> — commit <sha>

## Deferred / blocked
- <task> — reason — ignored test: <path::name> — ADR: <id>
```

Task ordering rules:

1. Follow the phase sequence of spec §37 (A → J). Do not start Phase C work while Phase B
   exit criteria are unmet, unless the item is strictly independent.
2. Within a phase, prefer the task that unblocks the most other tasks.
3. Prefer the machine-readable contract (`docs/spec/*.yaml`) before its implementation.
4. Repairing the gate or the acceptance harness (§14) outranks features whenever either
   cannot run — a broken referee makes every later claim of progress worthless.
5. A phase is complete only when its success criterion in spec §37 is demonstrated by an
   automated test — write that test explicitly and name it in `docs/STATE.md`.

---

## 10. Quality Gate (Definition of Done)

An increment is done only when the gate passes locally:

```bash
scripts/gate.sh          # or: cargo xtask gate
```

which runs, in order:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo run -p xtask -- spec-check          # contract ↔ implementation drift (spec §36.5)
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

**And, for any increment that adds or changes a user-visible capability, the container:**

```bash
scripts/acceptance.sh    # or: cargo xtask acceptance
```

`scripts/acceptance.sh` builds `docker/Dockerfile` and runs every case in
`docker/acceptance/cases/` against the real `ono` binary, as an unprivileged user whose login
shell is `ono`, with networking disabled. **A capability without a passing acceptance case is
not delivered** — write the case in the same increment as the feature, not afterwards
(`docs/ACCEPTANCE.md` §2). Unit tests prove the code is sound; only the container proves the
product exists.

Additionally, per spec §50, an increment that advertises a user-visible capability is not done
until: help is complete, completion metadata exists, the output schema is inspectable, error
cases are structured (spec §43 taxonomy), doc examples parse and run, and behaviour is
deterministic when output is redirected.

`cargo xtask spec-check` MUST fail on contract drift (spec §36.5): undocumented stable command,
metadata without implementation, doc example that no longer parses, schema break without version
bump, provider output violating its advertised schema.

If a gate tool is not installed or not yet bootstrapped, **create it** (§14) rather than
skipping the gate.

---

## 11. Testing: Behaviour, Not Structure

**Tests describe *what* the system does, never *how* it does it.** This is the rule that makes
autonomous refactoring safe.

Binding consequences:

- **A pure refactor MUST leave the test suite green and unchanged.** Renaming a type, splitting
  a module, moving a function between crates, replacing an algorithm, changing an internal trait
  — if observable behaviour is identical, every test still passes without edits.
- If a restructuring forces test edits, one of two things is true: the change altered behaviour
  (then it is not a refactor — split it into `feat`/`fix` with its own tests), or **the test was
  coupled to structure and is itself the defect** (then fix the test to assert the outcome, in a
  separate `test:` commit, before the refactor).
- Assert on **outcomes at a contract boundary**: command output values and their schema, exit
  status, stream contents, emitted diagnostics/error codes, filesystem or system state after the
  run, rendered output for renderer tests. Not on: call counts, call order, which internal
  function ran, private fields, mock interactions, log lines that are not a contract.
- **Integration tests are outcome tests.** Given this input state and this command line, the
  resulting objects/state/exit code are these. They MUST NOT know the implementation path.
- **Test through public API.** Unit tests are allowed for pure logic with a genuine contract of
  its own (a parser rule, a unit-arithmetic function). Do not make items public solely to test
  them; if a behaviour cannot be observed from outside, ask whether it needs a test at all.
- No mocks of your own internals. Fake the *outside world* (procfs fixture, container, PTY),
  not your own layers.
- Name tests after behaviour: `should_<observable behaviour>_when_<condition>`. A test name
  containing an internal type or function name is a smell.

Test layers (spec §35):

| Layer | Location | Content |
|---|---|---|
| Unit | `#[cfg(test)]` in module | pure logic with its own contract: value semantics, edge cases |
| Golden AST | `crates/ono-parser/tests/` | parse trees, diagnostics snapshots, incremental parse |
| Property | testkit-driven | serialization round trips, unit arithmetic, null semantics |
| Fuzz | `fuzz/` | parser, serializers, protocol, procfs/netlink decoders (spec §35.6) |
| Conformance | generated from `docs/spec/providers/*.yaml` | every provider capability (spec §35.3) |
| Integration | `tests/` | container/VM fixtures: processes, systemd, sockets, PTY, signals (spec §35.4) |
| Snapshot | `tests/render/` | renderer output only — **never** a data contract (spec §35.5) |
| Plugin conformance | KUANG/11 test host (spec §31.73) | manifest/schema validation, capability denial paths, cancellation, backpressure, quotas (spec §31.74) |
| Doc examples | via `cargo xtask` | every documented example must parse and execute |

Test quality rules: deterministic (no wall-clock, network or ordering dependence), isolated (no
shared mutable global state; never rely on the developer machine's real processes unless the
fixture creates them), one behaviour per test, assertion messages that explain the contract.

---

## 12. Branch Policy and Commits

### 12.1 Implementation lives on a feature branch

**`main` is never written to by an agent.** It holds the immutable specification, these
instructions, the verification harness and the README — the state a run starts from, and the
state the user can always return to.

**All implementation happens on the branch `implementation`.** It is created from `main` and is
**disposable by design**: the entire run can be thrown away and restarted from a clean `main`
without losing anything that matters, because everything that matters is on `main` already.

```bash
git switch implementation || git switch --create implementation main   # start or resume
git rev-parse --abbrev-ref HEAD                                        # confirm before editing
```

Rules:

- Confirm the branch **before your first edit**, every session (§17). A commit on `main` is a
  policy breach even when the content is good.
- Never merge, rebase or fast-forward `implementation` into `main`. Promoting the work is the
  user's decision and the user's action, taken when the release gate passes (§15).
- Never delete or recreate `implementation` yourself. Discarding a run is the user's call.
- `scripts/gate.sh` refuses to run on `main`. That guard is not to be removed or worked around;
  the user sets `ONO_ALLOW_MAIN=1` when they work on the harness itself.
- Sub-branches are allowed for parallel agents (`implementation/<crate>`), merged back into
  `implementation` — never into `main`.

### 12.2 Commits

- **Every commit must be green** (§10). One increment of **one kind** per commit (§4).
- Conventional Commits: `feat|fix|refactor|perf|test|docs|spec|chore(scope): summary`
- Body: what changed, which spec section it implements, which ADR it follows, which tests prove
  it. For `refactor`: state explicitly that no test changed.
- Never `--force`, never rewrite pushed history, never commit generated artifacts that the
  generator can reproduce (except where CI needs them checked in — then say so in an ADR).
- Push `implementation` freely so work is not lost. Do **not** open PRs unless asked.

---

## 13. Multi-Agent Coordination

When several agents work in parallel:

- Claim work by writing your entry into `docs/STATE.md` → *In progress* **before** editing code;
  include agent id, timestamp and the file paths you will touch.
- **Never** edit files another agent has claimed. Pick a different task from *Next up*.
- Prefer decomposition along crate boundaries — one crate per agent — to avoid conflicts.
- Shared documents (`docs/spec/*`, `docs/STATE.md`) are edited in short, single-purpose commits;
  re-read the file immediately before editing it.
- Integration role: after N increments, run the full gate on the merged tree and fix drift.
- A stale claim (no commit for its files, older than the current session) may be reclaimed;
  note the takeover in `docs/STATE.md`.

---

## 14. The Harness (already built — keep it working)

The bootstrap is **done**. The repository ships a green baseline; confirm it before your first
edit so a later red gate is unambiguously yours.

| Piece | What it is |
|---|---|
| `Cargo.toml`, `rust-toolchain.toml` | workspace, toolchain pinned to 1.94 (ADR-0001) |
| `crates/ono-cli` | the `ono` binary — scaffolding: `--version`, `--help`, usage error |
| `crates/ono-core`, `crates/ono-testkit` | shared types; test helpers for outcome assertions |
| `xtask` | `gate`, `spec-check`, `acceptance`, `release-check` |
| `scripts/gate.sh` | the quality gate of §10 |
| `scripts/acceptance.sh` | builds the container, runs `docker/acceptance/cases/` |
| `scripts/release-check.sh` | the stopping rule of §15 |
| `.github/workflows/ci.yml` | gate + acceptance on every push |

Rules for the harness itself:

- **The referee outranks the feature.** If the gate or the acceptance harness cannot run, fixing
  it is the next task, ahead of anything in *Next up*.
- Never weaken the harness to get a green result: not by deleting a case, not by loosening a
  regex, not by removing `-D warnings`, not by adding `--no-verify`. If a case is wrong, fix the
  case in its own `test:` commit and say why in the body.
- The scaffolding in `ono-cli/src/main.rs` is meant to be replaced by the real interpreter. Its
  three acceptance cases are a floor and must keep passing.
- Crates from spec §24.2 (`ono-parser`, `ono-value`, `ono-pipeline`, …) are created when a phase
  needs them, not upfront (ADR-0001).
- `docs/spec/` registries arrive with Phase D (spec §47); `docs/spec/kuang/` with Phase I
  (spec §31.78). `spec-check` already fails on a top-level `spec/`, on a missing narrative spec,
  on instructions that reference a spec file that does not exist, and on empty contracts.
- **`spec-check` verifies the specification against `docs/spec.sha256` on every gate run.** Any
  edit to the narrative spec turns the gate red (§5.1). Restore the file; do not update the
  checksum. `docs/spec.sha256` is the user's to change, when they replace the spec on purpose.

---

## 15. Stopping Rule

`docs/ACCEPTANCE.md` defines *finished*, and `scripts/release-check.sh` evaluates it: the
quality gate, then the containerised acceptance suite, then a scan for unticked boxes in the
release checklist.

**Stop when — and only when — it prints `release-check: the shell is release-ready`.**

Not stopping conditions, individually or together:

- the quality gate is green;
- the acceptance suite passes;
- a phase from spec §37 is complete;
- the repository looks tidy and `docs/STATE.md` has an empty *In progress*;
- the remaining work looks large, tedious or unglamorous;
- you have been working for a long time.

If a box in `docs/ACCEPTANCE.md` §4 is unticked, there is a next task. Decompose it into the
next increment, write it into `docs/STATE.md`, and continue the loop of §7. Tick a box only when
an automated case or test that runs in the gate proves it — never on judgement.

The deliverable is a shell someone can set as their login shell and keep. Not a demo, not a
prototype, not "the interesting parts". Handing back an unfinished shell because the remainder
is routine is the one failure mode this project cannot absorb.

---

## 16. Code Style

- Idiomatic Rust; `rustfmt` defaults are authoritative — never hand-format around it.
- Public items in library crates carry doc comments; doc examples compile.
- `unsafe` requires a `// SAFETY:` comment and an ADR if it crosses a crate boundary.
- No `unwrap()`/`expect()`/`panic!` in library code paths reachable from user input; use the
  structured error model (spec §16, §43). `expect()` is acceptable in tests and in provably
  unreachable states with a justifying comment.
- Errors are typed and structured, carrying spans/identity where the spec requires it.
- Comments explain *why*, not *what*. Match the density of surrounding code.
- No TODO without a matching entry in `docs/STATE.md` → *Deferred*.

---

## 17. Session Checklist

**Start:** read `AGENTS.md` → `docs/STATE.md` → `docs/ACCEPTANCE.md` → recent ADRs →
`git log --oneline -10` → **switch to `implementation` and confirm you are not on `main`**
(§12.1) → run `scripts/gate.sh` to confirm you start from green.

**During:** one TDD loop at a time; one kind of change at a time; decide instead of asking;
ADR for anything architectural; an acceptance case for anything a user can see.

**End:** gate green → acceptance green if a capability changed → `docs/STATE.md` updated
(In progress cleared, Done/Next up accurate, boxes ticked in `docs/ACCEPTANCE.md` only where
proven) → committed → then run `scripts/release-check.sh`. If it does not print
`release-check: the shell is release-ready`, take the next task and keep going (§15).
