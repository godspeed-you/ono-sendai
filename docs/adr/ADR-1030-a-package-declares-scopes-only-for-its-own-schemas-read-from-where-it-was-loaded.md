# ADR-1030: A package declares scopes only for its own schemas, read from where it was loaded

- Status: accepted
- Date: 2026-11-11
- Spec refs: v0.2 §31.5, §31.68; external-system-provider §9.2; ADR-1009
- Issues: v0.6.5 security review finding S-F6; #224 (4)
- Decided by: agent (autonomous)

## Context

ADR-1009 lets a package's on-disk `contributions.schemas` document declare `scope: [<field>, …]`
for a schema, so the package's places sit in nested `plugin` scopes. Its consequences said the
declaration "is read from the plugin path the process started with". The v0.6.5 security review
found two faults in that (S-F6), and commit e1a6437c fixed them without recording the decision:

- `spatial::lazy::declare_scopes` passed every schema id in the document to the scope registry
  unchecked, so package A could declare the scope dimensions of package B's schema, and the last
  load won. §31.5 reserves `<publisher>.<package>.*` to its owner.
- It found the package under the process's `ONO_PLUGIN_PATH`/`HOME`, while the session loads a
  package from the session's plugin path. Two copies of a package on the two paths gave the
  instance of one and the scope of the other.

## Decision

**A package's scope declarations are read from the directory the session loaded that package
from, and a declaration for a schema outside the package's own namespace (`<package-id>.`) is
ignored.**

- `declare_scopes` takes the loaded package's directory, passed by `contributions::adopt` from the
  one place a package is loaded (`plugins.rs`); it no longer resolves a plugin path of its own.
- A schema id that does not start with `<package-id>.` is skipped: declaring where another
  package's places sit is that package's word alone. Nothing is refused at load, because the
  schema document is the package's own prose and ADR-1009 already declined to refuse a dimension
  it cannot check.

This supersedes ADR-1009 in part: the sentence that the declaration is read from the plugin path
the process started with.

## Consequences

- A package cannot move another package's places into a scope of its choosing, and the scope a
  place reports comes from the same copy of the package that answered for it.
- `spatial::lazy` keeps a second resolver of the plugin path for what it does before a package is
  loaded (finding which installed package declares a kind word, ADR-1005, ADR-1021); the review's
  preference F10 for one shared resolver stays a preference.

Encoded by `crates/ono-cli/src/spatial/lazy.rs` (unit, a package laid out on disk):
`should_ignore_a_scope_a_package_declares_for_a_schema_outside_its_namespace`,
`should_declare_the_scope_a_package_states_for_its_own_schema`; and by
`crates/ono-cli/tests/spatial_contributed_targets.rs::should_read_the_scope_from_the_package_the_session_loaded`.
The contract is `docs/contracts/spatial/spatial.yaml` → `contributed_types.scope`.

## Alternatives considered

- Refuse a package at load whose schema document names a foreign schema — rejected: ADR-1009 reads
  the document leniently by design, and a refusal would turn a harmless stray line into an
  unloadable package.
- Keep the process plugin path and document it — rejected: the scope would then describe a
  package the session did not load.
