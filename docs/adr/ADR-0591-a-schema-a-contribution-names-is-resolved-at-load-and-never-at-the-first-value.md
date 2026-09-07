# ADR-0591: A schema a contribution names is resolved at load, and never at the first value

- Status: accepted
- Date: 2026-09-07
- Spec refs: §31.7, §31.22, §31.23, §31.64, §31.68, §31.75; `docs/contracts/kuang/contributions.v1.yaml`
  (`target.schema`, `command.output`, `command.input`, `static_analysis.undeclared-id`);
  `docs/architecture/external-system-provider.md` §10.3, §33.3; ADR-0022, ADR-0582, ADR-0587
- Decided by: agent (autonomous)

## Context

A contributed target declares the schema its records carry, and a contributed command declares
the type it emits and the type it consumes. The supervisor checked the target's schema id for a
*prefix* — the package's own namespace or `ono.` — and checked a command's types not at all. A
schema id that was well-formed and inside the namespace loaded; whether the registry held such a
schema was discovered by the first record, which failed to decode or was refused as
`runtime.schema_violation`, after the package was registered, after `help` listed the target, and
after an operator had typed `get <target>`.

The Kubernetes provider found this and recorded it as latent: every one of its targets names a
schema it contributes, and `tests/contributions.rs` there holds the document, the handshake and the
wiring table to one another. That is one package guarding itself against a check the host should
have made. §31.75 lists `undeclared-id` — "a command or target id used but not declared" — among
the findings static analysis exists to catch cheaply, and a schema id nothing contributes is that
finding with the word "schema" in it. §33.3 of the provider contract states the principle
outright: "preventable mistakes should be impossible or loud", and a mistake that surfaces at the
first value is neither.

## Decision

**At load, after the handshake's schemas are registered and before any contribution is
registered, every schema id a contribution names is resolved against the registry the instance
will validate its values against. An id that does not resolve is `package.invalid`, naming the
contribution and the id.**

Concretely, in `ono-kuang-supervisor`'s load:

- a target's `schema` must be a schema this package contributed in the same handshake, or a
  core schema. The prefix check is gone; a registry lookup replaced it;
- a command's `output`, and its `input` where it declares one, are read the way the instance
  reads them for validation (`parse_expected`), and where that reading is a schema id, the id
  must resolve the same way. A declared scalar or container type (`stream<int>`) is a type, not a
  schema, and is unaffected;
- an id that does not parse as a schema id at all is refused with the shape a schema id has.

The check runs against the same `SchemaRegistry` the actor is built with, so the set of schemas a
contribution may name and the set the instance validates against are one set by construction.

## Consequences

A package whose declarations disagree with what it contributes never registers: no placeholder in
`help`, no target a user can type and watch fail, no quarantine of an instance that did nothing
wrong on the wire. The refusal says which target or command named which schema, which is what a
publisher needs to fix it.

The example package gained two misbehaviour modes — `--misbehave=phantom-target-schema` and
`--misbehave=phantom-command-schema` — that send a hello naming an uncontributed schema inside the
package's own namespace, precisely the case a prefix check passes. The tests that encode the
decision, in `crates/ono-kuang-sdk/tests/conformance.rs`:

- `should_refuse_to_load_a_target_whose_schema_the_package_never_contributed`;
- `should_refuse_to_load_a_command_whose_output_schema_the_package_never_contributed`.

What this does not check: the on-disk `contributions.targets` documents the shell reads before a
package runs (§31.68, ADR-0282) name schemas too, and the shell reads no on-disk schema document
to settle them against. A placeholder for a target whose schema will never exist is therefore
still possible until the package loads — at which point this decision refuses it. Reading the
on-disk schema documents at install time would close that too, and is a separate increment.

## Alternatives considered

**Keep the prefix check and rely on per-record validation.** Rejected: that is the state this
replaces. It puts the failure after registration, in the wrong error family, in front of a user
rather than a publisher.

**Check on-disk documents at install time instead of the handshake at load.** Rejected as the
only check: the handshake is what the instance actually contributes, and a package whose disk and
handshake disagree is refused on other grounds already (ADR-0282, ADR-0585). The handshake check
is the one that cannot be bypassed by an edited document, and the install-time check is an
addition rather than a substitute.

**Warn rather than refuse.** Rejected: §31.7 and `contributions.v1.yaml` fail registration checks
closed, and a warning nobody reads followed by a runtime failure is what this decision exists to
remove.
