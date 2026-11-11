# ADR-1021: A kind word loads only the package that declares it on disk

- Status: accepted
- Date: 2026-11-10
- Spec refs: v0.2 §31.68; v0.4 §6.8, §11.2, §20.4, §36.1; K11P (a package is loaded on its first
  invocation); ADR-1005, ADR-1009
- Issues: v0.6.5 security review finding S-F5
- Decided by: agent (autonomous)

## Context

ADR-1005 §2 had `jump <kind>/<key>`, `find place --type <kind>` and `jump @<pin>` load the
installed packages that can answer for a kind this session does not know: those whose on-disk
`contributions.schemas` documents call one of their targets' schemas by that name or id — **and
every package that declares targets but no schema document**, because only its handshake could say
what its schemas are called.

The second half made any word a reason to start code. `jump typo/whatever`, answered
`spatial.not_found`, still spawned every such package (`lifecycle.load` in the audit trail), and
so did a `--type` typo and a pin left behind by a removed package. K11P's rule is that a package
is loaded on its first invocation; a typo is no invocation of anything.

## Decision

1. A kind word loads exactly the installed packages whose on-disk `contributions.schemas`
   documents declare a schema of one of their targets **by that display name or id**. A word no
   declaration on disk names loads nothing, and the command answers as it would have.
2. A package that declares no schema document is not started for a kind word. What it can still
   do is everything else: it loads on its first invocation (`get <target>`, a contributed command,
   `load plugin`), a role it declares on a target loads it (`find place --role`), a relation it
   waits for loads it (ADR-1008), and once loaded its kinds are known to `jump`, `find place` and
   pins like any other.

## Consequences

- **Limitation:** in a fresh session, `jump <kind>/<key>`, `find place --type <kind>` and
  `jump @<pin>` do not reach the places of a package that names its schemas only in its handshake;
  they answer as for an unknown kind until something loads the package. A package that wants
  those words to work before its first invocation declares its schemas on disk — the Kubernetes
  package already does, and ADR-1009's scope declaration lives in the same document.
- The contract text is `docs/contracts/spatial/spatial.yaml` → `contributed_types.before_load`.
  ADR-1005 is superseded in that one part; its pin handling and its loading rule for roles and
  declared kinds stand.
- Encoded by `crates/ono-cli/tests/spatial_contributed_pins.rs`
  (`should_load_no_package_for_a_jump_to_a_kind_no_package_declares`,
  `should_load_no_package_for_a_search_by_a_kind_no_package_declares`,
  `should_load_no_package_for_a_pin_whose_kind_no_package_declares`,
  `should_reach_the_kinds_of_a_package_without_a_schema_document_once_it_is_loaded`); the suite's
  kind-word tests now lay the package out with its schema document, as the contract requires,
  and `docker/acceptance/cases/171-a-kind-word-loads-only-its-package.case`.

## Alternatives considered

**Keep loading schema-less packages, but only once per session per package.** That is what
ADR-1005 already did; the cost was never the repetition but that a typo runs code.

**Load schema-less packages only for a `jump @<pin>`, whose kind a session once saw.** A pin
records the kind's display name and not the package, so the pin does not say which package to
start either; the result would still be every schema-less package.

**Refuse to install or load a package without a schema document.** It breaks every such package
for every command to fix a speculative-load problem of three spatial words.
