# ADR-1005: A spatial command loads the lazy package whose kind of place it names, and a pin nobody can read costs only itself

- Status: superseded by ADR-1021 (in part: that a package declaring targets but no schema document is loaded for any unknown kind word)
- Date: 2026-11-10
- Spec refs: v0.4 §6.5, §6.8, §11.2, §20.4, §36.1, §46.1; v0.2 §31.68; external-system-provider
  §15.5; ADR-0584, ADR-0596
- Issues: #129
- Decided by: agent (autonomous)

## Context

A kind of place a KUANG/11 package contributes is registered from the package's handshake
(ADR-0584), and an installed package is lazy: spec §31.68 loads it on its first invocation. A
contributed *command* or *target* is such an invocation — `get k8s-namespace` loads the package —
but the spatial commands are the shell's own, so nothing ever loaded a package for them. #129 found
three consequences with the Kubernetes package:

- `pins.json` held a pin whose `object_type` was `KubernetesNamespace`. In a new session nobody had
  loaded the package, `SpatialType::from_name` did not know the word, and `PinStore::load` refused
  the *whole store* with `spatial.unsupported`. Every command that reads pins — `jump`, `unpin`,
  `find place` — failed with an error about a pin it had not been asked about, and editing the file
  by hand was the only way out.
- `find place --type KubernetesPod` and `--role workload` named a kind and a role the installed
  package declares, and were refused because the package was not loaded.
- `jump kubernetespod/<name>` was `spatial.not_found` for the same reason.

§20.4 is explicit about the first: "If the target cannot be resolved later, the pin remains but
reports unresolved state." A pin that cannot be read is one unresolved pin, not an unreadable
store.

## Decision

### 1. A pin whose type this session does not know costs that pin and nothing else

`PinStore::load` leaves such a pin out of the registry instead of refusing. It stays on disk:
`PinStore::save` carries every such pin over from the file, so writing another pin never drops it.
`PinStore::unplaced` lists them, `unpin <name>` removes one by name, and `jump @<name>` refuses it
with `spatial.unsupported` naming the pin and its type and saying how to remove it. Nothing else
reports it, because nothing else asked about it. A pin whose *identity* does not parse still
refuses the store: that is a file this shell did not write, which is the case the existing refusal
is for.

### 2. A spatial command naming an unknown kind or role loads the installed package for it

Before a pipeline's native stages run, the evaluator reads the words of `jump` and `find place`
stages (`crate::spatial::lazy::load_contributors`) and loads, silently and exactly as a
contributed command loads its package, the installed packages that can answer for:

- **a role** no loaded package declares — the packages whose on-disk `contributions.targets`
  declare that role (the declaration suffices, so nothing else is loaded);
- **a kind** no declared or loaded type answers to — in `jump <kind>/<key>`, `find place --type
  <kind>`, or the type a pin named by `jump @<pin>` recorded — the packages whose on-disk
  `contributions.schemas` documents call one of their targets' schemas by that name or id, and
  every package that declares targets but no schema document, because only its handshake can say
  what its schemas are called.

A package that will not load is left as it was, and the command then refuses as it always did.

### 3. `jump @<pin>` reads the pinned place again when the session has not observed it

A pin outlives its session and a new session's index is empty, so the identity a pin stored names
a place nobody has observed. Where the pin's place is not in the index, `jump` asks the canonical
target of the pin's kind and resolves the pin against what it answered — by identity, or by the
resilient selector where the identity is gone (§20.4).

## Consequences

- `pin` on a contributed place keeps working across sessions; a removed package leaves a pin that
  reports itself and can be removed; `find place --type/--role` and `jump <kind>/<key>` reach a lazy
  package's places. The pin store format is unchanged (version 1).
- The cost of an unknown kind word is bounded by the installed set and paid once per session: the
  packages stay loaded. A package without on-disk schema documents is loaded for any unknown kind
  word in a `jump <kind>/<key>` or `--type`; the Kubernetes package declares its schemas on disk,
  so it is loaded only for its own kinds.
- The evaluator gained one call (`crates/ono-cli/src/eval/pipeline.rs`), behind the `spatial` and
  `kuang` features, and `plugin_registry` gained `declared_target_roles`, an accessor beside
  `declared_target_names`.
- Encoded by `crates/ono-cli/tests/spatial_contributed_pins.rs` (ten outcome tests) and
  `docker/acceptance/cases/165-pins-on-contributed-places.case`.

## Alternatives considered

**`pin` refuses a contributed place** (the issue's first alternative: "`pin` accepts only what
`jump @pin` can resolve later"). It closes the defect by taking the feature away, and a removed
package would still leave the same stale pin behind.

**Store the contributing package's id in each pin.** Precise for new pins, useless for the pins
already on disk, and a second identity field for something the on-disk declarations already
answer.

**Load the package from inside the command.** A spatial command runs as an asynchronous command
implementation with the provider registry and no session, and loading a package needs the
session; the evaluator is the one place that holds both before the command runs.
