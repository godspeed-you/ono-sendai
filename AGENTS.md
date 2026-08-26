# AGENTS.md — Operating Instructions for Autonomous Agents

> This file is the **single source of truth** for how any AI agent (Codex, Claude Code, or any
> other) works in this repository. `CLAUDE.md` is a thin Claude-specific layer that points here.
> Read this file completely before your first action in a session.

---

## 1. Prime Directive

Build **Ono-Sendai**, the shell specified in `docs/ono-sendai_shell_spec_v0.1.md`, to completion —
**test-driven, autonomously, without asking the user for input.**

Four rules override everything else:

1. **Do not block on the user.** Every decision that is not explicitly fixed in the spec
   is yours to make. Decide, record the decision (§8), continue.
2. **Tests are the referee.** A (sub)goal counts as reached only when its tests are green
   and the full quality gate (§10) passes. Only then start the next step.
3. **No test, no code.** Production code is written *after* a failing test exists for it.
4. **Be pragmatic and stay on task.** Solve the problem in front of you, nothing else (§4).

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
└── docs/
    ├── ono-sendai_shell_spec_v0.1.md
    │                             narrative spec (normative)
    ├── STATE.md                  progress board (§9)
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
    │   └── providers/*.yaml
    └── reference/                generated docs — never hand-edited
```

This deviates deliberately from the top-level `spec/` sketched in spec §24.2 and §47: **the
directory is `docs/spec/`.** Read every `spec/...` path in the narrative spec as `docs/spec/...`.
Only build artifacts, source code and tooling belong at the top level.

---

## 3. Naming

The project was renamed from *Kuang* to **Ono-Sendai**; the narrative spec still says "KUANG"
throughout. Read every occurrence of that word in the spec as **Ono-Sendai**, and apply these
names in everything you write:

| Thing | Name |
|---|---|
| Project / product | **Ono-Sendai** |
| Short name, binary, command, prose default | **`ono`** — the form to prefer everywhere |
| Cargo crates | `ono-cli`, `ono-core`, `ono-parser`, `ono-value`, … — spec §24.2 with the `ono-` prefix |
| Config paths, env vars, protocol ids | derived from `ono`: `~/.config/ono/`, `ONO_*` |
| Plugin / extension system | **Kuang/11** — reserved for the plugin system of spec §31, nothing else |

`kuang` MUST NOT appear as a prefix, crate name, identifier or path for anything outside the
Kuang/11 plugin system. If you touch a file that still carries the old name, rename it as part
of that increment; do not do a repository-wide sweep inside an unrelated change (§4).

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
1. docs/ono-sendai_shell_spec_v0.1.md   narrative spec — intent & semantics (normative MUST/SHOULD/MAY)
2. docs/spec/*.yaml, grammar.ebnf  machine-readable contracts (public API surface)
3. docs/decisions/ADR-*.md         recorded agent decisions (fill gaps in 1 & 2)
4. tests/                          executable behaviour contract
5. crates/                         implementation
6. docs/reference/, generated code derived artifacts — never hand-edited
```

Lower levels must never silently contradict higher ones. If implementation reality forces a
change to a higher level, **change the higher level explicitly** (edit the contract or write an
ADR) in the same commit, and note it in the commit body.

The narrative spec is explorative. Where it says "plausible", "suggested", "MAY" or leaves an
Open Design Question (spec §39), **you decide** — see §8.

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
  changes — and then the spec/ADR changes in the same commit.

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
4. Bootstrapping infrastructure (§14) outranks features when a quality gate cannot run.
5. A phase is complete only when its success criterion in spec §37 is demonstrated by an
   automated test — write that test explicitly and name it in `docs/STATE.md`.

---

## 10. Quality Gate (Definition of Done)

An increment is done only when **all** of these pass locally:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo xtask spec-check     # once xtask exists: contract ↔ implementation drift (spec §36.5)
cargo doc --workspace --no-deps   # no doc warnings
```

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
| Doc examples | via `cargo xtask` | every documented example must parse and execute |

Test quality rules: deterministic (no wall-clock, network or ordering dependence), isolated (no
shared mutable global state; never rely on the developer machine's real processes unless the
fixture creates them), one behaviour per test, assertion messages that explain the contract.

---

## 12. Commits and Version Control

- Work on `main` unless the user asked otherwise; **every commit must be green** (§10).
- One increment of **one kind** per commit (§4). Conventional Commits:
  `feat|fix|refactor|perf|test|docs|spec|chore(scope): summary`
- Body: what changed, which spec section it implements, which ADR it follows, which tests prove
  it. For `refactor`: state explicitly that no test changed.
- Never `--force`, never rewrite pushed history, never commit generated artifacts that the
  generator can reproduce (except where CI needs them checked in — then say so in an ADR).
- Do **not** push or open PRs unless the user asked for it.

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

## 14. Bootstrapping (empty or partial repository)

If the workspace does not exist yet, this is the ordered bootstrap backlog. Each step ends with
a green quality gate:

1. `Cargo.toml` workspace + `crates/ono-cli` + `crates/ono-core`, `rust-toolchain.toml`,
   `rustfmt.toml`, `clippy.toml`, and one behaviour test that fails first.
2. `xtask` crate: `cargo xtask spec-check`, `cargo xtask gen-docs`, `cargo xtask gen-tests`
   (spec §36, §47). Start as failing stubs with tests.
3. `docs/spec/` skeleton per spec §47 — `language.yaml`, `grammar.ebnf`, `verbs.yaml`,
   `targets.yaml`, `errors.yaml`, `capabilities.yaml`, `commands/`, `schemas/`, `providers/` —
   each with a loader + validation test.
4. `crates/ono-testkit`: fixtures, golden/snapshot helpers, property-test helpers, provider
   conformance harness generated from registry metadata (spec §35.3).
5. CI workflow running exactly the §10 gate.
6. `docs/STATE.md`, `docs/decisions/ADR-0001-*.md` recording the bootstrap choices.
7. Then Phase A of spec §37.

---

## 15. Code Style

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

## 16. Session Checklist

**Start:** read `AGENTS.md` → `docs/STATE.md` → recent ADRs → `git log --oneline -10` →
run the quality gate to confirm you start from green.

**During:** one TDD loop at a time; one kind of change at a time; decide instead of asking;
ADR for anything architectural.

**End:** gate green → `docs/STATE.md` updated (In progress cleared, Done/Next up accurate) →
committed → a two-line summary of what was proven by which tests and what the next task is.
