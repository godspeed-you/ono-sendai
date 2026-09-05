# ADR-0126: The spatial registry lives in `docs/contracts/spatial/`

- Status: accepted
- Date: 2026-08-28
- Spec refs: v0.4 §41 (machine-readable spatial registry), §41.1, §41.2, §42; v0.2 §27, §47;
  AGENTS.md §2; ADR-0012 (the registries), ADR-0026 (spec-check)
- Decided by: agent (autonomous); confirmed by the user 2026-08-28

## Context

v0.4 §41 recommends five files:

```text
docs/contracts/spatial.yaml
docs/contracts/relations.yaml
docs/contracts/spaces.yaml
docs/contracts/landmarks.yaml
docs/contracts/spatial-errors.yaml
```

`docs/contracts/` today holds the cross-cutting registries as flat files — `language.yaml`,
`grammar.ebnf`, `verbs.yaml`, `targets.yaml`, `errors.yaml`, `capabilities.yaml` — and one
directory per family: `commands/`, `schemas/`, `providers/`, `adapters/`, `kuang/`. The KUANG/11
contracts arrived in exactly this situation (spec §31.78 wrote `spec/kuang/`) and became
`docs/contracts/kuang/`.

Flat `relations.yaml` and `spaces.yaml` at the root of `docs/contracts/` would also read as
cross-cutting registries of the whole product rather than as the contracts of one subsystem,
which they are.

## Decision

1. The spatial contracts live in **`docs/contracts/spatial/`**:

   ```text
   docs/contracts/spatial/spatial.yaml     the subsystem's own registry (§41)
   docs/contracts/spatial/spaces.yaml      the canonical places and collections (§41.1)
   docs/contracts/spatial/relations.yaml   the relation vocabulary (§41.2)
   docs/contracts/spatial/landmarks.yaml   the landmark reasons and thresholds (§26)
   ```

   File names keep §41's spelling; only the directory is added.
2. **There is no `spatial-errors.yaml`** — the spatial error family lives in
   `docs/contracts/errors.yaml` (ADR-0125).
3. `spec-check` validates these files the way it validates the other registries, and holds them
   against the implementation in both directions (a served space or relation that is not
   declared, and a declared one that is not served, are both drift) — the rule
   `crates/ono-cli/tests/spatial_contracts_missing.rs` already tests.
4. Generated reference pages go to `docs/reference/`, produced by the xtask, never hand-edited.

## Consequences

- `docs/contracts/` keeps its shape: flat files are the product-wide registries, directories are
  subsystems.
- The RED suite accepts `docs/contracts/spatial/<name>` first and the flat spelling second, so it
  goes green either way; this ADR is what makes the choice, and the delivering increment may
  simplify the test to the chosen path.
- Anyone reading the specification finds the files one directory deeper than §41 writes them.
  This ADR is the pointer, exactly as AGENTS.md §2 is the pointer for `spec/` → `docs/contracts/`.

## Alternatives considered

- **Follow §41 literally, flat.** Rejected: four subsystem files at the root of `docs/contracts/`
  next to `verbs.yaml` and `errors.yaml` misrepresents their scope, and the repository already
  answered this question for KUANG/11.
- **Fold the spatial contracts into the existing registries** (`spaces` into `targets.yaml`,
  `relations` into a schema). Rejected: §41 requires them to be sufficient on their own to
  generate help, completion, tests and SDK bindings, and §42 hangs provider conformance on
  them; burying them in the command registry would lose that.
