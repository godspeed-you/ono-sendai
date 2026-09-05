# ADR-0056: Negotiation, identity pinning and conflict resolution

- Status: accepted
- Date: 2026-08-27
- Spec refs: v0.3 §1.6, §1.14–§1.16, §1.22, §1.24, §1.25, §1.46, §1.57; ADR-0052, ADR-0055
- Decided by: agent (autonomous)

## Context

Spec v0.3 §1.6 names six negotiation states, §1.25 seven ranking criteria and §1.46 a probe
cache, without saying how a declarative invocation matcher decides "unsupported", what
"executable identity" means for a contract that names a program by name, or when a conflict is
an error rather than a tie-break. `explain` must show all of it (§1.23) without spawning the
subject (v0.2 §15.3).

## Decision

1. **Negotiation is a pure function of (resolved executable, argv, demand)** —
   `Registry::negotiate`. A `RawBytes`, `Text` or `Discard` demand answers `RawPreferred`
   ("downstream bytes") when any adapter knows the program and `NotApplicable` otherwise;
   nothing is probed. `Structured` and `Interactive` demands consult every adapter whose
   `output_demand` includes them.

2. **Identity pinning (§1.22).** A contract that names an executable by bare name matches any
   resolved path with that basename — the program the shell would have run anyway. A contract
   that names an absolute path matches only that path; anything else is `ExecutableMismatch`.
   The plan pins the resolved path: PATH changes after negotiation change nothing.

3. **Version (§1.46).** An adapter whose range is not `any` needs a version. The probe runs
   the declared argv under the prober the shell injects, once per executable identity (path,
   device, inode, mtime, size) for the registry's lifetime; a probe that yields nothing is a
   refusal — `IncompatibleVersion { found: None }` — never an assumption.

4. **Invocation matching (§1.14–§1.16).** Arguments are split into flags (`-x`, `--x`,
   `--x=v`) and words. Every flag must be in `allow` or `allow_with_value` (the latter consumes
   the next word); the words must begin with one of `match.words`' alternatives (the longest
   wins); remaining words pass through only under `positionals: allow`. Anything else is
   `UnsupportedInvocation` naming the first offending token. Allowed flags and positionals are
   appended to the plan's argv in the order typed when `append_user_flags` is true. Combined
   short flags (`-ad`) are not decomposed: a spelling the contract did not enumerate is
   unsupported, which is the safe direction.

5. **Limits (§1.6).** Support is "with limits" when the adapter's field map leaves a schema
   field unreported; each limit reads `` `device` is not reported by findmnt `` and travels to
   `explain` and provenance.

6. **Ranking (§1.25).** Among adapters that answer with a plan: exact path match, then
   invocation specificity (the length of the matched word alternative), then tier
   (first-party > recommended > community > experimental), then the adapter's full id. Load
   order is never consulted, so any two registries holding the same packs answer alike. Two
   candidates with the same full id — one adapter installed twice — are `Conflict`, the only
   case the ranking cannot separate and the only one that raises `adapter.conflict`. When no
   adapter answers with a plan, the highest-ranked refusal is reported.

7. **Consequences by demand.** Under a `Structured` demand every refusal fails the stage
   with the matching `adapter.*` error (§1.18); under `Interactive` the adapter's `fallback`
   decides (`raw` today for every first-party adapter). `Negotiation::describe(demand)`
   renders the §1.57 states — `adapted by …`, `raw (downstream bytes)`,
   `raw (unsupported invocation: …)`, `unsupported invocation: …; fails`,
   `raw (version incompatible: …)`, `conflict: …` — and `explain` prints that text on an
   `adaptation` row, with `argv` and `candidates` rows when a plan exists.

8. **`explain` may probe.** The version probe is a declared, bounded, read-only invocation of
   a different program than the subject; running it does not execute the subject, and an
   `explain` that guessed the version would tell the user something the run might contradict.

## Consequences

- The executor (ADAPT-004) calls the same function with the same inputs, so `explain` and the
  run cannot disagree (v0.3 §1.53).
- Tests: `crates/ono-adapter/tests/negotiation.rs` (fourteen cases: exact argv, pass-through,
  refusals, probe cache, ranking under both load orders, conflict, pinning, limits, the
  diagnostic words), `crates/ono-cli/tests/builtins.rs`, acceptance case `073`.

## Alternatives considered

- Decomposing combined short flags — rejected for now: `-ad` may mean `-a -d` or `-a d`
  depending on the tool; a contract can list the combined spelling when it wants it.
- Probing lazily at execution only — rejected under point 8.
