# Ono-Sendai Autonomous Implementation Runbook

## Mission

Your task is to implement **the entire Ono-Sendai project** described by this repository until it is demonstrably complete, functional, tested, and release-ready according to the repository's specifications, contracts, acceptance criteria, and release gates.

You are not implementing a prototype, proof of concept, MVP, demo, partial subset, or representative sample.

You are implementing the complete product defined by the repository.

Continue working until the repository itself provides objective evidence that the implementation satisfies its requirements.

---

# 1. Authority and immutable input

Before doing anything else, inspect the repository and identify its authoritative documents.

The initial Ono-Sendai specification is an **immutable experimental input**.

## Absolute rule

You MUST NOT modify the initial specification document.

This includes:

- content changes;
- typo fixes;
- formatting changes;
- heading changes;
- whitespace-only changes;
- renaming;
- moving;
- regenerating;
- replacing;
- rewriting;
- automatically formatting it.

If the specification is ambiguous, incomplete, internally inconsistent, or leaves an implementation choice open, do NOT modify the specification.

Instead:

1. analyze the ambiguity;
2. choose the most reasonable interpretation consistent with the overall product;
3. record the decision in an ADR;
4. implement according to that decision;
5. continue.

The immutable specification describes human intent.

ADRs describe implementation-time interpretation.

Machine-readable contracts describe the resulting executable/public contract.

Tests verify that contract.

Implementation realizes it.

Do not reverse this direction.

---

# 2. Operating mode

Operate autonomously.

Do not stop merely because:

- a design decision must be made;
- several technically reasonable approaches exist;
- the specification leaves a detail open;
- an implementation is difficult;
- the next phase is large;
- additional refactoring is required;
- tests reveal architectural problems.

When a decision is required, make it.

Prefer decisions that are:

- consistent with the specification;
- architecturally coherent;
- production-quality;
- testable;
- maintainable;
- unsurprising to users;
- compatible with already established contracts;
- reversible where uncertainty is high.

Document meaningful architectural decisions as ADRs.

Do not ask the user to choose between implementation alternatives unless progress is genuinely impossible without information that cannot be derived from the repository or environment.

---

# 3. Definition of done

The project is NOT complete when:

- it compiles;
- unit tests pass;
- a single demo works;
- Phase A works;
- native commands work;
- object pipelines work;
- the happy path works;
- most acceptance criteria work;
- the remaining failures appear minor;
- documentation says the feature exists;
- you have produced a large amount of code.

The project is complete only when all applicable repository-defined completion and release criteria have been satisfied.

At minimum, completion requires:

1. all required implementation phases completed;
2. all stable public contracts implemented;
3. all required commands implemented;
4. all required value and schema semantics implemented;
5. Unix external-command interoperability implemented;
6. object pipelines implemented;
7. Linux providers implemented as specified;
8. context/navigation functionality implemented;
9. live/watch semantics implemented;
10. relationship/trace functionality implemented;
11. remote functionality implemented;
12. KUANG/11 implemented according to the specification;
13. required TUI functionality implemented;
14. documentation generated or updated where appropriate;
15. unit tests passing;
16. integration tests passing;
17. conformance tests passing;
18. acceptance tests passing;
19. release checks passing;
20. no known blocking defects remaining.

Use the repository's own gates and acceptance definitions as the authoritative executable evidence.

---

# 4. Never weaken the test to make the implementation pass

You MUST NOT obtain a green build by weakening the requirements.

Do not:

- delete failing tests because they are inconvenient;
- skip required tests;
- mark incomplete functionality as complete;
- disable checks;
- reduce acceptance criteria;
- convert required tests into ignored tests;
- loosen assertions merely to accommodate incorrect behavior;
- change contracts merely because implementation is difficult;
- silently remove unsupported features from registries;
- claim a phase complete when required functionality is missing.

Tests may be corrected when they are objectively wrong, inconsistent with higher-authority contracts, nondeterministic, or testing an invalid assumption.

When doing so, document why.

A passing test suite must mean the implementation is correct, not that the test suite has been made easier.

---

# 5. Use sub-agents extensively

Use sub-agents as a core implementation mechanism.

The project is intentionally too large to treat one conversation context as the only working memory.

Sub-agents should be used to:

- keep detailed problem contexts isolated;
- investigate subsystems independently;
- implement well-bounded work packages;
- review code written by other agents;
- analyze specification sections;
- write and improve tests;
- investigate failures;
- validate architecture;
- perform security review;
- perform compatibility review;
- inspect release readiness.

Do not delegate multiple agents to make overlapping uncontrolled edits to the same subsystem at the same time.

Prefer bounded ownership.

Example decomposition:

```text
parent/orchestrator
│
├── language/parser agent
├── execution/job-control agent
├── values/pipeline agent
├── Linux provider agent
├── renderer/TUI agent
├── context/navigation agent
├── graph/trace agent
├── remote/link agent
├── KUANG/11 runtime agent
├── KUANG/11 SDK/protocol agent
├── AI/model-broker agent
├── testing/acceptance agent
├── security/reliability reviewer
└── release/integration reviewer
```

The exact decomposition may change as the architecture evolves.

Use additional specialized agents whenever doing so improves correctness or preserves useful context.

---

# 6. Parent agent responsibilities

The parent agent is the project orchestrator.

It must maintain global coherence.

The parent must:

- understand the full specification;
- maintain the implementation plan;
- identify dependencies;
- select work packages;
- delegate work;
- integrate results;
- resolve conflicts;
- enforce architectural consistency;
- run phase gates;
- maintain repository state;
- detect incomplete areas;
- ensure no subsystem is forgotten;
- initiate reviews;
- continue until release readiness is demonstrated.

Do not allow sub-agent completion to be interpreted automatically as project completion.

Every result must be integrated and independently verified.

---

# 7. Persistent repository state is the project memory

Do not depend on conversational context as the sole record of project state.

Context may be compacted or lost.

The repository must contain enough persistent state that another agent could resume implementation without needing the previous conversation.

Maintain the repository's existing state/work tracking mechanism.

If the repository defines files such as:

```text
docs/STATE.md
docs/adr/
docs/contracts/
```

use them consistently.

Persistent state should make clear:

- completed work;
- current phase;
- current work packages;
- known failures;
- unresolved implementation issues;
- relevant ADRs;
- contracts created;
- tests still missing;
- next executable tasks.

Do not place transient reasoning dumps into the repository.

Persist conclusions, decisions, state and contracts.

---

# 8. Context-compaction resilience

Assume that the parent context may eventually be compacted.

Design your workflow so this does not threaten the project.

Before completing substantial milestones:

1. update persistent project state;
2. ensure important decisions are represented by ADRs;
3. ensure contracts are committed;
4. ensure tests represent expected behavior;
5. ensure unfinished work is explicitly listed.

Use sub-agents for detailed subsystem work so the parent does not need to retain every implementation detail.

When recovering from compacted context, reconstruct state from the repository rather than guessing from memory.

The repository is authoritative project memory.

---

# 9. Work from contracts outward

Where the repository calls for machine-readable contracts, implement contract-first.

A preferred direction is:

```text
immutable narrative specification
        ↓
ADR where interpretation is required
        ↓
machine-readable contract
        ↓
generated metadata / test fixtures where applicable
        ↓
tests
        ↓
implementation
```

Do not create public behavior casually in implementation code and document it afterwards.

Stable commands, schemas, errors, capabilities and protocols should be represented in their appropriate registries/contracts.

---

# 10. Test-driven implementation

Use test-driven development for behavior with meaningful semantics.

For each bounded unit of behavior:

1. identify the governing requirement;
2. identify or create the relevant contract;
3. write a failing test;
4. implement the behavior;
5. make the test pass;
6. run adjacent regression tests;
7. refactor if needed;
8. commit coherent progress.

Use the appropriate test level:

- unit tests for local semantics;
- integration tests for subsystem interaction;
- PTY tests for interactive shell behavior;
- conformance tests for providers/contracts;
- container tests for real installation/use;
- end-to-end tests for user-visible workflows.

---

# 11. Build the product vertically as well as horizontally

Avoid spending enormous periods constructing infrastructure that cannot yet demonstrate a user-visible path.

Within dependency constraints, regularly produce integrated vertical slices.

For example:

```text
parse
→ resolve command
→ execute provider
→ create typed values
→ pipe values
→ render result
→ acceptance test through real ono binary
```

Then extend those slices.

This exposes architectural mistakes earlier than isolated subsystem completion.

---

# 12. External command compatibility is mandatory

Ono is a Unix shell.

External programs must remain first-class citizens.

Verify real behavior for:

- foreground programs;
- interactive programs;
- stdin/stdout/stderr;
- pipelines;
- redirections;
- exit status;
- environment variables;
- cwd;
- signals;
- terminal resize;
- process groups;
- Ctrl-C;
- Ctrl-Z;
- background jobs;
- `fg`;
- `bg`;
- PTY allocation;
- executable scripts;
- shebang handling.

Do not treat successful invocation of `echo` as sufficient evidence of shell compatibility.

Use real interactive programs in PTY-based tests.

---

# 13. Ono-native structured behavior

Native Ono functionality must preserve structured values internally.

Do not reduce native object pipelines to formatted text pipelines.

Rendering is presentation.

The pipeline carries typed values.

Tests should explicitly prove this.

Where native values cross into external Unix programs, use the conversion/interoperability rules established by the specification and relevant ADRs.

---

# 14. Shell language consistency

Preserve Ono's core language identity.

Commands should follow the repository's established language principles, especially predictable verb-target semantics where specified.

Do not introduce arbitrary one-off command grammar because it is convenient for implementation.

Discoverability matters.

Help, completion, command registries and schemas should agree with runtime behavior.

---

# 15. KUANG/11

KUANG/11 is not optional polish.

It is a major Ono-Sendai subsystem.

Implement it as the specification describes.

It must eventually support the defined extension model for functionality such as:

- analysis programs;
- system investigation tools;
- providers;
- commands;
- structured data processors;
- views;
- interactive lenses;
- assistants;
- AI-backed analysis;
- model-mediated workflows.

Treat KUANG/11 as a security boundary.

Plugins and assistants must not receive unrestricted ambient authority.

Implement capability-based access, lifecycle management, isolation and audit behavior as specified.

AI assistants must use the same controlled runtime principles.

The model may reason and request actions.

Ono retains authority over:

- available context;
- tool exposure;
- capability checks;
- user confirmation where required;
- execution;
- auditability.

Do not insert an LLM directly into privileged execution paths.

---

# 16. Security

Security review is required, particularly for:

- shell execution;
- quoting;
- path handling;
- environment propagation;
- privilege boundaries;
- remote connections;
- plugin execution;
- KUANG/11 capability enforcement;
- model/tool invocation;
- untrusted structured data;
- terminal escape handling;
- command injection;
- prompt injection;
- serialization boundaries.

Use dedicated review agents periodically.

Security findings must result in tests whenever practical.

---

# 17. Performance

Do not postpone all performance work until the end.

Measure critical paths as they become real.

Important areas include:

- startup latency;
- parser latency;
- pipeline throughput;
- backpressure;
- large structured streams;
- rendering large tables;
- provider enumeration;
- live watches;
- remote streams;
- plugin IPC;
- memory consumption.

Do not prematurely micro-optimize.

But do not ship architectural performance failures that could have been detected early.

---

# 18. Reviews

At meaningful integration points, delegate independent review agents.

Reviewers should assume the implementation may be wrong.

They should look for:

- missing requirements;
- contract drift;
- untested behavior;
- incorrect edge cases;
- unsafe assumptions;
- race conditions;
- resource leaks;
- process-control errors;
- compatibility regressions;
- architectural duplication;
- dead code;
- missing error handling;
- fake/stub behavior;
- documentation claims not supported by implementation.

A reviewer should not merely summarize the implementation.

It should try to falsify its correctness.

---

# 19. No fake completion

Search explicitly for incomplete implementation markers before declaring success.

Examples include:

```text
TODO
FIXME
unimplemented!
todo!
panic!("not implemented")
stub
placeholder
temporary
mock-only production path
return Ok(()) // without required behavior
```

Not every TODO is necessarily release-blocking, but every one must be inspected.

Also search for specification sections, registry entries or acceptance criteria that lack implementation/test coverage.

---

# 20. Continuous integration cycle

Repeatedly run the strongest applicable verification available.

Typical cycle:

```text
format
lint
compile
unit tests
integration tests
spec/contract checks
provider conformance
container acceptance
release checks
```

Do not wait until the very end to discover that the system fails its real acceptance environment.

---

# 21. Phase completion

Before marking a phase complete:

1. enumerate the requirements of the phase;
2. map them to implementation;
3. map them to tests;
4. run the relevant gates;
5. use an independent review agent;
6. resolve review findings;
7. update persistent state.

Then continue immediately to the next phase.

Do not stop after a phase merely to report progress.

---

# 22. Release-readiness loop

When implementation appears complete, enter a dedicated release-hardening loop.

Repeat:

```text
run all gates
↓
run full acceptance suite
↓
run release check
↓
perform independent code review
↓
perform security review
↓
perform compatibility review
↓
inspect incomplete markers
↓
inspect uncovered contracts
↓
perform real interactive smoke tests
↓
fix all release-blocking findings
↓
repeat
```

Continue until no release-blocking issue remains.

---

# 23. Final proof

Do not conclude with a subjective statement such as:

```text
"The project appears complete."
```

Provide objective evidence.

The final report must include at least:

- commit/revision tested;
- build result;
- complete test result;
- acceptance result;
- release-check result;
- major subsystem status;
- known remaining issues;
- whether any requirement remains knowingly incomplete.

If any required subsystem or acceptance criterion remains incomplete, the project is not complete.

Continue working.

---

# 24. Stop conditions

You may stop only when one of these conditions is true.

## SUCCESS

The complete product has been implemented and objective repository-defined release criteria pass.

## HARD EXTERNAL BLOCKER

Progress is genuinely impossible because of something outside the repository and outside your ability to resolve, such as unavailable credentials, unavailable external infrastructure, or an inaccessible required dependency.

If this occurs:

1. exhaust reasonable alternatives first;
2. document exactly what is blocked;
3. document everything already completed;
4. make the remaining work mechanically resumable;
5. state the smallest external action required to unblock progress.

Do not use uncertainty, difficulty, context size or amount of remaining work as a blocker.

---

# 25. First actions

Begin now.

First:

1. inspect the complete repository tree;
2. read the immutable initial specification;
3. read `AGENTS.md`;
4. read project state and acceptance documentation;
5. inspect existing ADRs;
6. inspect CI, scripts, contracts and tests;
7. establish the current actual implementation state;
8. identify the next dependency-valid work package;
9. create or update persistent implementation state if needed;
10. delegate appropriate bounded work to sub-agents;
11. begin implementation.

Do not return with only a plan.

Implement.

Continue until the stop conditions above are satisfied.

---

# Bootstrap prompt for a coding session

Use this short prompt when starting the actual coding session:

> Work autonomously on this repository until Ono-Sendai is fully implemented, demonstrably functional, and release-ready.
>
> Before doing any implementation, read `AUTONOMOUS_IMPLEMENTATION.md` in full and treat it as the binding orchestration instructions for this entire run. Then read and obey the repository's authority chain, including `AGENTS.md`, the immutable initial Ono-Sendai specification, project state, ADRs, contracts, acceptance criteria, and release gates.
>
> Use sub-agents extensively for bounded implementation, analysis, testing, review, security, and release work so detailed subsystem contexts do not all depend on the parent session context. Persist project state, decisions, contracts, and unfinished work in the repository so context compaction or session continuation does not lose essential state.
>
> The initial specification is immutable and must never be modified in any way. Resolve ambiguities through ADRs.
>
> Do not stop after planning, a phase, an MVP, or a mostly working implementation. Continue implementing, testing, integrating, reviewing, and fixing until the repository provides objective evidence that the complete product satisfies its release criteria, or until a genuine external blocker makes further progress impossible.
>
> Start by inspecting the repository and then begin implementation immediately.