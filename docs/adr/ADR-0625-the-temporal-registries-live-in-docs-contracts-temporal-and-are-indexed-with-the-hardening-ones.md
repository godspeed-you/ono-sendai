# ADR-0625: The temporal registries live in `docs/contracts/temporal/` and are indexed with the hardening ones

- Status: accepted
- Date: 2026-09-08
- Spec refs: v0.5 §36, §36.1–§36.4; v0.4.1 §52.1, §52.3; AGENTS.md §2, §14; ADR-0012, ADR-0547
- Decided by: agent (autonomous)

## Context

v0.5 §36 requires six version-controlled registries and names their home:

```text
docs/spec/temporal/
```

No such directory exists in this repository and none may be created. AGENTS.md §2 fixed that
mapping once, for the whole narrative specification: *"Read every `spec/...` path in the narrative
spec as `docs/contracts/...`."* `spec-check` fails the gate on a top-level `spec/` directory
precisely so the mapping cannot be quietly undone.

The second question is smaller and easier to get wrong. v0.4.1 §52.3 asks the gate to validate
**every** machine-readable contract, and `docs/contracts/hardening/registries.yaml` is what makes
"every" checkable rather than aspirational: a contract with no row there fails the gate, and a row
whose validator nobody wrote fails the gate (ADR-0547). That index reads one directory. Six new
registries one directory across would sit outside it, validated by a check that happens to exist —
which is the exact state `remote_limits.yaml` was in before the index was written.

## Decision

### 1. The six registries are `docs/contracts/temporal/*.yaml`

`temporal.yaml`, `events.yaml`, `evidence.yaml`, `causality.yaml`, `sources.yaml`,
`recorder.yaml`, with the contents §36.1 to §36.3 enumerate. §36's "Commands SHOULD live in
`docs/spec/commands/temporal.yaml`" maps the same way, onto
`docs/contracts/commands/temporal.yaml`, where every other command family already lives.

### 2. All six are required once the directory exists

A missing `docs/contracts/temporal/` is silence: a registry arrives with the phase that needs it
(AGENTS.md §14). A directory that exists and is missing one of the six is a failure, because §36
lists them as required and a half-written contract set makes a promise nobody can check. The
check therefore has one graceful degradation and no others.

### 3. They are indexed in the existing inventory, by relative path

`docs/contracts/hardening/registries.yaml` gains six rows whose `file` is
`../temporal/<name>.yaml`, each naming `xtask/src/temporal.rs::check` as its validator.
`xtask/src/temporal.rs::check` holds the inventory to it in the other direction: a temporal
registry with no row there fails the gate.

The alternative was a second index inside `docs/contracts/temporal/`. It was rejected because
§52.3's binding word is *every*, and two inventories is one inventory nobody reads. The cost is
that a row in a directory-scoped index names a path out of its directory, which the header now
says in as many words.

### 4. One check function holds all six

`xtask/src/temporal.rs::check` validates them together rather than one function per file, because
most of what can go wrong with these registries is a cross-reference between two of them: a causal
rule requiring an evidence strength nobody declares, a source naming an evidence class that does
not exist, a settings block that has drifted from the shell's catalogue. Six independent
validators would each be correct about its own file and blind to the joins.

## Consequences

- Every `spec/temporal/...` path in v0.5 reads as `docs/contracts/temporal/...`, consistently with
  every other path in every other enhancement specification.
- `xtask spec-check` fails on all six of §36.4's drift rules and on the internal consistency of
  the registries. `xtask/tests/temporal_contracts.rs` proves each refusal against a mutated copy
  of this repository's own registries.
- A seventh temporal registry cannot be added without a row in the inventory and a validator in
  `xtask/`, which is the property ADR-0547 bought and this decision keeps.
- The registries are contract-first: every one of them describes behaviour the implementations
  land later (AGENTS.md §7 step 1). `causality.yaml`'s rules carry `status: declared` so the file
  says which of the two states each row is in rather than implying both.

## Alternatives considered

**Create `docs/spec/temporal/` as the specification writes it.** Rejected: AGENTS.md §2 is
authoritative about the layout and `spec-check` enforces it. The specification is immutable and
its path is read through the mapping, which is what the mapping is for.

**Put the six files in `docs/contracts/` beside `verbs.yaml` and `targets.yaml`.** Rejected:
`docs/contracts/spatial/` and `docs/contracts/hardening/` already establish that a tranche with
its own vocabulary gets its own subdirectory, and six files with no shared prefix at the top level
would read as six unrelated contracts.

**Leave them out of the hardening inventory and rely on the check existing.** Rejected for the
reason ADR-0547 was written: a check that exists is not a check anybody can find, and the
inventory is the list of what the gate promises to hold.
