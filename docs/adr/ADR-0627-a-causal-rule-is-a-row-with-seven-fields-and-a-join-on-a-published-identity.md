# ADR-0627: A causal rule is a row with seven fields and a join on a published identity

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §1.4, §15.1–§15.8, §17.6, §21.6, §37.4, §45.3; ADR-0012
- Decided by: agent (autonomous)

## Context

v0.5 §15.8 makes every built-in causal rule machine-readable and lists what a rule records:

```text
rule_id
input event kinds
required evidence strengths
identity constraints
time constraints if any
output relation
provider/source constraints
```

and closes with the sentence the whole causal model rests on: *"No renderer may create causal
language outside this registry."* §15.2 gives four examples of admissible evidence and one
prohibition — *"Temporal proximity is insufficient"* — and §15.5 puts association without causal
evidence in a separate class that must be *"visually and structurally distinct"*.

What §15.8 does not say is what a rule row looks like, how a correlation rule differs from a
causal one in the file, or how the gate refuses a rule that is registered and unimplemented. The
implementations land with the causal engine; the registry lands now, because AGENTS.md §7 puts the
contract before the code.

## Decision

### 1. Seven fields, under §15.8's own names

`docs/contracts/temporal/causality.yaml` carries `rules:`, one row per built-in rule, with exactly
the seven keys §15.8 names: `rule_id`, `input_event_kinds`, `required_evidence_strengths`,
`identity_constraints`, `time_constraints`, `output_relation`, `provider_source_constraints`. The
gate refuses a row missing any of them, because a rule missing one cannot be inspected against its
inputs — and inspectability is the entire justification for letting Ono say "caused".

`required_evidence_strengths` is a mapping from input event kind to the minimum strength that
input must carry, rather than a bare list, because a rule usually needs `authoritative` on one
side and less on the other. The link's own strength is the weakest of its inputs, which is §7.2's
composition rule and not a per-rule choice.

`time_constraints` may be `null`. A rule that needs no time bound is a better rule, and
`ono.action-launched-process` is one: the shell owns process creation, so the identity join is
complete on its own.

### 2. The identity constraint is the rule

Every one of the seven built-in rules joins on an identity a source actually published — a systemd
job path, an Ono `ActionId` propagated into a provider transaction, a kernel-reported parent
identity, a cgroup membership, an explicit provider causal token. None joins on proximity. That is
§15.2's prohibition made structural: a row whose `identity_constraints` describes a time window
rather than an equality is a correlation rule that has been filed in the wrong list, and a
reviewer can see it at a glance.

### 3. Correlation rules are a separate list that may only emit `correlated_with`

`correlation_rules:` carries the same seven fields plus `window`, which is required. The gate
refuses a correlation rule that emits a causal relation, refuses a causal rule that emits a
non-causal one, and refuses a correlation rule with no window. So the file cannot express the
failure §15.5 exists to prevent, and a rule cannot be promoted from association to cause by being
edited in place — it would have to be moved between lists, rewritten to join on an identity, and
reviewed.

### 4. `status:` says whether the engine runs the rule

Every row carries `status: declared` until its implementation lands, and `registered` afterwards.
ADR-0012 already fixed that a registry describes the whole product rather than the part that
exists today; `status` is how a reader tells which is which without inferring it from the absence
of code.

### 5. The registered set is a constant in the gate until the engine exposes its own

`xtask::temporal::BUILTIN_CAUSAL_RULES` and `BUILTIN_CORRELATION_RULES` hold the ten ids, and the
check compares `causality.yaml` against them in both directions: an unregistered built-in rule
fails the gate (§36.4), and so does a registry row naming a rule the engine will not run — because
a rule in the registry that nothing implements is a causal claim Ono cannot make.

**This is the half of §36.4's third rule that can bind today.** When the causal engine exposes its
rule registry — a `&'static [CausalRuleId]` of every rule it will evaluate, reachable from
`xtask` — the two constants are replaced by a read of it, and the check binds to the
implementation instead of to a second copy of the specification. Until then, the constant is v0.5
§15.2 and §15.5 verbatim and the comparison is real; what it cannot yet catch is an engine that
silently stops running a rule the registry still declares.

### 6. Renderer wording is data in the same file

`renderer_wording:` carries §15.6's four forbidden words and §45.3's two connector styles, and the
gate refuses a file that gives a causal edge and a non-causal edge the same style. §15.8 says no
renderer may create causal language outside this registry; the way to make that checkable is for
the permitted language to be in the registry.

## Consequences

- Ten rules are declared and inspectable before any of them runs, so the crate that implements
  them is written against a fixed contract rather than defining one as it goes.
- A new built-in rule is three edits in one increment: a row here, an id in the gate's constant,
  and the implementation. Any two of the three without the third fails the gate.
- A third-party rule is namespaced to its publisher, identifies its source and is capped at
  `asserted` (§37.4). `contributed_rules:` states those bounds; the broker enforces them.
- `why` can be conservative by construction: it has ten rules and no fallback, so "cause: unknown"
  is the answer whenever none of them fires, which §15.7 makes a valid outcome rather than an
  error.

## Alternatives considered

**One `rules:` list with a `causal: true|false` flag.** Rejected: the two kinds of rule have
different obligations — a window is required for one and suspicious in the other — and one list
would let a rule change kind by flipping a boolean.

**Free-form prose for `identity_constraints`.** Kept, deliberately, and it is the one field the
gate checks for presence rather than for shape. A join condition over spatial identities, cgroups
and provider tokens has no single machine-readable form that would not be a small query language,
and inventing one now would be the speculative generality AGENTS.md §4 forbids. The field is
required, it is reviewed, and the rule it describes is implemented against a test.

**Deferring the registry until the engine exists.** Rejected: AGENTS.md §7 puts the contract
first, and the six crates that will consume causal links need the vocabulary before the engine
produces any.
