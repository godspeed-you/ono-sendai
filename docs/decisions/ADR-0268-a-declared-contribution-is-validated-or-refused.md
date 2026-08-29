# ADR-0268: A declared contribution is validated or refused

- Status: accepted
- Date: 2026-08-29
- Spec refs: §31.5, §31.7, §31.22, §31.23, §31.27, §31.62
- Decided by: agent (autonomous, `close-spat`)

## Context

`contributions.views` and `contributions.annotations` were parsed out of a manifest, reported by
`inspect plugin`, included in the artifact digest — and nothing else. Nothing validated them and
nothing registered them. There is no `views.open`/`views.submit`/`views.close` in this build and
no `output.annotate`, so both declarations were accepted and then did nothing, which is the same
class of defect as an option nothing reads: the operator is shown a lens or an annotation key that
does not exist.

`docs/spec/kuang/contributions.v1.yaml` names the two rules that were missing:
`id-in-namespace` ("Every contributed id is inside the package's namespace and does not claim
`ono.*`") and, for views, the whole `ViewRegistry` row that has nowhere to be.

## Decision

**1. An annotation key outside the package's own namespace is `package.invalid`.** §31.23 says
declaring annotation keys "is what keeps an annotation from being an undeclared schema fork"; a key
outside the package's namespace is a claim on records the package does not own. It fails at parse
time, which is where manifest-before-code puts it.

**2. A view contribution is `package.incompatible`, naming the missing dimension.** §31.62 makes
the view protocol its own version dimension, and this host provides none of it. A refusal that says
"this host implements no view protocol to register it in" is the honest answer; listing the view in
`inspect plugin` and registering nothing is not.

This is the second of the two outcomes `docs/STATE.md`'s B-kuang-5 names — "a package declaring a
view registers it into `view`, **or** the manifest field is refused as unsupported rather than
silently accepted". Registering it is a tranche: the view protocol, the `ViewRegistry` and
§31.28's lifecycle, including "every view declares a deterministic non-interactive output for when
stdout is redirected". Refusing is one increment, and it removes the false claim today.

## Consequences

- A package that declares views cannot load on this host, and the message says why and what is
  missing. When the view protocol exists, this refusal is what gets replaced.
- Annotation keys are validated but still cannot be *emitted*: `output.annotate` does not exist.
  The declaration is now honest metadata rather than an unchecked one; emitting remains open work
  and is not claimed.
- Encoded by `ono-kuang-protocol/tests/manifest_validation.rs::should_refuse_an_annotation_key_outside_the_packages_namespace`,
  `::should_accept_an_annotation_key_inside_the_packages_namespace` and
  `::should_refuse_a_view_contribution_this_host_cannot_register`.

## Alternatives considered

- **Leaving the fields listed and inert** — the state this ADR removes.
- **Dropping the fields from the manifest contract** — the contract is right; §31.27 and §31.23
  are the spec. It is the host that does not implement them, and saying so is the host's job.
