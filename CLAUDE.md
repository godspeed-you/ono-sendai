# CLAUDE.md — Ono-Sendai

@AGENTS.md

**`AGENTS.md` is the authoritative instruction set for this repository.** Read it in full before
your first action. This file adds only the Claude-Code-specific layer on top of it. Do not
duplicate rules here — if a rule needs to change, change it in `AGENTS.md`.

---

## The short version (full rules in AGENTS.md)

- **Project:** Ono-Sendai (binary: `ono`), an object-pipeline shell in Rust. Narrative spec:
  `docs/ono_sendai_shell_spec_v0.2.md` (normative MUST/SHOULD/MAY). Machine-readable contracts live
  in `docs/spec/`.
- **Naming:** the product is **Ono-Sendai**, the short name and binary is **`ono`**, crates are
  `ono-*`. **KUANG/11** is a different thing — the extension runtime of spec §31, not an old
  name for the shell. `kuang` belongs only in KUANG/11 context (AGENTS.md §3).
- **Everything an agent reads lives under `docs/`** — no top-level `spec/`. Read every
  `spec/...` path in the narrative spec as `docs/spec/...` (AGENTS.md §2).
- **Method:** strict TDD. `RED → GREEN → REFACTOR → GATE → RECORD → LOOP` (AGENTS.md §7).
  No production code without a failing test first.
- **Pragmatism:** solve the problem in front of you. Fixes, features, refactors and
  optimisations are separate changes and separate commits — never mixed (AGENTS.md §4).
- **Tests assert outcomes, not structure.** A pure refactor must leave the suite green *and
  unchanged*; a test that breaks on restructuring without behaviour change is itself the defect
  (AGENTS.md §11).
- **Autonomy:** every decision not fixed by the spec is yours. Decide, write an ADR in
  `docs/decisions/`, continue. Do not ask the user; do not idle (AGENTS.md §8).
- **Referee:** the test suite plus the quality gate (AGENTS.md §10) decide whether a (sub)goal is
  reached and the next step may start.
- **State board:** `docs/STATE.md` — read first, update last, every session (AGENTS.md §9).
- **Repo language is English** (code, tests, docs, commits). Talk to the user in their language.

Quality gate, must be green before every commit:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo xtask spec-check
```

---

## Claude-Code-specific guidance

**Plan mode.** For a multi-increment task, plan briefly, then execute. Do not use `ExitPlanMode`
to request permission for decisions that AGENTS.md §8 already delegates to you.

**AskUserQuestion.** Effectively banned for this project. Architecture, library and design
questions are answered by you with an ADR. Use it only for the escalation cases in AGENTS.md §8.

**Subagents.** Use them when they genuinely reduce context load or parallelise disjoint work:
- `Explore` for locating code and spec sections across the large spec file;
- `Plan` for decomposing a phase of spec §37 into increments;
- `general-purpose` agents for one crate each, following AGENTS.md §13 claiming rules.

Do **not** spawn subagents for a single-file edit or a task one TDD loop can finish. When you do
spawn several, launch them in one message so they run concurrently, and give each one the
AGENTS.md rules plus its exclusive file scope.

**Parallel tool calls.** Batch independent reads/greps/test runs into a single message.

**Long spec file.** `docs/ono_sendai_shell_spec_v0.2.md` is ~5800 lines. Do not read it whole; use
`grep -n '^#'` for the section index, then `sed -n 'A,Bp'` for the sections you need, and cite
sections as `§N` in ADRs, tests and commit messages.

**Commits.** Conventional Commits, one kind of change per commit, green tree only, trailer:
`Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>`.
Never push or open a PR unless asked.

**Reporting.** End a session with: what was proven, by which tests, and the next task from
`docs/STATE.md` — not a narration of the steps taken.
