# ADR-0508: A session is eight groups, each owning its own invariants

- Status: accepted
- Date: 2026-09-02
- Spec refs: v0.4.1 §23.1, §23.4, §24.1, §31.1–§31.4, §55.1, §65.12, §66.6, Appendix I.3;
  spec §14.1, §18.4, §20.2, §30; ADR-0010 (the config layers), ADR-0090 (the session tables),
  ADR-0094 (settings provenance), ADR-0161 (a link's agent is the shell's), ADR-0249/ADR-0458
  (retention), ADR-0457 (the capture budget)
- Decided by: agent (autonomous)

## Context

§31.1 sets the terms: "v0.4.1 does not attempt to make the shell stateless. A shell session
legitimately owns mutable state. The goal is to make categories of state explicit." `Session` had
twenty-seven fields in one flat struct — a working directory beside a tokio runtime beside a job
table beside a theme — and about a hundred methods reaching into any of them. Nothing said which
fields belonged together, so nothing said which invariants existed.

§31.2 names eight groups. §31.3 says what they are for: "each state group SHOULD own the
invariants for its data. For example, result-history byte-budget enforcement belongs in
`ResultHistoryState`, not scattered across evaluator call sites." §31.4 sets the boundary:
segmentation "MUST not accidentally turn ephemeral handles, runtimes or jobs into serializable
state". And Appendix I.3 lists the five things that must not change: config precedence,
environment mutation semantics, job reaping, navigation trail semantics, result-history
identifiers.

## Decision

**`Session` holds eight fields, one per group of §31.2, and keeps every method it had.**

```text
environment  EnvironmentState   cwd, env, inherited_env, env_provider
scope        ScopeState         scopes, definitions, expanding
execution     ExecutionState    status, executor, mode, interactive, leaving, runtime,
                                captures, capture_budget
navigation   NavigationState    frames, links, selection
history      ResultHistoryState results
jobs         JobState           native_jobs, job_started, tables
provider     ProviderState      providers, adapters, adaptations
presentation PresentationState  settings, theme
```

### 1. The public API did not move

§31.2 permits it — "the public API MAY continue to expose convenient methods on `Session` so
callers do not need to know the internal split" — and this takes the permission: every one of
`Session`'s methods kept its name, its signature and its visibility, and not one file outside
`session.rs` changed. The groups are private types with private fields; nothing outside the
module can name them, so the split cannot leak into a caller by accident.

### 2. Three groups own an invariant, and the rest own a category

§31.3 asks for mutation locality, and three places had a real invariant to move:

- **`ResultHistoryState::retain`** is the enforcement site §31.3 names. It narrows the four
  retention dimensions to what the settings declare, then retains under the redaction policy.
  `Session::retain` is now two lines that hand it the settings. No evaluator call site decides
  what fits, and there is exactly one path from a finished pipeline to a retained result.
- **`ExecutionState::capture`** holds the capture stack and the budget that bounds it in one
  type, and charges before it pushes. §23.1 forbids the buffer being "an invisible unlimited
  vector"; that it is not one is now a property of a five-field struct rather than of a
  convention spread over four methods.
- **`EnvironmentState::set_cwd`** owns the pairing of the session's directory and the process's:
  the kernel move happens first, and the field follows it or neither moves.

The other five groups are categories rather than enforcement points, and they are left as
categories. Inventing an invariant for `PresentationState` to make the list symmetrical would be
the speculative generality AGENTS.md §4 rules out.

### 3. `settings` lives with `theme`, and that is a judgement

§31.2 names no configuration group. The layered settings are read by nearly everything — limits,
spatial enablement, the presentation profile, the theme — so any home for them is a choice rather
than a derivation. They sit in `PresentationState` because the session-level thing they are read
*for* is how the shell behaves and how it looks, and because `Session::settings()` and
`settings_mut()` are unchanged, so no caller is affected either way. Appendix I.3's "different
config precedence" is `Settings`' own invariant, and `Settings` was not touched.

### 4. Nothing became serializable

§31.4, checked rather than asserted: none of the eight groups derives `Serialize`, `Deserialize`,
`Clone` or `Default`, and `session.rs` names `serde` nowhere. The runtime, the executor, the
provider registry, the adapter registry and the job table are exactly as un-serializable as they
were, which is the point — grouping ephemeral handles into a named struct is where a future
`#[derive(Serialize)]` would become tempting, so this ADR is the record that it must not be.

Drop order was considered and is immaterial: `Drop for Session` takes the links out with
`std::mem::take` and hangs them up before any field drops, so what remains is plain memory
release. `ExecutionState` is declared before `NavigationState`, which keeps the runtime dropping
before the links either way.

## Consequences

Easy: a reader asking "what does a session know about where it is?" reads a four-field struct
with its own doc comment. A change to retention is a change to `ResultHistoryState`. A new field
has an obvious home, and a field with no obvious home is a question worth asking.

Hard, or newly visible:

- **A method that spans two groups needs two disjoint borrows.** `Session::retain` borrows
  `history` mutably and `presentation` immutably; the compiler allows it because they are
  separate fields, and it would not if the groups had been one. That is the segmentation working,
  but it is a constraint future methods will meet.
- **`session.rs` is one file, not eight.** §31 draws no file layout, unlike §29.2 and §30.2, and
  the file's five capture sites are keyed by path in `docs/contracts/hardening/streaming.yaml`. A
  split would move them for no requirement, so it was not made.
- **`docs/contracts/hardening/streaming.yaml` moved one entry**: the capture stack is now held by
  `ExecutionState` rather than by `Session`. Same file, same class, same reason.

Encoded by: the whole suite unchanged and green, and specifically Appendix I.3's five —
config precedence (`crates/ono-cli/tests/meta_config.rs`), environment mutation
(`::context.rs`, `::data.rs`), job reaping (`::jobs_native.rs`, `::signals.rs`,
`::session_lifetime.rs`), navigation trail semantics (`::context.rs`,
`::spatial_navigation.rs`), and result-history identifiers (`::builtins.rs`,
`crates/ono-history/tests/result_history.rs`). Not one test file was edited.

## Alternatives considered

**Give each group a public type and expose it.** `session.environment().cwd()` reads well, and
§31.2 explicitly allows the opposite. Rejected: it would have rewritten every call site in the
crate, which is a diff nobody can review against a test suite that cannot tell the two apart —
§29.4's discipline, applied to §31.

**Split `session.rs` into `session/` the way `eval/` was split.** Rejected: §31 asks for state
groups, not modules, and the file's capture sites are path-keyed. The cost is registry churn and
a second edit to `xtask/src/scan.rs` for no requirement.

**A ninth `ConfigState` for `settings`.** Rejected: §31.2 enumerates eight and a session's
configuration is not a category of session state so much as the declaration the session was
built from. Putting it beside the theme keeps the count and states the reading.

**Move the retention ceilings into `ono-history` itself,** so `ResultHistoryState` would be a
newtype. Rejected: `ono_history::ResultHistory` already enforces all four dimensions; what lives
in `ono-cli` is reading the *settings catalogue*, which is `ono-cli`'s (§55.1). Moving it down
would have been the inversion §30.4 warns about, in the other direction.
